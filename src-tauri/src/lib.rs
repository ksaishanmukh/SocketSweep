//! SocketSweep desktop host.
//!
//! Orchestrates the on-device daemon over ADB, owns the scanned tree, and
//! exposes it to the frontend as small typed queries.
//!
//! # Why the frontend does not get the tree
//!
//! A device holds well over a hundred thousand files. Keeping the tree in
//! [`arena`] and serving the few hundred rows the frontend is about to draw
//! bounds every IPC payload by what fits on screen rather than by the size of
//! the device.
//!
//! Nodes are addressed by id rather than path for a second reason: Android
//! filenames are arbitrary bytes, and a path round-tripped through a JavaScript
//! string can come back subtly different — as a delete target. Every id travels
//! with the generation of the tree it came from, so one held across a rescan
//! cannot name a different file.
//!
//! # Threads and locks
//!
//! Commands that talk to the phone (`list_devices`, `connect`, `disconnect`,
//! `scan`, `delete`) run on a blocking worker thread, so the window stays
//! responsive however long the phone takes. Tree queries answer from memory and
//! run synchronously.
//!
//! No lock is held across a call to adb or the daemon. The session lives in an
//! `Arc` that a command clones out of its lock before using it, and only one of
//! connect, scan or delete runs at a time (see [`Busy`]). `disconnect` and app
//! exit are deliberately outside that rule: they cancel a running scan rather
//! than wait for it.
//!
//! Where several locks are taken together the order is `busy`, `session`,
//! `tree`.

pub mod adb;
pub mod arena;
pub mod history;
pub mod session;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use socketsweep_protocol::Frame;
use tauri::{AppHandle, Emitter, Manager, State};

use adb::{Adb, Device};
use arena::{AppUsage, Arena, Crumb, NodeId, Row, Stats, TreemapNode, TypeGroup, View};
use history::ScanRecord;
use session::{ScanCancel, Session};

/// Rows per view response. Enough to fill any screen; the frontend virtualises
/// and asks for more if it needs them.
const DEFAULT_VIEW_LIMIT: usize = 500;

/// How often the host pushes an updated view during a scan. 10Hz is fast enough
/// that numbers look live and slow enough that React is not re-rendering on
/// every one of several thousand frames.
const VIEW_PUSH_INTERVAL: Duration = Duration::from_millis(100);

const NOT_CONNECTED: &str = "not connected to a device yet";
const NO_SCAN: &str = "no scan has been run yet";

/// Lock a mutex, recovering the data if a thread panicked while holding it.
///
/// Every value behind these locks is left consistent between statements, and a
/// poisoned lock would otherwise turn one bug into every later command failing.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The scanned tree and which directory of it the frontend is showing.
///
/// Kept under one lock so the directory being watched always belongs to the
/// tree it is read from: a new tree starts out watching its root.
struct Tree {
    arena: Arena,
    /// Only this view is pushed during a scan, so the payload does not grow
    /// with the tree.
    watching: NodeId,
}

impl Tree {
    fn new(arena: Arena) -> Self {
        Tree {
            arena,
            watching: arena::ROOT,
        }
    }
}

/// The long-running operation in progress, if any. Only one runs at a time.
enum Busy {
    Connecting,
    Scanning(Arc<ScanCancel>),
    Deleting,
}

impl Busy {
    /// Why `next` cannot start while `self` is running.
    fn refuse(&self, next: &Busy) -> String {
        match (self, next) {
            (Busy::Scanning(_), Busy::Deleting) => "Wait for the scan to finish before deleting.",
            (Busy::Scanning(_), _) => "a scan is already running",
            (Busy::Connecting, _) => "already connecting to a device",
            (Busy::Deleting, _) => "a delete is still in progress",
        }
        .into()
    }
}

/// Marks an operation as running for as long as it is held.
struct BusyGuard<'a>(&'a AppState);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *lock(&self.0.busy) = None;
    }
}

#[derive(Default)]
pub struct AppState {
    adb: Mutex<Option<Arc<Adb>>>,
    session: Mutex<Option<Arc<Session>>>,
    tree: Mutex<Option<Tree>>,
    busy: Mutex<Option<Busy>>,
    /// Bumped by every disconnect, so a connect that was already under way
    /// when the user disconnected does not install its session afterwards.
    disconnects: AtomicU64,
}

impl AppState {
    fn claim(&self, next: Busy) -> CmdResult<BusyGuard<'_>> {
        let mut busy = lock(&self.busy);
        if let Some(current) = busy.as_ref() {
            return Err(current.refuse(&next));
        }
        *busy = Some(next);
        Ok(BusyGuard(self))
    }

    /// Stop a running scan, if there is one.
    fn cancel_scan(&self) {
        if let Some(Busy::Scanning(cancel)) = lock(&self.busy).as_ref() {
            cancel.cancel();
        }
    }

    fn session(&self) -> CmdResult<Arc<Session>> {
        lock(&self.session)
            .clone()
            .ok_or_else(|| NOT_CONNECTED.into())
    }

    /// Cancel any scan, forget the tree and stop the daemon.
    fn end_session(&self) {
        self.disconnects.fetch_add(1, Ordering::SeqCst);
        // Before anything else: the daemon serves one connection at a time, so
        // the shutdown request below would otherwise queue behind the scan.
        self.cancel_scan();
        let session = lock(&self.session).take();
        *lock(&self.tree) = None;

        let adb = lock(&self.adb).clone();
        if let (Some(session), Some(adb)) = (session, adb) {
            session.stop(&adb);
        }
    }
}

type CmdResult<T> = Result<T, String>;

/// Run `f` on a blocking worker thread, off the thread that drives the window.
async fn off_main<T, F>(app: AppHandle, f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle, &AppState) -> CmdResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        f(&app, &state)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

// ── Resource resolution ─────────────────────────────────────────────────────

fn bundled(app: &AppHandle, file_name: &str) -> CmdResult<std::path::PathBuf> {
    let dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("cannot locate the resource directory: {e}"))?;

    let path = dir.join("bin").join(file_name);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!(
            "bundled binary '{file_name}' is missing. Run `npm run setup` if you are running from source."
        ))
    }
}

/// A binary we execute on this machine. Windows needs the `.exe` suffix.
fn host_binary(app: &AppHandle, name: &str) -> CmdResult<std::path::PathBuf> {
    let file_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    bundled(app, &file_name)
}

/// A payload we push to the phone. Always an Android ELF, never suffixed —
/// which is why it cannot share a code path with [`host_binary`].
fn device_payload(app: &AppHandle, name: &str) -> CmdResult<std::path::PathBuf> {
    bundled(app, name)
}

/// The adb connection, made on first use.
///
/// The lock is held while connecting so two callers cannot both start an adb
/// server; nothing else needs it in the meantime.
fn adb(app: &AppHandle, state: &AppState) -> CmdResult<Arc<Adb>> {
    let mut guard = lock(&state.adb);
    if let Some(adb) = guard.as_ref() {
        return Ok(adb.clone());
    }
    let adb = Arc::new(Adb::connect(&host_binary(app, "adb")?).map_err(|e| e.to_string())?);
    *guard = Some(adb.clone());
    Ok(adb)
}

// ── Commands: connection ────────────────────────────────────────────────────

#[tauri::command]
async fn list_devices(app: AppHandle) -> CmdResult<Vec<Device>> {
    off_main(app, |app, state| {
        adb(app, state)?.devices().map_err(|e| e.to_string())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub serial: String,
    pub model: String,
    pub root: String,
    /// The last finished scan of this device and root, if we have seen it
    /// before. Captured at connect so a rescan still compares against the
    /// previous session rather than against the scan a moment ago.
    pub previous: Option<ScanRecord>,
}

/// Start a session on a device, replacing any current one.
///
/// `root` must be one of the roots [`session::check_root`] accepts; it defaults
/// to `/sdcard`.
#[tauri::command]
async fn connect(
    app: AppHandle,
    serial: Option<String>,
    root: Option<String>,
) -> CmdResult<Connected> {
    off_main(app, move |app, state| {
        connect_blocking(app, state, serial, root)
    })
    .await
}

fn connect_blocking(
    app: &AppHandle,
    state: &AppState,
    serial: Option<String>,
    root: Option<String>,
) -> CmdResult<Connected> {
    let _busy = state.claim(Busy::Connecting)?;
    let disconnects = state.disconnects.load(Ordering::SeqCst);

    let root = root.unwrap_or_else(|| session::DEFAULT_ROOT.into());
    session::check_root(&root)?;
    let daemon = device_payload(app, "daemon")?;
    let adb = adb(app, state)?;

    let serial = adb.resolve(serial.as_deref()).map_err(|e| e.to_string())?;
    let model = adb
        .devices()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.serial == serial)
        .map(|d| d.model)
        .unwrap_or_else(|| serial.clone());

    // The old phone's tree goes first, so a reconnect that fails is not left
    // serving it.
    *lock(&state.tree) = None;
    let old = lock(&state.session).take();
    if let Some(old) = old {
        old.stop(&adb);
    }

    let session = Session::start(&adb, &serial, &daemon, &root).map_err(|e| e.to_string())?;

    {
        let mut slot = lock(&state.session);
        if state.disconnects.load(Ordering::SeqCst) != disconnects {
            drop(slot);
            session.stop(&adb);
            return Err("disconnected while connecting".into());
        }
        *slot = Some(Arc::new(session));
    }

    let previous = history::previous(app, &serial, &root);

    Ok(Connected {
        serial,
        model,
        root,
        previous,
    })
}

/// Stop the daemon and forget the tree. Cancels a running scan, whose `scan`
/// call then fails with "the scan was cancelled".
#[tauri::command]
async fn disconnect(app: AppHandle) -> CmdResult<()> {
    off_main(app, |_, state| {
        state.end_session();
        Ok(())
    })
    .await
}

// ── Commands: scanning ──────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgress {
    stats: Stats,
    /// The view the frontend said it was looking at, refreshed. Sending only
    /// this keeps the payload constant no matter how large the tree grows.
    view: Option<View>,
}

/// Walk the device and stream the tree into the arena.
///
/// Emits `scan-progress` roughly ten times a second while running, and returns
/// the final totals once the walk completes. The previous tree stays queryable
/// until the daemon confirms the new walk has started.
#[tauri::command]
async fn scan(app: AppHandle, root: Option<String>) -> CmdResult<Stats> {
    off_main(app, move |app, state| scan_blocking(app, state, root)).await
}

fn scan_blocking(app: &AppHandle, state: &AppState, root: Option<String>) -> CmdResult<Stats> {
    let cancel = Arc::new(ScanCancel::default());
    let _busy = state.claim(Busy::Scanning(cancel.clone()))?;
    let session = state.session()?;

    // The label the user recognises, kept separate from the resolved path the
    // daemon reports.
    let root = match root {
        Some(root) => {
            session::check_root(&root)?;
            root
        }
        None => session.root.clone(),
    };

    let mut started = false;
    let mut last_push = Instant::now();
    let mut place_error: Option<String> = None;

    // Each frame is applied under the tree lock and only if the scan has not
    // been cancelled, checked under that same lock: a disconnect cancels before
    // it clears the tree, so a frame can never land after the clear.
    let result = session.scan(root.as_bytes(), &cancel, |frame| match frame {
        Frame::ScanStarted { root: resolved } => {
            // The arena is keyed on the resolved root, not the requested one:
            // the daemon canonicalises, so it walks /storage/emulated/0 where
            // we asked for /sdcard, and every frame is keyed on that.
            let mut tree = lock(&state.tree);
            if !cancel.is_cancelled() {
                *tree = Some(Tree::new(Arena::new_labelled(&resolved, root.as_bytes())));
                started = true;
            }
        }
        Frame::Dir { path, entries } if started => {
            {
                let mut tree = lock(&state.tree);
                if cancel.is_cancelled() {
                    return;
                }
                if let Some(tree) = tree.as_mut() {
                    if let Err(e) = tree.arena.apply_dir(&path, &entries) {
                        // A frame we cannot place means the tree is already
                        // inconsistent; surface it rather than drawing a lie.
                        place_error.get_or_insert_with(|| e.to_string());
                    }
                }
            }

            if last_push.elapsed() >= VIEW_PUSH_INTERVAL {
                last_push = Instant::now();
                emit_progress(app, state);
            }
        }
        Frame::ScanDone(_) if started => {
            // Authoritative end of walk; the arena cannot infer it.
            let mut tree = lock(&state.tree);
            if let (false, Some(tree)) = (cancel.is_cancelled(), tree.as_mut()) {
                tree.arena.finish();
            }
        }
        _ => {}
    });

    result?;
    if let Some(e) = place_error {
        return Err(e);
    }
    if !started {
        return Err(session::SCAN_CUT_OFF.into());
    }

    let stats = with_tree(state, |t| t.arena.stats())?;
    history::record(app, &session.serial, &root, &stats);
    emit_progress(app, state);
    Ok(stats)
}

fn emit_progress(app: &AppHandle, state: &AppState) {
    let payload = {
        let guard = lock(&state.tree);
        let Some(tree) = guard.as_ref() else { return };
        ScanProgress {
            stats: tree.arena.stats(),
            view: tree.arena.view(tree.watching, DEFAULT_VIEW_LIMIT).ok(),
        }
    };

    let _ = app.emit("scan-progress", payload);
}

// ── Commands: queries ───────────────────────────────────────────────────────

fn with_tree<T>(state: &AppState, f: impl FnOnce(&mut Tree) -> T) -> CmdResult<T> {
    let mut guard = lock(&state.tree);
    let tree = guard.as_mut().ok_or(NO_SCAN)?;
    Ok(f(tree))
}

/// The rows for one directory, largest first.
///
/// Also records which directory the frontend is showing, so scan progress
/// pushes stay scoped to it.
#[tauri::command]
fn get_view(
    state: State<'_, AppState>,
    id: Option<NodeId>,
    limit: Option<usize>,
) -> CmdResult<View> {
    let id = id.unwrap_or(arena::ROOT);
    with_tree(&state, |t| {
        let view = t.arena.view(id, limit.unwrap_or(DEFAULT_VIEW_LIMIT))?;
        t.watching = id;
        Ok(view)
    })?
    .map_err(|e: arena::ArenaError| e.to_string())
}

#[tauri::command]
fn get_breadcrumbs(state: State<'_, AppState>, id: NodeId) -> CmdResult<Vec<Crumb>> {
    with_tree(&state, |t| t.arena.breadcrumbs(id))?.map_err(|e| e.to_string())
}

/// A depth-limited slice of the tree, so the treemap can draw children nested
/// inside their parent tile without the frontend holding the tree.
#[tauri::command]
fn get_treemap(
    state: State<'_, AppState>,
    id: Option<NodeId>,
    depth: Option<usize>,
) -> CmdResult<TreemapNode> {
    let id = id.unwrap_or(arena::ROOT);
    with_tree(&state, |t| t.arena.treemap(id, depth.unwrap_or(2)))?.map_err(|e| e.to_string())
}

/// The largest files anywhere in the tree, regardless of depth.
#[tauri::command]
fn largest_files(state: State<'_, AppState>, limit: Option<usize>) -> CmdResult<Vec<Row>> {
    with_tree(&state, |t| t.arena.largest_files(limit.unwrap_or(100)))
}

/// Total bytes per broad file category. Answers "what kind of thing is eating
/// the space", which no amount of folder-by-folder browsing makes obvious.
#[tauri::command]
fn type_breakdown(state: State<'_, AppState>) -> CmdResult<Vec<TypeGroup>> {
    with_tree(&state, |t| t.arena.type_breakdown())
}

/// Storage attributed to the app that owns it.
///
/// Answers "which app is eating my space" where the file-type view cannot: much
/// of a device is game assets whose extensions no category list recognises.
#[tauri::command]
fn app_breakdown(state: State<'_, AppState>, limit: Option<usize>) -> CmdResult<Vec<AppUsage>> {
    with_tree(&state, |t| t.arena.app_breakdown(limit.unwrap_or(50)))
}

#[tauri::command]
fn search(state: State<'_, AppState>, query: String, limit: Option<usize>) -> CmdResult<Vec<Row>> {
    with_tree(&state, |t| t.arena.search(&query, limit.unwrap_or(200)))
}

// ── Commands: delete ────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub items: u64,
    pub stats: Stats,
    /// The directory the frontend was showing, refreshed. If that directory
    /// was inside what was deleted, this is its nearest surviving ancestor
    /// instead, and later scan progress follows it there.
    pub view: View,
}

/// Delete a node by id.
///
/// `gen` is the generation the id was handed out with; an id from a tree that
/// a rescan has since replaced is refused rather than resolved against the new
/// one, where it would name a different file.
///
/// The id resolves to a byte-exact path here, and the daemon re-validates that
/// path against the session root before acting. The authoritative guard is the
/// daemon's: a check on this side is one that anything speaking to the socket
/// directly never reaches.
#[tauri::command]
async fn delete(app: AppHandle, id: NodeId, gen: u32) -> CmdResult<Deleted> {
    off_main(app, move |_, state| delete_blocking(state, id, gen)).await
}

fn delete_blocking(state: &AppState, id: NodeId, gen: u32) -> CmdResult<Deleted> {
    let _busy = state.claim(Busy::Deleting)?;

    let path = with_tree(state, |t| t.arena.delete_target(id, gen))?.map_err(|e| e.to_string())?;
    let items = state.session()?.delete(&path)?;

    // Only discount it locally once the device confirms it is gone.
    with_tree(state, |t| {
        t.arena.check_gen(gen)?;
        t.arena.remove(id)?;
        t.watching = t.arena.nearest_live(t.watching);
        Ok(Deleted {
            items,
            stats: t.arena.stats(),
            view: t.arena.view(t.watching, DEFAULT_VIEW_LIMIT)?,
        })
    })?
    .map_err(|e: arena::ArenaError| e.to_string())
}

// ── Entry point ─────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_devices,
            connect,
            disconnect,
            scan,
            get_view,
            get_breadcrumbs,
            get_treemap,
            largest_files,
            type_breakdown,
            app_breakdown,
            search,
            delete,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the application")
        .run(|app, event| {
            // Tear the session down, or the forward stays open and the daemon
            // keeps running on the phone until it is unplugged.
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().end_session();
            }
        });
}

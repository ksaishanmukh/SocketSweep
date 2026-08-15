//! SocketSweep desktop host.
//!
//! Orchestrates the on-device daemon over ADB, owns the scanned tree, and
//! exposes it to the frontend as small typed queries.
//!
//! # Why the frontend does not get the tree
//!
//! It used to. The daemon serialised the whole thing to JSON, Rust passed it
//! through as a `String`, and React held ~56,000 nodes in component state —
//! then deep-cloned all of them with `JSON.parse(JSON.stringify(...))` on every
//! delete. Here the tree stays in [`arena`] and React asks for the few hundred
//! rows it is about to draw, referring to nodes by id.
//!
//! Nodes are addressed by id rather than path for a second reason: Android
//! filenames are arbitrary bytes, and a path that round-tripped through a
//! JavaScript string could come back subtly different — as a delete target.

pub mod adb;
pub mod arena;
pub mod session;

use std::sync::Mutex;

use serde::Serialize;
use socketsweep_protocol::Frame;
use tauri::{AppHandle, Emitter, Manager, State};

use adb::{Adb, Device};
use arena::{Arena, Crumb, NodeId, Row, Stats, TreemapNode, View};
use session::Session;

/// Rows per view response. Enough to fill any screen; the frontend virtualises
/// and asks for more if it needs them.
const DEFAULT_VIEW_LIMIT: usize = 500;

/// How often the host pushes an updated view during a scan. 10Hz is fast enough
/// that numbers look live and slow enough that React is not re-rendering on
/// every one of several thousand frames.
const VIEW_PUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Default)]
pub struct AppState {
    adb: Mutex<Option<Adb>>,
    session: Mutex<Option<Session>>,
    tree: Mutex<Option<Arena>>,
    /// The directory the frontend is currently showing. Only this view is
    /// pushed during a scan, so the payload does not grow with the tree.
    watching: Mutex<NodeId>,
}

type CmdResult<T> = Result<T, String>;

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

// ── Commands: connection ────────────────────────────────────────────────────

#[tauri::command]
fn list_devices(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Vec<Device>> {
    let mut guard = state.adb.lock().unwrap();
    if guard.is_none() {
        *guard = Some(Adb::connect(&host_binary(&app, "adb")?).map_err(|e| e.to_string())?);
    }
    guard.as_mut().unwrap().devices().map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub serial: String,
    pub model: String,
    pub root: String,
}

#[tauri::command]
fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: Option<String>,
    root: Option<String>,
) -> CmdResult<Connected> {
    let root = root.unwrap_or_else(|| "/sdcard".into());

    let mut adb_guard = state.adb.lock().unwrap();
    if adb_guard.is_none() {
        *adb_guard = Some(Adb::connect(&host_binary(&app, "adb")?).map_err(|e| e.to_string())?);
    }
    let adb = adb_guard.as_mut().unwrap();

    let serial = adb.resolve(serial.as_deref()).map_err(|e| e.to_string())?;
    let model = adb
        .devices()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.serial == serial)
        .map(|d| d.model)
        .unwrap_or_else(|| serial.clone());

    // Replace any previous session before starting a new one.
    if let Some(old) = state.session.lock().unwrap().take() {
        old.stop(adb);
    }

    let daemon = device_payload(&app, "daemon")?;
    let session =
        Session::start(adb, &serial, &daemon, root.as_bytes()).map_err(|e| e.to_string())?;

    *state.session.lock().unwrap() = Some(session);
    *state.tree.lock().unwrap() = None;

    Ok(Connected {
        serial,
        model,
        root,
    })
}

#[tauri::command]
fn disconnect(state: State<'_, AppState>) -> CmdResult<()> {
    let session = state.session.lock().unwrap().take();
    let mut adb_guard = state.adb.lock().unwrap();

    if let (Some(session), Some(adb)) = (session, adb_guard.as_mut()) {
        session.stop(adb);
    }
    *state.tree.lock().unwrap() = None;
    *state.watching.lock().unwrap() = arena::ROOT;
    Ok(())
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
/// Emits `scan-progress` about ten times a second while running and
/// `scan-complete` at the end. Returns once the walk finishes.
#[tauri::command]
fn scan(app: AppHandle, state: State<'_, AppState>, root: Option<String>) -> CmdResult<Stats> {
    let session_guard = state.session.lock().unwrap();
    let session = session_guard
        .as_ref()
        .ok_or("not connected to a device yet")?;

    let root_bytes = root
        .map(|r| r.into_bytes())
        .unwrap_or_else(|| session.root.clone());

    *state.tree.lock().unwrap() = Some(Arena::new(&root_bytes));
    *state.watching.lock().unwrap() = arena::ROOT;

    let mut last_push = std::time::Instant::now();
    let mut scan_error: Option<String> = None;

    let result = session.scan(&root_bytes, |frame| {
        match frame {
            Frame::Dir { path, entries } => {
                {
                    let mut guard = state.tree.lock().unwrap();
                    if let Some(tree) = guard.as_mut() {
                        if let Err(e) = tree.apply_dir(&path, &entries) {
                            // A frame we cannot place means the tree is already
                            // inconsistent; surface it rather than drawing a lie.
                            scan_error.get_or_insert_with(|| e.to_string());
                        }
                    }
                }

                if last_push.elapsed() >= VIEW_PUSH_INTERVAL {
                    last_push = std::time::Instant::now();
                    emit_progress(&app, &state);
                }
            }
            Frame::Error { message } => {
                scan_error.get_or_insert(message);
            }
            _ => {}
        }
    });

    result?;
    if let Some(e) = scan_error {
        return Err(e);
    }

    let stats = current_stats(&state)?;
    emit_progress(&app, &state);
    let _ = app.emit("scan-complete", stats);
    Ok(stats)
}

fn emit_progress(app: &AppHandle, state: &State<'_, AppState>) {
    let watching = *state.watching.lock().unwrap();

    let payload = {
        let guard = state.tree.lock().unwrap();
        let Some(tree) = guard.as_ref() else { return };
        ScanProgress {
            stats: tree.stats(),
            view: tree.view(watching, DEFAULT_VIEW_LIMIT).ok(),
        }
    };

    let _ = app.emit("scan-progress", payload);
}

fn current_stats(state: &State<'_, AppState>) -> CmdResult<Stats> {
    state
        .tree
        .lock()
        .unwrap()
        .as_ref()
        .map(|t| t.stats())
        .ok_or_else(|| "no scan has been run yet".to_string())
}

// ── Commands: queries ───────────────────────────────────────────────────────

fn with_tree<T>(state: &State<'_, AppState>, f: impl FnOnce(&Arena) -> T) -> CmdResult<T> {
    let guard = state.tree.lock().unwrap();
    let tree = guard.as_ref().ok_or("no scan has been run yet")?;
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
    *state.watching.lock().unwrap() = id;
    with_tree(&state, |t| t.view(id, limit.unwrap_or(DEFAULT_VIEW_LIMIT)))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_breadcrumbs(state: State<'_, AppState>, id: NodeId) -> CmdResult<Vec<Crumb>> {
    with_tree(&state, |t| t.breadcrumbs(id))?.map_err(|e| e.to_string())
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
    with_tree(&state, |t| t.treemap(id, depth.unwrap_or(2)))?.map_err(|e| e.to_string())
}

#[tauri::command]
fn get_stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    with_tree(&state, |t| t.stats())
}

/// The largest files anywhere in the tree — the question the app exists to
/// answer, which previously required expanding folders one at a time.
#[tauri::command]
fn largest_files(state: State<'_, AppState>, limit: Option<usize>) -> CmdResult<Vec<Row>> {
    with_tree(&state, |t| t.largest_files(limit.unwrap_or(100)))
}

#[tauri::command]
fn search(state: State<'_, AppState>, query: String, limit: Option<usize>) -> CmdResult<Vec<Row>> {
    with_tree(&state, |t| t.search(&query, limit.unwrap_or(200)))
}

// ── Commands: delete ────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub items: u64,
    pub stats: Stats,
    pub view: Option<View>,
}

/// Delete a node by id.
///
/// The id is resolved to a byte-exact path here; the daemon then re-validates
/// that path against the session root and is free to refuse. The host does not
/// police it, because a guard on this side is one an attacker talking to the
/// socket directly would never encounter — which is exactly how the previous
/// version was wrong.
#[tauri::command]
fn delete(app: AppHandle, state: State<'_, AppState>, id: NodeId) -> CmdResult<Deleted> {
    if id == arena::ROOT {
        return Err("the scan root cannot be deleted".into());
    }

    let path = with_tree(&state, |t| t.path_of(id))?.map_err(|e| e.to_string())?;

    let items = {
        let guard = state.session.lock().unwrap();
        let session = guard.as_ref().ok_or("not connected to a device yet")?;
        session.delete(&path)?
    };

    // Only discount it locally once the device confirms it is gone.
    let (stats, view) = {
        let mut guard = state.tree.lock().unwrap();
        let tree = guard.as_mut().ok_or("no scan has been run yet")?;
        tree.remove(id).map_err(|e| e.to_string())?;

        let watching = *state.watching.lock().unwrap();
        (tree.stats(), tree.view(watching, DEFAULT_VIEW_LIMIT).ok())
    };

    let _ = app.emit("tree-changed", stats);
    Ok(Deleted { items, stats, view })
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
            get_stats,
            largest_files,
            search,
            delete,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the application")
        .run(|app, event| {
            // Without this, closing the window leaves the forward open and the
            // daemon running on the phone until it is unplugged.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                let session = state.session.lock().unwrap().take();
                let mut adb = state.adb.lock().unwrap();
                if let (Some(session), Some(adb)) = (session, adb.as_mut()) {
                    session.stop(adb);
                }
            }
        });
}

//! [`App`]: the host's brain, implementing the GUI's [`Commands`].
//!
//! # Threads and locks
//!
//! Every command may block on the phone; the shell calls them off the window
//! thread. A scan runs on an ingest thread of its own, which drains the scan's
//! reply channel into the tree and emits `scan-progress` every
//! [`AppConfig::progress_interval`]. Focus changes are coalesced by a
//! [`FocusDriver`] per scan.
//!
//! **No lock is held across device I/O.** Shared state is reached through
//! `Arc`s cloned out of their slot, and the long operations that must not
//! overlap (connecting, starting a scan, deleting, copying) claim the single
//! `Busy` slot instead of holding a lock. A running scan does not hold it:
//! queries, focus, delete and copy all work while it runs.
//!
//! Lock order where several are taken: `busy`, `conn`, `scan`, then a scan's
//! `tree`, then its `meta`.
//!
//! # Sessions
//!
//! One phone at a time. Every scan gets a new GUI [`SessionId`], so a node
//! from an earlier scan is refused as stale. A connection carries an epoch,
//! bumped by every disconnect, so a connect that was under way when the user
//! disconnected does not install itself afterwards, and a lost-connection
//! report for an old connection is ignored.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use socketsweep_gui_contract::{
    self as gui, AppUsage, CommandError, Commands, Connected, ConnectionLost, CopyProgress,
    CopyResult, Crumb, Deleted, Device, DeviceState, ErrorCode, Event, Measure, NodeId, NodeRef,
    Row, ScanFinished, ScanOutcome, ScanPhase, ScanProgress, ScanStarted, SessionId, SizeMetric,
    SkeletonReady, TreemapNode, TypeGroup, View, DEFAULT_APPS_LIMIT, DEFAULT_LARGEST_LIMIT,
    DEFAULT_SEARCH_LIMIT, DEFAULT_TREEMAP_DEPTH, DEFAULT_VIEW_LIMIT, SCAN_PROGRESS_INTERVAL_MS,
};
use socketsweep_transport::{self as transport, Transport, TransportError};
use socketsweep_wire::{ErrCode, Outcome, ReqId, ScanEvent, ScanOpts, ServerMsg};

use crate::client::{Client, ClientError};
use crate::copy::{self, CopyError, CopyTick};
use crate::daemon::{
    check_root, tear_down, Daemon, DaemonBinaries, LaunchError, LaunchOptions, DEFAULT_ROOT,
};
use crate::focus::{FocusDriver, FocusTarget, FOCUS_WINDOW};
use crate::history::History;
use crate::lock;
use crate::tree::{Ingested, Tree, TreeError, ROOT};

/// An event for the GUI. The shell emits the payload under [`HostEvent::name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    ScanProgress(ScanProgress),
    CopyProgress(CopyProgress),
    ConnectionLost(ConnectionLost),
}

impl HostEvent {
    /// The Tauri event name, from the contract.
    pub fn name(&self) -> &'static str {
        match self {
            HostEvent::ScanProgress(_) => ScanProgress::NAME,
            HostEvent::CopyProgress(_) => CopyProgress::NAME,
            HostEvent::ConnectionLost(_) => ConnectionLost::NAME,
        }
    }
}

/// Where events go: the shell forwards each to the window.
pub type EventSink = Arc<dyn Fn(HostEvent) + Send + Sync>;

/// Everything the host needs from the shell.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// The daemon builds bundled with the app.
    pub daemon: DaemonBinaries,
    pub launch: LaunchOptions,
    /// Where scan history is kept; `None` keeps none.
    pub history_file: Option<PathBuf>,
    /// Where `copyToDesktop` copies when the GUI names no destination: the
    /// desktop.
    pub default_destination: Option<PathBuf>,
    pub scan_opts: ScanOpts,
    pub focus_window: Duration,
    pub progress_interval: Duration,
    /// How long `disconnect` and a rescan wait for a cancelled scan to end.
    pub scan_release_timeout: Duration,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            daemon: DaemonBinaries::default(),
            launch: LaunchOptions::default(),
            history_file: None,
            default_destination: None,
            scan_opts: ScanOpts::default(),
            focus_window: FOCUS_WINDOW,
            progress_interval: Duration::from_millis(u64::from(SCAN_PROGRESS_INTERVAL_MS)),
            scan_release_timeout: Duration::from_secs(3),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

fn err(code: ErrorCode, message: impl Into<String>) -> CommandError {
    CommandError::new(code, message)
}

fn tree_err(e: TreeError) -> CommandError {
    let code = match e {
        TreeError::Stale => ErrorCode::Stale,
        TreeError::Removed => ErrorCode::Removed,
        TreeError::UnknownNode(_) | TreeError::RootNotDeletable => ErrorCode::Invalid,
        TreeError::Protocol(_) => ErrorCode::Daemon,
    };
    err(code, e.to_string())
}

fn transport_err(e: TransportError) -> CommandError {
    err(ErrorCode::Device, e.to_string())
}

fn client_err(e: ClientError) -> CommandError {
    let code = match &e {
        ClientError::Failed {
            code: ErrCode::Cancelled,
            ..
        } => ErrorCode::Cancelled,
        ClientError::Failed {
            code: ErrCode::OutsideRoot | ErrCode::ProtectedRoot | ErrCode::BadRequest,
            ..
        } => ErrorCode::Invalid,
        ClientError::Closed(_) => ErrorCode::Device,
        _ => ErrorCode::Daemon,
    };
    err(code, e.to_string())
}

fn launch_err(e: LaunchError) -> CommandError {
    let code = match &e {
        LaunchError::BadRoot(_) => ErrorCode::Invalid,
        LaunchError::Unsupported(_) | LaunchError::Missing(_) | LaunchError::Transport(_) => {
            ErrorCode::Device
        }
        LaunchError::Handshake(_) | LaunchError::NeverAnswered { .. } => ErrorCode::Daemon,
    };
    err(code, e.to_string())
}

fn copy_err(e: CopyError) -> CommandError {
    let code = match &e {
        CopyError::Cancelled => ErrorCode::Cancelled,
        CopyError::Local { .. } => ErrorCode::Io,
        CopyError::Transport(_) => ErrorCode::Device,
    };
    err(code, e.to_string())
}

/// Source of GUI session ids: unique for the life of the process, never 0.
static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);

// ── State ───────────────────────────────────────────────────────────────────

/// Lets `cancel_copy` reach a running copy.
#[derive(Default)]
struct CopyCtl {
    cancel: AtomicBool,
    /// The manifest request while it is being listed, to cancel on the phone.
    prepare: Mutex<Option<(Client, ReqId)>>,
}

/// The long operation in progress. Only one runs at a time.
enum Busy {
    Connecting,
    StartingScan,
    Deleting,
    Copying(Arc<CopyCtl>),
}

impl Busy {
    fn describe(&self) -> &'static str {
        match self {
            Busy::Connecting => "Still connecting to the phone.",
            Busy::StartingScan => "A scan is starting.",
            Busy::Deleting => "A delete is still in progress.",
            Busy::Copying(_) => "A copy is still in progress.",
        }
    }
}

struct BusyGuard<'a>(&'a Inner);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *lock(&self.0.busy) = None;
    }
}

struct Conn {
    daemon: Daemon,
    model: String,
    epoch: u64,
}

struct ScanMeta {
    phase: ScanPhase,
    started: Instant,
    listed: u32,
    known: u32,
    skeleton: Option<SkeletonReady>,
    /// The folder last passed to `focus`.
    focus: Option<NodeId>,
    /// The daemon's latest `FocusState`: its seq, folder and whether settled.
    daemon_focus: Option<(u32, Option<NodeId>, bool)>,
    finished: Option<ScanFinished>,
    /// How the GUI last asked for a view, used for the one in progress events.
    view_limit: u32,
    view_by: SizeMetric,
}

struct Scan {
    session: SessionId,
    req: ReqId,
    client: Client,
    serial: String,
    root: String,
    tree: Mutex<Tree>,
    meta: Mutex<ScanMeta>,
    focus: FocusDriver,
    /// Set when the ingest thread has ended.
    done: Mutex<bool>,
    done_cv: Condvar,
}

impl Scan {
    fn running(&self) -> bool {
        lock(&self.meta).finished.is_none()
    }

    fn cancel(&self) {
        if self.running() {
            let _ = self.client.cancel(self.req);
        }
    }

    fn wait_done(&self, timeout: Duration) {
        let done = lock(&self.done);
        let _ = self
            .done_cv
            .wait_timeout_while(done, timeout, |d| !*d)
            .unwrap_or_else(|e| e.into_inner());
    }

    fn mark_done(&self) {
        *lock(&self.done) = true;
        self.done_cv.notify_all();
    }

    fn elapsed_ms(meta: &ScanMeta) -> u32 {
        u32::try_from(meta.started.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    fn fail(&self, message: String) {
        let mut meta = lock(&self.meta);
        if meta.finished.is_none() {
            meta.finished = Some(ScanFinished {
                outcome: ScanOutcome::Failed,
                elapsed_ms: Self::elapsed_ms(&meta),
                message: Some(message),
            });
            meta.phase = ScanPhase::Finished;
        }
    }

    /// Fold a batch into the tree. `Ok(true)` once the scan has finished.
    fn apply(&self, events: &[ScanEvent]) -> Result<bool, TreeError> {
        let mut tree = lock(&self.tree);
        let mut meta = lock(&self.meta);
        for event in events {
            match tree.ingest(event)? {
                Ingested::Tree => {}
                Ingested::SkeletonReady {
                    listed,
                    frontier,
                    elapsed_ms,
                } => {
                    meta.skeleton = Some(SkeletonReady {
                        listed,
                        frontier,
                        elapsed_ms,
                    });
                    if meta.phase == ScanPhase::Skeleton {
                        meta.phase = ScanPhase::Measuring;
                    }
                }
                Ingested::FocusState { seq, node, settled } => {
                    if meta.daemon_focus.is_none_or(|(s, ..)| seq >= s) {
                        meta.daemon_focus = Some((seq, node, settled));
                    }
                }
                Ingested::Progress { listed, known } => {
                    meta.listed = listed;
                    meta.known = known;
                }
                Ingested::Finished {
                    outcome,
                    elapsed_ms,
                    ..
                } => {
                    let (outcome, message) = match outcome {
                        Outcome::Complete => (ScanOutcome::Complete, None),
                        Outcome::Cancelled => (ScanOutcome::Cancelled, None),
                        Outcome::Aborted { code } => (
                            ScanOutcome::Failed,
                            Some(format!("The phone stopped the scan ({code:?}).")),
                        ),
                    };
                    meta.finished = Some(ScanFinished {
                        outcome,
                        elapsed_ms,
                        message,
                    });
                    meta.phase = ScanPhase::Finished;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn progress(&self) -> ScanProgress {
        let tree = lock(&self.tree);
        let meta = lock(&self.meta);
        let watching = tree.nearest_live(meta.focus.unwrap_or(ROOT));
        let focus = meta.focus.map(|f| {
            let node = tree.nearest_live(f);
            let daemon_settled = matches!(meta.daemon_focus, Some((_, Some(n), true)) if n == node);
            gui::FocusState {
                node: tree.node_ref(node),
                settled: daemon_settled || tree.measure(node) == Some(Measure::Final),
            }
        });
        ScanProgress {
            session: self.session,
            phase: meta.phase,
            totals: tree.totals(),
            listed: meta.listed,
            known: meta.known,
            elapsed_ms: meta
                .finished
                .as_ref()
                .map_or_else(|| Self::elapsed_ms(&meta), |f| f.elapsed_ms),
            skeleton: meta.skeleton,
            focus,
            finished: meta.finished.clone(),
            view: tree.view(watching, meta.view_limit, meta.view_by).ok(),
        }
    }
}

struct Inner {
    transport: Arc<dyn Transport>,
    config: AppConfig,
    sink: EventSink,
    history: Option<History>,
    busy: Mutex<Option<Busy>>,
    conn: Mutex<Option<Arc<Conn>>>,
    scan: Mutex<Option<Arc<Scan>>>,
    epoch: AtomicU64,
}

impl Inner {
    fn emit(&self, event: HostEvent) {
        (self.sink)(event);
    }

    fn claim(&self, next: Busy) -> CmdResult<BusyGuard<'_>> {
        let mut busy = lock(&self.busy);
        if let Some(current) = busy.as_ref() {
            return Err(err(ErrorCode::Busy, current.describe()));
        }
        *busy = Some(next);
        Ok(BusyGuard(self))
    }

    fn conn(&self) -> CmdResult<Arc<Conn>> {
        lock(&self.conn)
            .clone()
            .ok_or_else(|| err(ErrorCode::NotConnected, "No phone is connected."))
    }

    fn scan(&self) -> CmdResult<Arc<Scan>> {
        lock(&self.scan)
            .clone()
            .ok_or_else(|| err(ErrorCode::NoScan, "No scan has been run yet."))
    }

    /// Cancel any scan, forget the tree, stop the daemon.
    fn end_session(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(Busy::Copying(ctl)) = lock(&self.busy).as_ref() {
            ctl.cancel.store(true, Ordering::SeqCst);
        }
        let scan = lock(&self.scan).take();
        if let Some(scan) = &scan {
            scan.focus.stop();
            scan.cancel();
        }
        let conn = lock(&self.conn).take();
        if let Some(conn) = conn {
            conn.daemon.stop(self.transport.as_ref());
        }
        if let Some(scan) = scan {
            scan.wait_done(self.config.scan_release_timeout);
        }
    }

    /// The connection of `epoch` ended without a disconnect.
    fn connection_lost(&self, epoch: u64, why: String) {
        let conn = {
            let mut slot = lock(&self.conn);
            match slot.as_ref() {
                Some(c) if c.epoch == epoch => slot.take(),
                _ => return,
            }
        };
        let Some(conn) = conn else { return };
        self.epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(scan) = lock(&self.scan).take() {
            scan.focus.stop();
        }
        self.emit(HostEvent::ConnectionLost(ConnectionLost {
            serial: conn.daemon.serial.clone(),
            message: why,
        }));
        // Best effort, and the phone may be gone: not on the reader thread.
        let transport = Arc::clone(&self.transport);
        let _ = std::thread::Builder::new()
            .name("socketsweep-teardown".into())
            .spawn(move || tear_down(transport.as_ref(), &conn.daemon.serial));
    }
}

/// The host. See the [module documentation](self).
pub struct App {
    inner: Arc<Inner>,
}

impl App {
    /// A host using `transport`, sending events to `sink`.
    pub fn new(
        transport: Arc<dyn Transport>,
        config: AppConfig,
        sink: impl Fn(HostEvent) + Send + Sync + 'static,
    ) -> Self {
        let history = config.history_file.clone().map(History::new);
        App {
            inner: Arc::new(Inner {
                transport,
                config,
                sink: Arc::new(sink),
                history,
                busy: Mutex::new(None),
                conn: Mutex::new(None),
                scan: Mutex::new(None),
                epoch: AtomicU64::new(0),
            }),
        }
    }

    /// End the session, as on app exit. Also done when the `App` is dropped.
    pub fn shutdown(&self) {
        self.inner.end_session();
    }

    fn resolve_serial(&self, serial: Option<String>) -> CmdResult<transport::Device> {
        let devices = self.inner.transport.devices().map_err(transport_err)?;
        let device = match serial {
            Some(serial) => devices
                .into_iter()
                .find(|d| d.serial == serial)
                .ok_or_else(|| transport_err(TransportError::NotFound { serial }))?,
            None => {
                let mut usable: Vec<_> = devices.iter().filter(|d| d.usable()).cloned().collect();
                match usable.len() {
                    1 => usable.remove(0),
                    // None usable: report why the first one is not.
                    0 => devices
                        .into_iter()
                        .next()
                        .ok_or_else(|| transport_err(TransportError::NoDevices))?,
                    _ => {
                        return Err(err(
                            ErrorCode::Invalid,
                            "Several phones are connected. Choose one.",
                        ))
                    }
                }
            }
        };
        if !device.usable() {
            return Err(transport_err(TransportError::Unusable {
                serial: device.serial,
                state: device.state,
                hint: device.hint,
            }));
        }
        Ok(device)
    }

    fn spawn_ingest(&self, scan: Arc<Scan>, rx: Receiver<ServerMsg>) {
        let inner = Arc::clone(&self.inner);
        let s = Arc::clone(&scan);
        let spawned = std::thread::Builder::new()
            .name("socketsweep-ingest".into())
            .spawn(move || ingest_loop(&inner, &s, rx));
        if let Err(e) = spawned {
            scan.fail(format!("cannot start the scan thread: {e}"));
            scan.cancel();
            scan.mark_done();
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.inner.end_session();
    }
}

fn ingest_loop(inner: &Inner, scan: &Scan, rx: Receiver<ServerMsg>) {
    let interval = inner.config.progress_interval;
    let mut next_emit = Instant::now() + interval;
    loop {
        let wait = next_emit.saturating_duration_since(Instant::now());
        let ended = match rx.recv_timeout(wait) {
            Ok(ServerMsg::Scan(events)) => match scan.apply(&events) {
                Ok(ended) => ended,
                Err(e) => {
                    scan.fail(e.to_string());
                    let _ = scan.client.cancel(scan.req);
                    true
                }
            },
            Ok(ServerMsg::Failed { message, .. }) => {
                scan.fail(message);
                true
            }
            Ok(other) => {
                scan.fail(format!("unexpected reply to the scan: {other:?}"));
                let _ = scan.client.cancel(scan.req);
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => {
                scan.fail(
                    scan.client
                        .closed_reason()
                        .unwrap_or_else(|| "the scan stopped without finishing".into()),
                );
                true
            }
        };
        if ended {
            break;
        }
        if Instant::now() >= next_emit {
            inner.emit(HostEvent::ScanProgress(scan.progress()));
            next_emit = Instant::now() + interval;
        }
    }
    scan.focus.stop();
    let progress = scan.progress();
    if progress
        .finished
        .as_ref()
        .is_some_and(|f| f.outcome == ScanOutcome::Complete)
    {
        if let Some(history) = &inner.history {
            history.record(&scan.serial, &scan.root, &progress.totals);
        }
    }
    inner.emit(HostEvent::ScanProgress(progress));
    scan.mark_done();
}

fn device_state(state: &transport::DeviceState) -> DeviceState {
    match state {
        transport::DeviceState::Ready => DeviceState::Ready,
        transport::DeviceState::Unauthorized => DeviceState::Unauthorized,
        transport::DeviceState::Offline => DeviceState::Offline,
        transport::DeviceState::NoPermission => DeviceState::NoPermission,
        transport::DeviceState::Other(_) => DeviceState::Other,
    }
}

impl Commands for App {
    fn list_devices(&self) -> CmdResult<Vec<Device>> {
        let devices = self.inner.transport.devices().map_err(transport_err)?;
        Ok(devices
            .into_iter()
            .map(|d| Device {
                usable: d.usable(),
                hint: d.hint.clone().or_else(|| match &d.state {
                    transport::DeviceState::Unauthorized => Some(
                        "Accept the \u{201c}Allow USB debugging\u{201d} prompt on the phone."
                            .into(),
                    ),
                    transport::DeviceState::Offline => {
                        Some("Unplug and reconnect the phone.".into())
                    }
                    _ => None,
                }),
                state: device_state(&d.state),
                model: d.model,
                serial: d.serial,
            })
            .collect())
    }

    fn connect(&self, serial: Option<String>, root: Option<String>) -> CmdResult<Connected> {
        let inner = &self.inner;
        let _busy = inner.claim(Busy::Connecting)?;
        let root = root.unwrap_or_else(|| DEFAULT_ROOT.into());
        check_root(&root).map_err(launch_err)?;
        let device = self.resolve_serial(serial)?;

        // The old phone's session goes first, so a failed reconnect is not
        // left serving it.
        inner.end_session();
        let epoch = inner.epoch.load(Ordering::SeqCst);

        let daemon = Daemon::start(
            inner.transport.as_ref(),
            &device.serial,
            &root,
            &inner.config.daemon,
            &inner.config.launch,
        )
        .map_err(launch_err)?;

        let weak: Weak<Inner> = Arc::downgrade(inner);
        daemon.client.set_on_close(Box::new(move |why| {
            if let Some(inner) = weak.upgrade() {
                inner.connection_lost(epoch, why);
            }
        }));

        {
            let mut slot = lock(&inner.conn);
            if inner.epoch.load(Ordering::SeqCst) != epoch {
                drop(slot);
                daemon.stop(inner.transport.as_ref());
                return Err(err(ErrorCode::Cancelled, "Disconnected while connecting."));
            }
            let model = if daemon.info.model.is_empty() {
                device.model.clone()
            } else {
                daemon.info.model.clone()
            };
            *slot = Some(Arc::new(Conn {
                daemon,
                model,
                epoch,
            }));
        }
        let conn = inner.conn()?;
        if !conn.daemon.client.is_open() {
            // Lost before the callback was in place: it has already run.
            return Err(err(ErrorCode::Device, "The phone disconnected."));
        }
        let previous = inner
            .history
            .as_ref()
            .and_then(|h| h.previous(&device.serial, &root));
        Ok(Connected {
            serial: device.serial,
            model: conn.model.clone(),
            root,
            previous,
        })
    }

    fn disconnect(&self) -> CmdResult<()> {
        self.inner.end_session();
        Ok(())
    }

    fn start_scan(&self, root: Option<String>) -> CmdResult<ScanStarted> {
        let inner = &self.inner;
        let _busy = inner.claim(Busy::StartingScan)?;
        let conn = inner.conn()?;
        let root = root.unwrap_or_else(|| conn.daemon.root.clone());
        check_root(&root).map_err(launch_err)?;

        // The daemon runs one scan at a time: stop the old one first. Its
        // tree stays queryable until the new scan has started.
        let old = lock(&inner.scan).clone();
        if let Some(old) = old {
            old.focus.stop();
            old.cancel();
            old.wait_done(inner.config.scan_release_timeout);
        }

        let client = conn.daemon.client.clone();
        let (req, rx) = client
            .start_scan(root.as_bytes().to_vec(), inner.config.scan_opts.clone())
            .map_err(client_err)?;
        let first = rx.recv_timeout(inner.config.launch.client.dead_after);
        let events = match first {
            Ok(ServerMsg::Scan(events)) => events,
            Ok(ServerMsg::Failed { code, message, .. }) => {
                let code = match code {
                    ErrCode::NotFound | ErrCode::PermissionDenied | ErrCode::OutsideRoot => {
                        ErrorCode::Invalid
                    }
                    _ => ErrorCode::Daemon,
                };
                return Err(err(code, message));
            }
            Ok(other) => {
                let _ = client.cancel(req);
                return Err(err(
                    ErrorCode::Daemon,
                    format!("Unexpected reply to the scan: {other:?}"),
                ));
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = client.cancel(req);
                return Err(err(ErrorCode::Daemon, "The phone did not start the scan."));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(client_err(ClientError::Closed(
                    client.closed_reason().unwrap_or_default(),
                )))
            }
        };
        let Some(ScanEvent::Started { root: resolved, .. }) = events.first() else {
            let _ = client.cancel(req);
            return Err(err(
                ErrorCode::Daemon,
                "The phone's scan did not begin with Started.",
            ));
        };

        let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let focus_client = client.clone();
        let focus = FocusDriver::start(inner.config.focus_window, move |(target, seq)| {
            let _ = match target {
                FocusTarget::Dir(dir) => focus_client.focus(req, dir, seq),
                FocusTarget::Clear => focus_client.clear_focus(req, seq),
            };
        });
        let scan = Arc::new(Scan {
            session,
            req,
            client,
            serial: conn.daemon.serial.clone(),
            root: root.clone(),
            tree: Mutex::new(Tree::new(session, resolved, &root)),
            meta: Mutex::new(ScanMeta {
                phase: ScanPhase::Skeleton,
                started: Instant::now(),
                listed: 0,
                known: 1,
                skeleton: None,
                focus: None,
                daemon_focus: None,
                finished: None,
                view_limit: DEFAULT_VIEW_LIMIT,
                view_by: SizeMetric::Disk,
            }),
            focus,
            done: Mutex::new(false),
            done_cv: Condvar::new(),
        });
        let rest_failed = match scan.apply(&events[1..]) {
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        };
        if let Some(message) = rest_failed {
            scan.fail(message);
            let _ = scan.client.cancel(req);
        }
        *lock(&inner.scan) = Some(Arc::clone(&scan));
        self.spawn_ingest(Arc::clone(&scan), rx);

        Ok(ScanStarted {
            session,
            root: NodeRef { session, id: ROOT },
            path: root,
        })
    }

    fn cancel_scan(&self) -> CmdResult<()> {
        if let Some(scan) = lock(&self.inner.scan).clone() {
            scan.cancel();
        }
        Ok(())
    }

    fn focus(&self, node: Option<NodeRef>) -> CmdResult<()> {
        let scan = self.inner.scan()?;
        let (folder, dir) = {
            let tree = lock(&scan.tree);
            let id = match node {
                Some(node) => tree.resolve(node).map_err(tree_err)?,
                None => ROOT,
            };
            tree.focus_dir(id).map_err(tree_err)?
        };
        let running = {
            let mut meta = lock(&scan.meta);
            meta.focus = Some(folder);
            meta.finished.is_none()
        };
        if running {
            let target = if folder == ROOT {
                FocusTarget::Clear
            } else {
                FocusTarget::Dir(dir)
            };
            scan.focus.request(target);
        }
        Ok(())
    }

    fn get_view(
        &self,
        node: Option<NodeRef>,
        limit: Option<u32>,
        by: Option<SizeMetric>,
    ) -> CmdResult<View> {
        let scan = self.inner.scan()?;
        let limit = limit.unwrap_or(DEFAULT_VIEW_LIMIT);
        let by = by.unwrap_or_default();
        let view = {
            let tree = lock(&scan.tree);
            let id = match node {
                Some(node) => tree.resolve(node).map_err(tree_err)?,
                None => ROOT,
            };
            tree.view(id, limit, by).map_err(tree_err)?
        };
        let mut meta = lock(&scan.meta);
        meta.view_limit = limit;
        meta.view_by = by;
        Ok(view)
    }

    fn get_breadcrumbs(&self, node: NodeRef) -> CmdResult<Vec<Crumb>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = tree.resolve(node).map_err(tree_err)?;
        tree.breadcrumbs(id).map_err(tree_err)
    }

    fn get_treemap(
        &self,
        node: Option<NodeRef>,
        depth: Option<u32>,
        by: Option<SizeMetric>,
    ) -> CmdResult<TreemapNode> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = match node {
            Some(node) => tree.resolve(node).map_err(tree_err)?,
            None => ROOT,
        };
        tree.treemap(
            id,
            depth.unwrap_or(DEFAULT_TREEMAP_DEPTH),
            by.unwrap_or_default(),
        )
        .map_err(tree_err)
    }

    fn largest_files(&self, limit: Option<u32>, by: Option<SizeMetric>) -> CmdResult<Vec<Row>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.largest_files(
            limit.unwrap_or(DEFAULT_LARGEST_LIMIT),
            by.unwrap_or_default(),
        ))
    }

    fn type_breakdown(&self, by: Option<SizeMetric>) -> CmdResult<Vec<TypeGroup>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.type_breakdown(by.unwrap_or_default()))
    }

    fn app_breakdown(
        &self,
        limit: Option<u32>,
        by: Option<SizeMetric>,
    ) -> CmdResult<Vec<AppUsage>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.app_breakdown(limit.unwrap_or(DEFAULT_APPS_LIMIT), by.unwrap_or_default()))
    }

    fn search(&self, query: String, limit: Option<u32>) -> CmdResult<Vec<Row>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let by = lock(&scan.meta).view_by;
        Ok(tree.search(&query, limit.unwrap_or(DEFAULT_SEARCH_LIMIT), by))
    }

    fn delete(&self, node: NodeRef) -> CmdResult<Deleted> {
        let inner = &self.inner;
        let _busy = inner.claim(Busy::Deleting)?;
        let conn = inner.conn()?;
        let scan = inner.scan()?;
        let (id, path, hint) = {
            let tree = lock(&scan.tree);
            let path = tree.delete_target(node).map_err(tree_err)?;
            let id = node.id;
            let hint = scan
                .running()
                .then(|| tree.dir_of(id).map(|dir| (scan.req, dir)))
                .flatten();
            (id, path, hint)
        };

        let (items, _) = conn
            .daemon
            .client
            .delete(path, hint, &mut |_| {})
            .map_err(client_err)?;

        // Only discount it locally once the phone confirms it is gone. A
        // `Pruned` may have done it already.
        let mut tree = lock(&scan.tree);
        if tree.exists(id) {
            tree.remove(id).map_err(tree_err)?;
        }
        let mut meta = lock(&scan.meta);
        let watching = tree.nearest_live(meta.focus.unwrap_or(ROOT));
        if meta.focus.is_some() {
            meta.focus = Some(watching);
        }
        let view = tree
            .view(watching, meta.view_limit, meta.view_by)
            .map_err(tree_err)?;
        Ok(Deleted {
            items,
            totals: tree.totals(),
            view,
        })
    }

    fn copy_to_desktop(&self, node: NodeRef, destination: Option<String>) -> CmdResult<CopyResult> {
        let inner = &self.inner;
        let ctl = Arc::new(CopyCtl::default());
        let _busy = inner.claim(Busy::Copying(Arc::clone(&ctl)))?;
        let conn = inner.conn()?;
        let scan = inner.scan()?;
        let path = {
            let tree = lock(&scan.tree);
            let id = tree.resolve(node).map_err(tree_err)?;
            tree.path_of(id).map_err(tree_err)?
        };
        let dest = destination
            .map(PathBuf::from)
            .or_else(|| inner.config.default_destination.clone())
            .ok_or_else(|| err(ErrorCode::Invalid, "Choose where to copy to."))?;
        std::fs::create_dir_all(&dest)
            .map_err(|e| err(ErrorCode::Io, format!("Cannot use {}: {e}", dest.display())))?;
        let destination = dest.display().to_string();
        let progress =
            |tick: &CopyTick, files_total: u32, bytes_total: u64, known: bool| CopyProgress {
                node,
                destination: destination.clone(),
                files_done: tick.files_done,
                files_total,
                bytes_done: tick.bytes_done,
                bytes_total,
                totals_known: known,
                current: tick.current.clone(),
            };

        // 1. The manifest.
        let client = conn.daemon.client.clone();
        let (req, rx) = client.prepare_copy(path).map_err(client_err)?;
        *lock(&ctl.prepare) = Some((client.clone(), req));
        if ctl.cancel.load(Ordering::SeqCst) {
            let _ = client.cancel(req);
        }
        let mut items = Vec::new();
        let root = loop {
            match rx.recv() {
                Ok(ServerMsg::CopyManifest {
                    root: r,
                    files,
                    last,
                }) => {
                    items.extend(files);
                    let files_total = items
                        .iter()
                        .filter(|i| i.kind == socketsweep_wire::Kind::File)
                        .count() as u32;
                    let bytes_total = items.iter().map(|i| i.size).sum();
                    inner.emit(HostEvent::CopyProgress(progress(
                        &CopyTick::default(),
                        files_total,
                        bytes_total,
                        false,
                    )));
                    if last {
                        break r;
                    }
                }
                Ok(ServerMsg::Failed {
                    code,
                    errno,
                    message,
                }) => {
                    *lock(&ctl.prepare) = None;
                    return Err(client_err(ClientError::Failed {
                        code,
                        errno,
                        message,
                    }));
                }
                Ok(other) => {
                    let _ = client.cancel(req);
                    *lock(&ctl.prepare) = None;
                    return Err(err(
                        ErrorCode::Daemon,
                        format!("Unexpected reply to the copy: {other:?}"),
                    ));
                }
                Err(_) => {
                    *lock(&ctl.prepare) = None;
                    return Err(client_err(ClientError::Closed(
                        client.closed_reason().unwrap_or_default(),
                    )));
                }
            }
        };
        *lock(&ctl.prepare) = None;
        if ctl.cancel.load(Ordering::SeqCst) {
            return Err(copy_err(CopyError::Cancelled));
        }

        // 2. The files.
        let plan = copy::plan(&root, &items, &dest, &|p| p.exists());
        let files_total = plan.files.len() as u32;
        let bytes_total = plan.bytes;
        let destination = plan.top.display().to_string();
        let progress = |tick: &CopyTick, known: bool| CopyProgress {
            node,
            destination: destination.clone(),
            files_done: tick.files_done,
            files_total,
            bytes_done: tick.bytes_done,
            bytes_total,
            totals_known: known,
            current: tick.current.clone(),
        };
        inner.emit(HostEvent::CopyProgress(progress(
            &CopyTick::default(),
            true,
        )));
        let last_emit = Mutex::new(Instant::now());
        let interval = inner.config.progress_interval;
        let done = copy::execute(
            inner.transport.as_ref(),
            &conn.daemon.serial,
            &plan,
            &ctl.cancel,
            &|tick| {
                let mut last = lock(&last_emit);
                if last.elapsed() >= interval || tick.files_done == files_total {
                    *last = Instant::now();
                    drop(last);
                    inner.emit(HostEvent::CopyProgress(progress(&tick, true)));
                }
            },
        )
        .map_err(copy_err)?;
        inner.emit(HostEvent::CopyProgress(progress(&done, true)));
        Ok(CopyResult {
            destination,
            files: done.files_done,
            bytes: done.bytes_done,
            skipped: plan.skipped,
            renamed: plan.renamed,
        })
    }

    fn cancel_copy(&self) -> CmdResult<()> {
        let ctl = match lock(&self.inner.busy).as_ref() {
            Some(Busy::Copying(ctl)) => Arc::clone(ctl),
            _ => return Ok(()),
        };
        ctl.cancel.store(true, Ordering::SeqCst);
        if let Some((client, req)) = lock(&ctl.prepare).clone() {
            let _ = client.cancel(req);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

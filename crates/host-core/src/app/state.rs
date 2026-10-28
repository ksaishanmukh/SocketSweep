//! The host's shared state: the busy slot, the connection, the scan, and the
//! [`Inner`] that holds them.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use socketsweep_gui_contract::{
    ConnectionLost, ErrorCode, NodeId, ScanFinished, ScanOutcome, ScanPhase, SessionId, SizeMetric,
    SkeletonReady,
};
use socketsweep_transport::{Transport, TransportError};
use socketsweep_wire::ReqId;

use super::errors::{err, transport_err, CmdResult};
use super::{AppConfig, EventSink, HostEvent};
use crate::client::Client;
use crate::daemon::{tear_down, Daemon};
use crate::focus::FocusDriver;
use crate::history::History;
use crate::lock;
use crate::tree::Tree;

/// Lets `cancel_copy` reach a running copy.
#[derive(Default)]
pub(super) struct CopyCtl {
    pub(super) cancel: AtomicBool,
    /// The manifest request while it is being listed, to cancel on the phone.
    pub(super) prepare: Mutex<Option<(Client, ReqId)>>,
}

/// The long operation in progress. Only one runs at a time.
pub(super) enum Busy {
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

pub(super) struct BusyGuard<'a>(&'a Inner);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *lock(&self.0.busy) = None;
    }
}

pub(super) struct Conn {
    pub(super) daemon: Daemon,
    pub(super) model: String,
    pub(super) epoch: u64,
}

pub(super) struct ScanMeta {
    pub(super) phase: ScanPhase,
    pub(super) started: Instant,
    pub(super) listed: u32,
    pub(super) known: u32,
    pub(super) skeleton: Option<SkeletonReady>,
    /// The folder last passed to `focus`.
    pub(super) focus: Option<NodeId>,
    /// The daemon's latest `FocusState`: its seq, folder and whether settled.
    pub(super) daemon_focus: Option<(u32, Option<NodeId>, bool)>,
    pub(super) finished: Option<ScanFinished>,
    /// How the GUI last asked, with `focus`, for the open folder to be
    /// shown: the size and row cap of the `view` in progress events and in
    /// the answer to `delete`. `get_view` never changes them.
    pub(super) view_limit: u32,
    pub(super) view_by: SizeMetric,
}

pub(super) struct Scan {
    pub(super) session: SessionId,
    pub(super) req: ReqId,
    pub(super) client: Client,
    pub(super) serial: String,
    pub(super) root: String,
    pub(super) tree: Mutex<Tree>,
    pub(super) meta: Mutex<ScanMeta>,
    pub(super) focus: FocusDriver,
    /// Set when the ingest thread has ended.
    pub(super) done: Mutex<bool>,
    pub(super) done_cv: Condvar,
}

impl Scan {
    pub(super) fn running(&self) -> bool {
        lock(&self.meta).finished.is_none()
    }

    pub(super) fn cancel(&self) {
        if self.running() {
            let _ = self.client.cancel(self.req);
        }
    }

    pub(super) fn wait_done(&self, timeout: Duration) {
        let done = lock(&self.done);
        let _ = self
            .done_cv
            .wait_timeout_while(done, timeout, |d| !*d)
            .unwrap_or_else(|e| e.into_inner());
    }

    pub(super) fn mark_done(&self) {
        *lock(&self.done) = true;
        self.done_cv.notify_all();
    }

    pub(super) fn elapsed_ms(meta: &ScanMeta) -> u32 {
        u32::try_from(meta.started.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    pub(super) fn fail(&self, message: String) {
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
}

/// Builds the transport on first use. See [`App::lazy`].
pub(super) type TransportFactory =
    Box<dyn Fn() -> Result<Arc<dyn Transport>, TransportError> + Send + Sync>;

pub(super) struct Inner {
    /// The transport, once built. Held only to build it (starting adb can take
    /// a moment), never while talking to a phone.
    pub(super) transport: Mutex<Option<Arc<dyn Transport>>>,
    pub(super) factory: Option<TransportFactory>,
    pub(super) config: AppConfig,
    pub(super) sink: EventSink,
    pub(super) history: Option<History>,
    pub(super) busy: Mutex<Option<Busy>>,
    pub(super) conn: Mutex<Option<Arc<Conn>>>,
    pub(super) scan: Mutex<Option<Arc<Scan>>>,
    pub(super) epoch: AtomicU64,
}

impl Inner {
    pub(super) fn emit(&self, event: HostEvent) {
        (self.sink)(event);
    }

    /// The transport, built on the first call. A failure is not remembered:
    /// the next command tries again.
    pub(super) fn transport(&self) -> CmdResult<Arc<dyn Transport>> {
        let mut slot = lock(&self.transport);
        if let Some(t) = slot.as_ref() {
            return Ok(Arc::clone(t));
        }
        let Some(factory) = &self.factory else {
            return Err(err(ErrorCode::Device, "No device transport is available."));
        };
        let t = factory().map_err(transport_err)?;
        *slot = Some(Arc::clone(&t));
        Ok(t)
    }

    /// The transport if it has been built. A connection or a copy exists only
    /// after it was, so cleanup uses this and never starts anything.
    pub(super) fn built_transport(&self) -> Option<Arc<dyn Transport>> {
        lock(&self.transport).clone()
    }

    pub(super) fn claim(&self, next: Busy) -> CmdResult<BusyGuard<'_>> {
        let mut busy = lock(&self.busy);
        if let Some(current) = busy.as_ref() {
            return Err(err(ErrorCode::Busy, current.describe()));
        }
        *busy = Some(next);
        Ok(BusyGuard(self))
    }

    pub(super) fn conn(&self) -> CmdResult<Arc<Conn>> {
        lock(&self.conn)
            .clone()
            .ok_or_else(|| err(ErrorCode::NotConnected, "No phone is connected."))
    }

    pub(super) fn scan(&self) -> CmdResult<Arc<Scan>> {
        lock(&self.scan)
            .clone()
            .ok_or_else(|| err(ErrorCode::NoScan, "No scan has been run yet."))
    }

    /// Cancel any scan, forget the tree, stop the daemon.
    pub(super) fn end_session(&self) {
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
        if let (Some(conn), Some(transport)) = (conn, self.built_transport()) {
            conn.daemon.stop(transport.as_ref());
        }
        if let Some(scan) = scan {
            scan.wait_done(self.config.scan_release_timeout);
        }
    }

    /// The connection of `epoch` ended without a disconnect.
    pub(super) fn connection_lost(&self, epoch: u64, why: String) {
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
        let Some(transport) = self.built_transport() else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("socketsweep-teardown".into())
            .spawn(move || tear_down(transport.as_ref(), &conn.daemon.serial));
    }
}

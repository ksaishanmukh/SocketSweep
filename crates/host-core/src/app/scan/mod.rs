//! Starting a scan: the first reply, the tree, the focus driver, and the
//! ingest thread that takes it from there.

mod ingest;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use socketsweep_gui_contract::{
    ErrorCode, NodeRef, ScanPhase, ScanStarted, SessionId, SizeMetric, DEFAULT_VIEW_LIMIT,
};
use socketsweep_wire::{ErrCode, ReqId, ScanEvent, ServerMsg};

use super::errors::{client_err, err, launch_err, CmdResult};
use super::state::{Busy, Conn, Inner, Scan, ScanMeta};
use crate::client::{Client, ClientError};
use crate::daemon::check_root;
use crate::focus::{FocusDriver, FocusTarget};
use crate::lock;
use crate::tree::{Tree, ROOT};
use ingest::spawn_ingest;

/// Source of GUI session ids: unique for the life of the process, never 0.
pub(super) static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);

pub(super) fn start(inner: &Arc<Inner>, root: Option<String>) -> CmdResult<ScanStarted> {
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
    let events = first_batch(&client, req, &rx, inner.config.launch.client.dead_after)?;
    let Some(ScanEvent::Started { root: resolved, .. }) = events.first() else {
        let _ = client.cancel(req);
        return Err(err(
            ErrorCode::Daemon,
            "The phone's scan did not begin with Started.",
        ));
    };

    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let scan = Arc::new(new_scan(
        inner, &conn, session, req, client, &root, resolved,
    ));
    let rest_failed = match scan.apply(&events[1..]) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
    if let Some(message) = rest_failed {
        scan.fail(message);
        let _ = scan.client.cancel(req);
    }
    *lock(&inner.scan) = Some(Arc::clone(&scan));
    spawn_ingest(inner, Arc::clone(&scan), rx);

    Ok(ScanStarted {
        session,
        root: NodeRef { session, id: ROOT },
        path: root,
    })
}

/// The scan's first batch, or why it did not start.
fn first_batch(
    client: &Client,
    req: ReqId,
    rx: &Receiver<ServerMsg>,
    timeout: Duration,
) -> CmdResult<Vec<ScanEvent>> {
    let first = rx.recv_timeout(timeout);
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
    Ok(events)
}

/// The scan of request `req`, with its tree, metadata and focus driver.
fn new_scan(
    inner: &Inner,
    conn: &Conn,
    session: SessionId,
    req: ReqId,
    client: Client,
    root: &str,
    resolved: &[u8],
) -> Scan {
    let focus_client = client.clone();
    let focus = FocusDriver::start(inner.config.focus_window, move |(target, seq)| {
        let _ = match target {
            FocusTarget::Dir(dir) => focus_client.focus(req, dir, seq),
            FocusTarget::Clear => focus_client.clear_focus(req, seq),
        };
    });
    Scan {
        session,
        req,
        client,
        serial: conn.daemon.serial.clone(),
        root: root.to_string(),
        tree: Mutex::new(Tree::new(session, resolved, root)),
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
    }
}

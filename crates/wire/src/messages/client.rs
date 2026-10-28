//! Host → daemon: requests and hints.

use serde::{Deserialize, Serialize};

use super::ScanOpts;
#[cfg(doc)]
use super::{ErrCode, Outcome, ScanEvent, ServerMsg};
use crate::{DirId, ReqId};

/// One request or hint from the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientFrame {
    /// Chosen by the host, unique among its requests still in progress, and
    /// never 0. Hints (`Focus`, `ClearFocus`, `Cancel`) take an id too, which
    /// nothing answers.
    pub id: ReqId,
    pub msg: ClientMsg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Answered with [`ServerMsg::Pong`] carrying the same nonce. Sent after
    /// [`HOST_PING_INTERVAL`](crate::HOST_PING_INTERVAL) idle.
    Ping { nonce: u32 },
    /// Walk `root`, streaming [`ServerMsg::Scan`] batches until
    /// [`ScanEvent::Finished`]. The id of this frame is the scan's id in
    /// `Focus`, `ClearFocus`, `Cancel` and `Delete.hint`.
    StartScan { root: Vec<u8>, opts: ScanOpts },
    /// The user opened directory `dir` of scan `scan`. A hint, never answered
    /// directly; the scheduler moves workers towards it and reports
    /// [`ScanEvent::FocusState`]. A `seq` lower than one already seen for the
    /// scan is stale and ignored.
    Focus { scan: ReqId, dir: DirId, seq: u32 },
    /// The user is outside every directory of the scan (or at its root):
    /// schedule as a plain parallel walk. Shares `seq` numbering with `Focus`.
    ClearFocus { scan: ReqId, seq: u32 },
    /// Stop request `target`. A hint: a scan still ends with
    /// [`ScanEvent::Finished`] (outcome [`Outcome::Cancelled`]), a delete or
    /// copy with [`ServerMsg::Failed`] ([`ErrCode::Cancelled`]) unless it had
    /// already finished.
    Cancel { target: ReqId },
    /// Recursively delete `path`, answered with [`ServerMsg::Deleted`].
    ///
    /// The path is what is acted on; the daemon resolves it through its guard
    /// (canonicalise the parent, never follow the final component) and
    /// refuses anything outside the allowed roots. `hint` names the same
    /// thing as `(scan, dir)` so a running scan can prune it; the daemon uses
    /// it only if it matches the path the guard resolved.
    Delete {
        path: Vec<u8>,
        hint: Option<(ReqId, DirId)>,
    },
    /// List everything under `path` for copying, answered with
    /// [`ServerMsg::CopyManifest`] parts. Requires [`Caps::COPY_MANIFEST`](crate::Caps::COPY_MANIFEST).
    PrepareCopy { path: Vec<u8> },
    /// Stop the daemon. Not answered; the connection closes.
    Shutdown,
}

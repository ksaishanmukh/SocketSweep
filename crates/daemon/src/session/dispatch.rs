//! Routing one request: answering it at once or starting a job for it.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use socketsweep_engine::Scan;
use socketsweep_wire::{Caps, ClientFrame, ClientMsg, ErrCode, ReqId, ServerMsg, SESSION_LEVEL};

use super::{lock, Active, Shared};
use crate::actions::ActionError;
use crate::outbox::ScanSink;

impl Shared {
    /// Route one frame. Returns true for `Shutdown`.
    pub(super) fn dispatch(self: &Arc<Self>, frame: ClientFrame) -> bool {
        let id = frame.id;
        match frame.msg {
            ClientMsg::Shutdown => return true,
            ClientMsg::Ping { nonce } => self.outbox.reply(id, ServerMsg::Pong { nonce }),
            ClientMsg::Focus { scan, dir, seq } => match self.scan(scan) {
                Some(s) => {
                    s.focus(seq, dir);
                }
                None => self.unknown(id, scan),
            },
            ClientMsg::ClearFocus { scan, seq } => match self.scan(scan) {
                Some(s) => {
                    s.clear_focus(seq);
                }
                None => self.unknown(id, scan),
            },
            ClientMsg::Cancel { target } => self.cancel_request(id, target),
            ClientMsg::StartScan { root, opts } => {
                if self.claim(id) {
                    self.start_scan(id, root, opts);
                }
            }
            ClientMsg::Delete { path, hint } => {
                if self.claim(id) {
                    self.start_delete(id, path, hint);
                }
            }
            ClientMsg::PrepareCopy { path } => self.prepare_copy(id, path),
        }
        false
    }

    pub(super) fn fail(&self, id: ReqId, e: ActionError) {
        self.outbox.reply(id, e.into_msg());
    }

    pub(super) fn scan(&self, id: ReqId) -> Option<Scan<ScanSink>> {
        match lock(&self.active).get(&id) {
            Some(Active::Scan(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// `Cancel { target }`: the scan's handle, or the delete's or copy's flag.
    fn cancel_request(&self, id: ReqId, target: ReqId) {
        let found = match lock(&self.active).get(&target) {
            Some(Active::Scan(s)) => Some(Some(s.clone())),
            Some(Active::Job(flag)) => {
                flag.store(true, Ordering::Relaxed);
                Some(None)
            }
            None => None,
        };
        match found {
            // Outside the map's lock: cancelling queues events.
            Some(Some(scan)) => scan.cancel(),
            Some(None) => {}
            None => self.unknown(id, target),
        }
    }

    /// `PrepareCopy`, if `COPY_MANIFEST` was negotiated.
    fn prepare_copy(self: &Arc<Self>, id: ReqId, path: Vec<u8>) {
        if !self.caps.contains(Caps::COPY_MANIFEST) {
            self.fail(
                id,
                ActionError::new(ErrCode::Unsupported, "COPY_MANIFEST was not negotiated"),
            );
        } else if self.claim(id) {
            self.start_copy(id, path);
        }
    }

    fn unknown(&self, id: ReqId, target: ReqId) {
        self.fail(
            id,
            ActionError::new(
                ErrCode::BadRequest,
                format!("no request {target} is in progress"),
            ),
        );
    }

    /// Check a new request's id; answer `Failed` and return false if it is
    /// unusable.
    fn claim(&self, id: ReqId) -> bool {
        if id == SESSION_LEVEL {
            self.fail(
                id,
                ActionError::new(ErrCode::BadRequest, "request id 0 is reserved"),
            );
            return false;
        }
        if lock(&self.active).contains_key(&id) {
            self.fail(
                id,
                ActionError::new(
                    ErrCode::DuplicateId,
                    format!("request {id} is already in progress"),
                ),
            );
            return false;
        }
        true
    }
}

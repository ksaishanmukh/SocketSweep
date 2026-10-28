//! The state every clone of a [`Client`](super::Client) shares, and frame
//! routing by request id.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::Instant;

use socketsweep_transport::Duplex;
use socketsweep_wire::{
    encode_frame, ClientFrame, ReqId, ScanEvent, ServerFrame, ServerMsg, SESSION_LEVEL,
};

use super::{wire_err, ClientError, OnClose};
use crate::lock;

#[derive(Default)]
pub(super) struct OnCloseSlot {
    pub(super) callback: Option<OnClose>,
    /// The reader has finished: a callback set now is called at once.
    pub(super) fired: Option<String>,
}

pub(super) struct Inner {
    pub(super) writer: Mutex<Box<dyn Duplex>>,
    /// A handle used only to shut the stream down, so closing never waits
    /// behind a blocked write.
    pub(super) control: Mutex<Box<dyn Duplex>>,
    pub(super) routes: Mutex<HashMap<ReqId, Sender<ServerMsg>>>,
    pub(super) next_id: AtomicU32,
    pub(super) next_nonce: AtomicU32,
    pub(super) last_rx: Mutex<Instant>,
    pub(super) last_tx: Mutex<Instant>,
    /// Why the connection ended, once it has.
    pub(super) closed: Mutex<Option<String>>,
    /// [`Client::close`] was called: the end is not news.
    pub(super) deliberate: AtomicBool,
    pub(super) on_close: Mutex<OnCloseSlot>,
    /// Dropped to stop the keepalive thread.
    pub(super) stop_keepalive: Mutex<Option<Sender<()>>>,
}

impl Inner {
    pub(super) fn next_id(&self) -> ReqId {
        loop {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id != SESSION_LEVEL {
                return id;
            }
        }
    }

    pub(super) fn send(&self, frame: &ClientFrame) -> Result<(), ClientError> {
        if let Some(why) = lock(&self.closed).clone() {
            return Err(ClientError::Closed(why));
        }
        let bytes = encode_frame(frame).map_err(wire_err)?;
        {
            let mut w = lock(&self.writer);
            w.write_all(&bytes)
                .and_then(|()| w.flush())
                .map_err(|e| ClientError::Closed(format!("cannot write to the phone: {e}")))?;
        }
        *lock(&self.last_tx) = Instant::now();
        Ok(())
    }

    /// End the connection, recording `why` if it is the first reason.
    pub(super) fn close(&self, why: String) {
        {
            let mut closed = lock(&self.closed);
            if closed.is_some() {
                return;
            }
            *closed = Some(why);
        }
        let _ = lock(&self.control).shutdown();
        lock(&self.stop_keepalive).take();
    }

    pub(super) fn route(&self, frame: ServerFrame) {
        if frame.id == SESSION_LEVEL {
            // Heartbeats only prove the daemon is alive, which receiving them
            // already recorded.
            return;
        }
        let terminal = match &frame.msg {
            ServerMsg::Pong { .. }
            | ServerMsg::Deleted { .. }
            | ServerMsg::Failed { .. }
            | ServerMsg::CopyManifest { last: true, .. } => true,
            ServerMsg::Scan(events) => events
                .iter()
                .any(|e| matches!(e, ScanEvent::Finished { .. })),
            _ => false,
        };
        let mut routes = lock(&self.routes);
        let delivered = match routes.get(&frame.id) {
            Some(tx) => tx.send(frame.msg).is_ok(),
            None => return,
        };
        if terminal || !delivered {
            routes.remove(&frame.id);
        }
    }
}

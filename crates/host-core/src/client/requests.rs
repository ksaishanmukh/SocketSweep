//! Calls on an open connection: requests, hints, and closing.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use socketsweep_wire::{ClientFrame, ClientMsg, DirId, ReqId, ScanOpts, ServerMsg};

use super::{Client, ClientError};
use crate::lock;

impl Client {
    /// Send a request and register for its replies, which arrive on the
    /// returned channel until the one that ends it. The channel disconnects
    /// if the connection ends first.
    pub fn request(&self, msg: ClientMsg) -> Result<(ReqId, Receiver<ServerMsg>), ClientError> {
        let id = self.inner.next_id();
        let (tx, rx) = mpsc::channel();
        lock(&self.inner.routes).insert(id, tx);
        // The reader drops every route once the connection is closed, but
        // only routes present at that moment: re-check after registering.
        if self.closed_reason().is_some() {
            lock(&self.inner.routes).remove(&id);
            return Err(self.closed_error());
        }
        if let Err(e) = self.inner.send(&ClientFrame { id, msg }) {
            lock(&self.inner.routes).remove(&id);
            return Err(e);
        }
        Ok((id, rx))
    }

    /// Send a hint, which nothing answers.
    pub fn hint(&self, msg: ClientMsg) -> Result<ReqId, ClientError> {
        let id = self.inner.next_id();
        self.inner.send(&ClientFrame { id, msg })?;
        Ok(id)
    }

    /// Start a scan. The returned channel carries its [`ServerMsg::Scan`]
    /// batches, or a [`ServerMsg::Failed`] if it could not start.
    pub fn start_scan(
        &self,
        root: Vec<u8>,
        opts: ScanOpts,
    ) -> Result<(ReqId, Receiver<ServerMsg>), ClientError> {
        self.request(ClientMsg::StartScan { root, opts })
    }

    pub fn cancel(&self, target: ReqId) -> Result<(), ClientError> {
        self.hint(ClientMsg::Cancel { target }).map(|_| ())
    }

    pub fn focus(&self, scan: ReqId, dir: DirId, seq: u32) -> Result<(), ClientError> {
        self.hint(ClientMsg::Focus { scan, dir, seq }).map(|_| ())
    }

    pub fn clear_focus(&self, scan: ReqId, seq: u32) -> Result<(), ClientError> {
        self.hint(ClientMsg::ClearFocus { scan, seq }).map(|_| ())
    }

    /// Round-trip a `Ping`, returning how long it took.
    pub fn ping(&self, timeout: Duration) -> Result<Duration, ClientError> {
        let nonce = self.inner.next_nonce.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let (id, rx) = self.request(ClientMsg::Ping { nonce })?;
        let reply = rx.recv_timeout(timeout);
        lock(&self.inner.routes).remove(&id);
        match reply {
            Ok(ServerMsg::Pong { nonce: n }) if n == nonce => Ok(started.elapsed()),
            Ok(other) => Err(ClientError::Protocol(format!(
                "expected Pong, got {other:?}"
            ))),
            Err(RecvTimeoutError::Timeout) => Err(ClientError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(self.closed_error()),
        }
    }

    /// Ask the daemon to exit, then close. Best effort.
    pub fn shutdown(&self) {
        let _ = self.hint(ClientMsg::Shutdown);
        self.close();
    }

    /// Close the connection. Waiting calls fail with
    /// [`ClientError::Closed`]; the `on_close` callback is not called.
    pub fn close(&self) {
        self.inner.deliberate.store(true, Ordering::SeqCst);
        self.inner.close("the connection was closed".into());
    }
}

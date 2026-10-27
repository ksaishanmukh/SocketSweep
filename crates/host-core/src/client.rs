//! The session client: one connection to the daemon, after the handshake.
//!
//! - **One reader thread** decodes [`ServerFrame`]s and routes them by
//!   [`ReqId`] to the channel the request registered. Scan batches go to the
//!   scan's channel, which the caller drains into the tree; one-shot replies
//!   go to the waiting call. A frame for an id nobody waits for (a late reply
//!   to something abandoned, a `Pong` to a keepalive ping) is dropped.
//! - **Writes** go through a mutex, so frames from several threads never
//!   interleave. Nothing else is held while writing.
//! - **A keepalive thread** sends `Ping` after [`HOST_PING_INTERVAL`] with
//!   nothing sent, and closes the connection after [`HOST_DEAD_AFTER`] with
//!   nothing received. Closing wakes the reader, which drops every route, so
//!   every waiting call sees its channel disconnect and reads why from
//!   [`Client::closed_reason`].

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use socketsweep_transport::Duplex;
use socketsweep_wire::{
    encode_frame, read_frame, Caps, ClientFrame, ClientMsg, DirId, ErrCode, Hello, HelloReply,
    ReqId, ScanEvent, ScanOpts, ServerFrame, ServerMsg, SessionId, Token, Totals, Welcome,
    WireError, HOST_DEAD_AFTER, HOST_PING_INTERVAL, PROTOCOL_VERSION, SESSION_LEVEL,
};

use crate::lock;

/// Timing of one connection. [`Default`] follows the wire constants; tests
/// shorten it.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Send a `Ping` after this long with nothing sent.
    pub ping_interval: Duration,
    /// Treat this long with nothing received as a dead daemon.
    pub dead_after: Duration,
    /// How long the handshake reply may take.
    pub handshake_timeout: Duration,
    /// How often the keepalive thread looks at the clock.
    pub tick: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            ping_interval: HOST_PING_INTERVAL,
            dead_after: HOST_DEAD_AFTER,
            handshake_timeout: Duration::from_millis(500),
            tick: Duration::from_millis(250),
        }
    }
}

/// Why a call on the connection failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The daemon refused the token. Not worth retrying.
    Refused,
    /// The daemon is another version. Not worth retrying.
    WrongVersion { daemon: u16 },
    /// The daemon echoed another session id.
    SessionMismatch,
    /// The connection is closed (or never opened): the reason.
    Closed(String),
    /// The request ended with [`ServerMsg::Failed`].
    Failed {
        code: ErrCode,
        errno: Option<i32>,
        message: String,
    },
    /// The daemon answered with something that makes no sense here.
    Protocol(String),
    /// No answer in time.
    Timeout,
}

impl ClientError {
    /// Whether a fresh connection attempt might succeed: the daemon was not
    /// up yet, rather than refusing.
    pub fn retryable(&self) -> bool {
        matches!(self, ClientError::Closed(_) | ClientError::Timeout)
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Refused => write!(f, "The phone's daemon refused this computer."),
            ClientError::WrongVersion { daemon } => write!(
                f,
                "The phone runs daemon protocol {daemon}, but this app speaks {PROTOCOL_VERSION}."
            ),
            ClientError::SessionMismatch => {
                write!(f, "The phone's daemon answered for another session.")
            }
            ClientError::Closed(why) => write!(f, "Lost the connection to the phone: {why}"),
            ClientError::Failed { message, .. } => write!(f, "{message}"),
            ClientError::Protocol(m) => write!(f, "Unexpected reply from the phone: {m}"),
            ClientError::Timeout => write!(f, "The phone did not answer in time."),
        }
    }
}

impl std::error::Error for ClientError {}

fn wire_err(e: WireError) -> ClientError {
    ClientError::Closed(e.to_string())
}

/// Called once, from the reader thread, when the connection ends without
/// [`Client::close`] having been called. See [`Client::set_on_close`].
pub type OnClose = Box<dyn FnOnce(String) + Send>;

#[derive(Default)]
struct OnCloseSlot {
    callback: Option<OnClose>,
    /// The reader has finished: a callback set now is called at once.
    fired: Option<String>,
}

struct Inner {
    writer: Mutex<Box<dyn Duplex>>,
    /// A handle used only to shut the stream down, so closing never waits
    /// behind a blocked write.
    control: Mutex<Box<dyn Duplex>>,
    routes: Mutex<HashMap<ReqId, Sender<ServerMsg>>>,
    next_id: AtomicU32,
    next_nonce: AtomicU32,
    last_rx: Mutex<Instant>,
    last_tx: Mutex<Instant>,
    /// Why the connection ended, once it has.
    closed: Mutex<Option<String>>,
    /// [`Client::close`] was called: the end is not news.
    deliberate: AtomicBool,
    on_close: Mutex<OnCloseSlot>,
    /// Dropped to stop the keepalive thread.
    stop_keepalive: Mutex<Option<Sender<()>>>,
}

impl Inner {
    fn next_id(&self) -> ReqId {
        loop {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id != SESSION_LEVEL {
                return id;
            }
        }
    }

    fn send(&self, frame: &ClientFrame) -> Result<(), ClientError> {
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
    fn close(&self, why: String) {
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

    fn route(&self, frame: ServerFrame) {
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

/// One authenticated connection to the daemon. Cheap to clone; every clone
/// is the same connection.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
    welcome: Arc<Welcome>,
}

impl Client {
    /// Send `Hello` on `stream`, read the reply, and start the reader and
    /// keepalive threads.
    ///
    /// A stream the daemon is not listening on yet ends at once, which is
    /// [`ClientError::Closed`], [`retryable`](ClientError::retryable).
    pub fn connect(
        stream: Box<dyn Duplex>,
        token: Token,
        session: SessionId,
        caps: Caps,
        config: ClientConfig,
    ) -> Result<Client, ClientError> {
        let closed = |e: std::io::Error| ClientError::Closed(e.to_string());
        stream
            .set_read_timeout(Some(config.handshake_timeout))
            .map_err(closed)?;
        stream
            .set_write_timeout(Some(config.dead_after))
            .map_err(closed)?;
        let mut writer = stream.try_clone().map_err(closed)?;
        let control = stream.try_clone().map_err(closed)?;
        let mut reader = BufReader::with_capacity(256 * 1024, stream);

        let hello = Hello::new(token, session, caps);
        writer
            .write_all(&encode_frame(&hello).map_err(wire_err)?)
            .and_then(|()| writer.flush())
            .map_err(closed)?;
        let reply = match read_frame::<_, HelloReply>(&mut reader) {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                return Err(ClientError::Closed(
                    "the daemon closed the connection during the handshake".into(),
                ))
            }
            Err(WireError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                let _ = control.shutdown();
                return Err(ClientError::Timeout);
            }
            Err(e) => return Err(wire_err(e)),
        };
        let welcome = match reply {
            HelloReply::Welcome(w) => w,
            HelloReply::Refused => return Err(ClientError::Refused),
            HelloReply::WrongVersion { version } => {
                return Err(ClientError::WrongVersion { daemon: version })
            }
        };
        if welcome.version != PROTOCOL_VERSION {
            return Err(ClientError::WrongVersion {
                daemon: welcome.version,
            });
        }
        if welcome.session != session {
            return Err(ClientError::SessionMismatch);
        }
        reader.get_ref().set_read_timeout(None).map_err(closed)?;

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let now = Instant::now();
        let inner = Arc::new(Inner {
            writer: Mutex::new(writer),
            control: Mutex::new(control),
            routes: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            next_nonce: AtomicU32::new(1),
            last_rx: Mutex::new(now),
            last_tx: Mutex::new(now),
            closed: Mutex::new(None),
            deliberate: AtomicBool::new(false),
            on_close: Mutex::new(OnCloseSlot::default()),
            stop_keepalive: Mutex::new(Some(stop_tx)),
        });

        let r = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("socketsweep-reader".into())
            .spawn(move || reader_loop(r, reader))
            .map_err(closed)?;
        let k = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("socketsweep-keepalive".into())
            .spawn(move || keepalive_loop(k, stop_rx, config))
            .map_err(closed)?;

        Ok(Client {
            inner,
            welcome: Arc::new(welcome),
        })
    }

    /// Call `callback` once if the connection ends other than by
    /// [`Client::close`], with the reason; at once if it already has.
    pub fn set_on_close(&self, callback: OnClose) {
        let why = {
            let mut slot = lock(&self.inner.on_close);
            let Some(why) = slot.fired.clone() else {
                slot.callback = Some(callback);
                return;
            };
            why
        };
        callback(why);
    }

    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Why the connection ended, or `None` while it is open.
    pub fn closed_reason(&self) -> Option<String> {
        lock(&self.inner.closed).clone()
    }

    pub fn is_open(&self) -> bool {
        self.closed_reason().is_none()
    }

    fn closed_error(&self) -> ClientError {
        ClientError::Closed(
            self.closed_reason()
                .unwrap_or_else(|| "the connection closed".into()),
        )
    }

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

    /// Delete `path` and wait for the phone to confirm, reporting
    /// `DeleteProgress` to `progress`. Returns the items removed and their
    /// totals. Waits as long as the delete takes: only a dead connection ends
    /// the wait early.
    pub fn delete(
        &self,
        path: Vec<u8>,
        hint: Option<(ReqId, DirId)>,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(u64, Totals), ClientError> {
        let (_, rx) = self.request(ClientMsg::Delete { path, hint })?;
        loop {
            match rx.recv() {
                Ok(ServerMsg::DeleteProgress { items }) => progress(items),
                Ok(ServerMsg::Deleted { items, totals }) => return Ok((items, totals)),
                Ok(ServerMsg::Failed {
                    code,
                    errno,
                    message,
                }) => {
                    return Err(ClientError::Failed {
                        code,
                        errno,
                        message,
                    })
                }
                Ok(other) => {
                    return Err(ClientError::Protocol(format!(
                        "unexpected reply to delete: {other:?}"
                    )))
                }
                Err(_) => return Err(self.closed_error()),
            }
        }
    }

    /// Ask for the copy manifest of `path`. The channel carries
    /// [`ServerMsg::CopyManifest`] parts until the `last` one, or a `Failed`.
    pub fn prepare_copy(&self, path: Vec<u8>) -> Result<(ReqId, Receiver<ServerMsg>), ClientError> {
        self.request(ClientMsg::PrepareCopy { path })
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

fn reader_loop(inner: Arc<Inner>, mut reader: BufReader<Box<dyn Duplex>>) {
    let why = loop {
        match read_frame::<_, ServerFrame>(&mut reader) {
            Ok(Some(frame)) => {
                *lock(&inner.last_rx) = Instant::now();
                inner.route(frame);
            }
            Ok(None) => break "the phone closed the connection".to_string(),
            Err(e) => break e.to_string(),
        }
    };
    // A reason recorded earlier (a deliberate close, a dead peer) wins.
    inner.close(why);
    lock(&inner.routes).clear();
    let why = lock(&inner.closed).clone().unwrap_or_default();
    if inner.deliberate.load(Ordering::SeqCst) {
        return;
    }
    let callback = {
        let mut slot = lock(&inner.on_close);
        slot.fired = Some(why.clone());
        slot.callback.take()
    };
    if let Some(callback) = callback {
        callback(why);
    }
}

fn keepalive_loop(inner: Arc<Inner>, stop: Receiver<()>, config: ClientConfig) {
    loop {
        match stop.recv_timeout(config.tick) {
            Err(RecvTimeoutError::Timeout) => {}
            // Stopped, or the connection is gone.
            _ => return,
        }
        let silent = lock(&inner.last_rx).elapsed();
        if silent >= config.dead_after {
            inner.close(format!(
                "the phone has not answered for {} s",
                silent.as_secs()
            ));
            return;
        }
        if lock(&inner.last_tx).elapsed() >= config.ping_interval {
            let id = inner.next_id();
            let nonce = inner.next_nonce.fetch_add(1, Ordering::Relaxed);
            // Nobody waits for the Pong: any frame resets `last_rx`.
            if inner
                .send(&ClientFrame {
                    id,
                    msg: ClientMsg::Ping { nonce },
                })
                .is_err()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;

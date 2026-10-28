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
//!
//! `connection` holds the shared connection state and frame routing,
//! `handshake` opens a connection, `requests` is every call on it, and
//! `threads` the reader and keepalive loops.

mod connection;
mod handshake;
mod requests;
mod threads;

use std::sync::Arc;
use std::time::Duration;

use socketsweep_wire::{
    ErrCode, Welcome, WireError, HOST_DEAD_AFTER, HOST_PING_INTERVAL, PROTOCOL_VERSION,
};

#[cfg(doc)]
use socketsweep_wire::{ReqId, ServerFrame, ServerMsg};

use crate::lock;
use connection::Inner;

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

/// Called once, from the reader thread, when the connection ends without
/// [`Client::close`] having been called. See [`Client::set_on_close`].
pub type OnClose = Box<dyn FnOnce(String) + Send>;

/// One authenticated connection to the daemon. Cheap to clone; every clone
/// is the same connection.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
    welcome: Arc<Welcome>,
}

impl Client {
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
}

fn wire_err(e: WireError) -> ClientError {
    ClientError::Closed(e.to_string())
}

#[cfg(test)]
mod tests;

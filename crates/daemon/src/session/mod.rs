//! `session`: one authenticated connection, from `Hello` to close.
//!
//! Portable: it sees the connection only as a reader, a writer and a way to
//! shut it down ([`Conn`]), and the phone only through the engine's [`Fs`] and
//! [`Actions`] ([`Services`]). The listener runs it over a unix socket; tests
//! and `socketsweep-sim`'s `FakeDaemon` run it over an in-memory pipe.
//!
//! # Threads
//!
//! - The thread calling [`serve`] is the **reader**: it decodes
//!   [`ClientFrame`]s and routes them.
//! - One **writer** thread sends the [`Outbox`]: control frames before bulk,
//!   scan events batched, heartbeats when idle; it gives up on a host silent
//!   for [`DAEMON_DEAD_AFTER`](socketsweep_wire::DAEMON_DEAD_AFTER) and shuts
//!   the connection, which wakes the reader.
//! - Each scan runs on a thread of its own (plus the engine's W workers and
//!   ticker); each delete and copy manifest on a thread of its own.
//!
//! # Routing
//!
//! | Request | Handling |
//! |---|---|
//! | `Ping` | `Pong`, on the control queue |
//! | `StartScan` | resolve the root ([`Actions::resolve_scan_root`], then [`Fs::root`]); `Failed` if either fails, else an engine [`Scan`] whose events go to the bulk queue under the request id |
//! | `Focus`, `ClearFocus` | the scan's handle; `Failed { BadRequest }` for an unknown scan |
//! | `Cancel` | the scan's handle, or the delete's or copy's flag |
//! | `Delete` | guard, then **prune the hinted directory first** (only if the hint names the path the guard resolved), then delete with `DeleteProgress` keepalives, then `Deleted { items, totals }` |
//! | `PrepareCopy` | guard, then walk, streaming `CopyManifest` parts |
//! | `Shutdown` | stop: [`End::Shutdown`] |
//!
//! Requests need a non-zero id not already in use (`BadRequest`,
//! `DuplicateId`). When the connection ends, for any reason, everything it
//! started is cancelled and waited for.

//!
//! # Layout
//!
//! `handshake` checks and answers the `Hello`; `serve` runs the reader and
//! the writer and tears the session down; `dispatch` routes each request;
//! `jobs` runs scans, deletes and copy manifests on their own threads.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use socketsweep_engine::{default_workers, Fs, Scan};
#[cfg(doc)]
use socketsweep_wire::ClientFrame;
use socketsweep_wire::{Caps, Limits, ReqId, Token};

use crate::actions::Actions;
use crate::outbox::{Outbox, OutboxConfig, ScanSink};

mod dispatch;
mod handshake;
mod jobs;
mod serve;

pub use handshake::{handshake, HandshakeError};
pub use serve::serve;

/// What this daemon can do, before intersecting with the host's `Hello`.
pub const DAEMON_CAPS: Caps = Caps::DISK_USAGE
    .union(Caps::LISTING_PARTS)
    .union(Caps::DEFERRED_STAT)
    .union(Caps::COPY_MANIFEST)
    .union(Caps::DIR_MTIME);

/// How one session behaves. [`SessionConfig::new`] gives the defaults.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// The session token from `SOCKETSWEEP_TOKEN`.
    pub token: Token,
    pub caps: Caps,
    pub daemon_build: String,
    /// Workers per scan unless `ScanOpts::workers` says otherwise; sent in
    /// `Welcome::workers`.
    pub workers: u8,
    pub limits: Limits,
    pub outbox: OutboxConfig,
    /// A delete sends `DeleteProgress` at most this often.
    pub progress_every: Duration,
}

impl SessionConfig {
    pub fn new(token: Token) -> Self {
        SessionConfig {
            token,
            caps: DAEMON_CAPS,
            daemon_build: crate::DAEMON_BUILD.to_string(),
            workers: default_workers(),
            limits: Limits::default(),
            outbox: OutboxConfig::default(),
            progress_every: Duration::from_millis(500),
        }
    }
}

/// The phone, as the session sees it.
#[derive(Clone)]
pub struct Services {
    pub fs: Arc<dyn Fs>,
    pub actions: Arc<dyn Actions>,
}

/// One connection: a reader, a writer, and a way to shut both down from any
/// thread (which must make a blocked read return).
pub struct Conn {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub shutdown: Arc<dyn Fn() + Send + Sync>,
}

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The host sent `Shutdown`: the daemon should exit.
    Shutdown,
    /// The connection ended, with the reason.
    Closed(String),
}

/// [`handshake`], then [`serve`].
pub fn run(mut conn: Conn, cfg: &SessionConfig, services: Services) -> Result<End, HandshakeError> {
    let (_, caps) = handshake(&mut conn, cfg)?;
    Ok(serve(conn, caps, cfg, services))
}

enum Active<S: socketsweep_engine::EventSink> {
    Scan(Scan<S>),
    Job(Arc<AtomicBool>),
}

struct Shared {
    outbox: Arc<Outbox>,
    services: Services,
    cfg: SessionConfig,
    caps: Caps,
    active: Mutex<HashMap<ReqId, Active<ScanSink>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

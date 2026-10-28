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

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use socketsweep_engine::{default_workers, Fs, FsError, Scan};
use socketsweep_wire::{
    check_hello, encode_frame, read_frame, read_frame_body, Caps, ClientFrame, ClientMsg, CopyItem,
    DirId, ErrCode, Hello, HelloCheck, HelloReply, Limits, ReqId, ScanOpts, ServerMsg, Token,
    WireError, MAX_LISTING_PART, SESSION_LEVEL,
};

use crate::actions::{ActionError, Actions};
use crate::log;
use crate::outbox::{Outbox, OutboxConfig, ScanSink};

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

/// Why the handshake did not lead to a session.
#[derive(Debug)]
pub enum HandshakeError {
    /// The peer closed before sending a `Hello`.
    Closed,
    /// Wrong magic, wrong token, or not a `Hello`: answered `Refused`.
    Refused,
    /// Right token, other version: answered `WrongVersion`.
    WrongVersion {
        host_version: u16,
    },
    Wire(WireError),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::Closed => write!(f, "closed before the handshake"),
            HandshakeError::Refused => write!(f, "refused: bad token or not a Hello"),
            HandshakeError::WrongVersion { host_version } => {
                write!(f, "refused: the host speaks protocol {host_version}")
            }
            HandshakeError::Wire(e) => write!(f, "handshake failed: {e}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Read the connection's first frame, check it with [`check_hello`] (token,
/// then version, then the rest) and answer it. Returns the accepted `Hello`
/// and the negotiated capabilities.
pub fn handshake(conn: &mut Conn, cfg: &SessionConfig) -> Result<(Hello, Caps), HandshakeError> {
    let body = match read_frame_body(&mut conn.reader) {
        Ok(Some(b)) => b,
        Ok(None) => return Err(HandshakeError::Closed),
        Err(e) => return Err(HandshakeError::Wire(e)),
    };
    let check = check_hello(&body, &cfg.token);
    let reply = match &check {
        HelloCheck::Accepted(hello) => {
            let welcome =
                hello.welcome(cfg.caps, cfg.daemon_build.clone(), cfg.workers, cfg.limits);
            HelloReply::Welcome(welcome)
        }
        rejected => rejected.rejection().expect("a rejection"),
    };
    let bytes = encode_frame(&reply).map_err(HandshakeError::Wire)?;
    conn.writer
        .write_all(&bytes)
        .and_then(|()| conn.writer.flush())
        .map_err(|e| HandshakeError::Wire(e.into()))?;
    match check {
        HelloCheck::Accepted(hello) => {
            let caps = hello.caps & cfg.caps;
            Ok((hello, caps))
        }
        HelloCheck::Refused => Err(HandshakeError::Refused),
        HelloCheck::WrongVersion { host_version } => {
            Err(HandshakeError::WrongVersion { host_version })
        }
    }
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

/// Serve an authenticated connection until the host sends `Shutdown`, the
/// connection ends, or the host goes silent. Cancels and waits for
/// everything the session started before returning, then shuts the
/// connection.
pub fn serve(conn: Conn, caps: Caps, cfg: &SessionConfig, services: Services) -> End {
    let Conn {
        reader,
        writer,
        shutdown,
    } = conn;
    let outbox = Outbox::new(cfg.outbox.clone());
    let shared = Arc::new(Shared {
        outbox: Arc::clone(&outbox),
        services,
        cfg: cfg.clone(),
        caps,
        active: Mutex::new(HashMap::new()),
        threads: Mutex::new(Vec::new()),
    });

    let ob = Arc::clone(&outbox);
    let on_dead = Arc::clone(&shutdown);
    let writer_thread = std::thread::Builder::new()
        .name("ssd-writer".into())
        .spawn(move || ob.run_writer(writer, move || on_dead()));
    let writer_thread = match writer_thread {
        Ok(t) => t,
        Err(e) => {
            shutdown();
            return End::Closed(format!("cannot start the writer thread: {e}"));
        }
    };

    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let end = loop {
        match read_frame::<_, ClientFrame>(&mut reader) {
            Ok(Some(frame)) => {
                outbox.touch_rx();
                if shared.dispatch(frame) {
                    break End::Shutdown;
                }
            }
            Ok(None) => {
                break End::Closed(
                    outbox
                        .closed()
                        .unwrap_or_else(|| "the host closed the connection".into()),
                )
            }
            Err(e) => break End::Closed(outbox.closed().unwrap_or_else(|| e.to_string())),
        }
    };

    shared.cancel_all();
    let threads = std::mem::take(&mut *lock(&shared.threads));
    for t in threads {
        let _ = t.join();
    }
    match &end {
        // Send what is already queued (the cancelled scans' `Finished`),
        // then stop.
        End::Shutdown => outbox.finish(),
        End::Closed(why) => outbox.close(why.clone()),
    }
    let _ = writer_thread.join();
    shutdown();
    end
}

impl Shared {
    /// Route one frame. Returns true for `Shutdown`.
    fn dispatch(self: &Arc<Self>, frame: ClientFrame) -> bool {
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
            ClientMsg::Cancel { target } => {
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
            ClientMsg::PrepareCopy { path } => {
                if !self.caps.contains(Caps::COPY_MANIFEST) {
                    self.fail(
                        id,
                        ActionError::new(ErrCode::Unsupported, "COPY_MANIFEST was not negotiated"),
                    );
                } else if self.claim(id) {
                    self.start_copy(id, path);
                }
            }
        }
        false
    }

    fn fail(&self, id: ReqId, e: ActionError) {
        self.outbox.reply(id, e.into_msg());
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

    fn scan(&self, id: ReqId) -> Option<Scan<ScanSink>> {
        match lock(&self.active).get(&id) {
            Some(Active::Scan(s)) => Some(s.clone()),
            _ => None,
        }
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

    fn spawn(self: &Arc<Self>, name: &str, f: impl FnOnce(Arc<Shared>) + Send + 'static) {
        let me = Arc::clone(self);
        match std::thread::Builder::new()
            .name(name.into())
            .spawn(move || f(me))
        {
            Ok(t) => {
                let mut threads = lock(&self.threads);
                threads.retain(|t| !t.is_finished());
                threads.push(t);
            }
            Err(e) => log!("cannot start a {name} thread: {e}"),
        }
    }

    fn start_scan(self: &Arc<Self>, id: ReqId, root: Vec<u8>, mut opts: ScanOpts) {
        let resolved = match self.services.actions.resolve_scan_root(&root) {
            Ok(r) => r,
            Err(e) => {
                log!("scan {id} refused: {}", e.message);
                return self.fail(id, e);
            }
        };
        let info = match self.services.fs.root(&resolved) {
            Ok(info) => info,
            Err(FsError::Failed { errno, .. }) => {
                let e = ActionError::errno(
                    errno,
                    format!(
                        "cannot open {}: errno {errno}",
                        String::from_utf8_lossy(&resolved)
                    ),
                );
                log!("scan {id} failed: {}", e.message);
                return self.fail(id, e);
            }
            Err(FsError::Interrupted) => {
                return self.fail(id, ActionError::cancelled());
            }
        };
        let limits = self.cfg.limits;
        opts.max_depth = opts.max_depth.min(limits.max_depth);
        let workers = opts
            .workers
            .unwrap_or(self.cfg.workers)
            .clamp(1, limits.max_workers);
        let sink = ScanSink {
            id,
            outbox: Arc::clone(&self.outbox),
        };
        let scan = Scan::with_workers(info, &opts, workers, sink);
        lock(&self.active).insert(id, Active::Scan(scan.clone()));
        log!(
            "scan {id} of {} with {workers} workers, {:?} mode",
            String::from_utf8_lossy(&resolved),
            opts.mode
        );
        self.spawn("ssd-scan", move |me| {
            let started = Instant::now();
            let outcome = scan.run(&*me.services.fs);
            let totals = scan.totals();
            lock(&me.active).remove(&id);
            log!(
                "scan {id}: {outcome:?} after {} ms, {} files, {} dirs, {} bytes on disk, {} errors",
                started.elapsed().as_millis(),
                totals.files,
                totals.dirs,
                totals.disk,
                totals.errors
            );
        });
    }

    fn start_delete(self: &Arc<Self>, id: ReqId, path: Vec<u8>, hint: Option<(ReqId, DirId)>) {
        let flag = Arc::new(AtomicBool::new(false));
        lock(&self.active).insert(id, Active::Job(Arc::clone(&flag)));
        self.spawn("ssd-delete", move |me| {
            let result = me.delete(id, &path, hint, &flag);
            lock(&me.active).remove(&id);
            match result {
                Ok((items, totals)) => {
                    log!("delete {id}: removed {items} items");
                    me.outbox.reply(id, ServerMsg::Deleted { items, totals });
                }
                Err(e) => {
                    log!("delete {id} failed: {}", e.message);
                    me.fail(id, e);
                }
            }
        });
    }

    fn delete(
        &self,
        id: ReqId,
        path: &[u8],
        hint: Option<(ReqId, DirId)>,
        cancel: &AtomicBool,
    ) -> Result<(u64, socketsweep_wire::Totals), ActionError> {
        let actions = &self.services.actions;
        let resolved = actions.resolve_delete(path)?;
        if let Some((scan_id, dir)) = hint {
            match self.scan(scan_id) {
                Some(scan) => match scan.path_of(dir) {
                    // Prune before touching the files, so the scan never
                    // reports half-deleted directories.
                    Some(p) if actions.same_entry(&p, &resolved) => {
                        scan.prune(dir);
                    }
                    _ => log!(
                        "delete {id}: hint ({scan_id}, {dir}) does not match the path; ignored"
                    ),
                },
                None => log!("delete {id}: hinted scan {scan_id} is not running"),
            }
        }
        let every = self.cfg.progress_every;
        let mut last = Instant::now();
        let outbox = &self.outbox;
        actions.delete(&resolved, cancel, &mut |items| {
            if last.elapsed() >= every {
                last = Instant::now();
                outbox.reply(id, ServerMsg::DeleteProgress { items });
            }
        })
    }

    fn start_copy(self: &Arc<Self>, id: ReqId, path: Vec<u8>) {
        let flag = Arc::new(AtomicBool::new(false));
        lock(&self.active).insert(id, Active::Job(Arc::clone(&flag)));
        self.spawn("ssd-copy", move |me| {
            let result = me.copy(id, &path, &flag);
            lock(&me.active).remove(&id);
            if let Err(e) = result {
                log!("copy manifest {id} failed: {}", e.message);
                me.fail(id, e);
            }
        });
    }

    fn copy(&self, id: ReqId, path: &[u8], cancel: &AtomicBool) -> Result<(), ActionError> {
        let actions = &self.services.actions;
        let root = actions.resolve_copy(path)?;
        let mut part: Vec<CopyItem> = Vec::new();
        actions.manifest(&root, cancel, &mut |item| {
            part.push(item);
            if part.len() >= MAX_LISTING_PART {
                self.outbox.reply(
                    id,
                    ServerMsg::CopyManifest {
                        root: root.clone(),
                        files: std::mem::take(&mut part),
                        last: false,
                    },
                );
            }
        })?;
        self.outbox.reply(
            id,
            ServerMsg::CopyManifest {
                root,
                files: part,
                last: true,
            },
        );
        Ok(())
    }

    /// The connection is ending: stop everything it started.
    fn cancel_all(&self) {
        let scans: Vec<Scan<ScanSink>> = lock(&self.active)
            .values()
            .filter_map(|a| match a {
                Active::Scan(s) => Some(s.clone()),
                Active::Job(flag) => {
                    flag.store(true, Ordering::Relaxed);
                    None
                }
            })
            .collect();
        for s in scans {
            s.cancel();
        }
    }
}

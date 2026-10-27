//! `FakeDaemon`: the real daemon session over an in-memory pipe, walking a
//! [`SimFs`] with the real engine (`docs/architecture.md`, section 1: the
//! fake on the host ↔ phone boundary).
//!
//! Everything above the file system is the production code:
//! `socketsweep_daemon::session` (handshake, routing, the writer's two queues,
//! heartbeats), the engine's scheduler and pool. Only the phone is simulated:
//! [`SimFs`] for scans, optionally slowed with [`SlowFs`], and [`SimActions`]
//! for guard, delete and copy manifest.
//!
//! Plug it into a `FakeTransport` with [`FakeDaemon::install`] (or
//! [`FakeDaemon::handler`] from a shell handler that reads the token from the
//! launch command), or talk to it directly with [`FakeDaemon::connect`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use socketsweep_daemon::actions::{ActionError, Actions};
use socketsweep_daemon::session::{self, Conn, End, HandshakeError, Services, SessionConfig};
use socketsweep_engine::Fs;
use socketsweep_transport::fake::{pipe, FakeTransport, PipeEnd};
use socketsweep_transport::Duplex;
use socketsweep_wire::{CopyItem, ErrCode, Kind, Token, Totals, BLOCK_SIZE};

use crate::simfs::{NodeId, SimFs};
use crate::slowfs::SlowFs;

/// [`Actions`] over a [`SimFs`]: paths must lie under its root and contain
/// no `.` or `..` components (a `SimFs` has no links to resolve).
pub struct SimActions {
    pub fs: Arc<SimFs>,
}

impl SimActions {
    fn root(&self) -> &[u8] {
        self.fs.root_path()
    }

    /// Check `path` names something under the root. `Ok(true)` for the root
    /// itself.
    fn check(&self, path: &[u8]) -> Result<bool, ActionError> {
        let root = self.root().strip_suffix(b"/").unwrap_or(self.root());
        if path == root || path == self.root() {
            return Ok(true);
        }
        let Some(rest) = path.strip_prefix(root).and_then(|r| r.strip_prefix(b"/")) else {
            return Err(ActionError::new(
                ErrCode::OutsideRoot,
                format!("{} is outside the root", String::from_utf8_lossy(path)),
            ));
        };
        if rest
            .split(|&b| b == b'/')
            .any(|c| c.is_empty() || c == b"." || c == b"..")
        {
            return Err(ActionError::new(
                ErrCode::BadRequest,
                "the path must be a plain path to an entry",
            ));
        }
        Ok(false)
    }

    fn node(&self, path: &[u8]) -> Result<NodeId, ActionError> {
        self.fs.find(path).ok_or_else(|| {
            ActionError::errno(
                socketsweep_engine::errno::ENOENT,
                format!("no such path: {}", String::from_utf8_lossy(path)),
            )
        })
    }

    /// Live nodes of the subtree at `id`, parents before children, with
    /// their paths relative to it.
    fn walk(&self, id: NodeId) -> Vec<(NodeId, Vec<u8>)> {
        let mut out = vec![(id, Vec::new())];
        let mut i = 0;
        while i < out.len() {
            let (x, rel) = out[i].clone();
            if self.fs.node(x).kind == Kind::Dir {
                for c in self.fs.children(x) {
                    let mut r = rel.clone();
                    if !r.is_empty() {
                        r.push(b'/');
                    }
                    r.extend_from_slice(&self.fs.node(c).name);
                    out.push((c, r));
                }
            }
            i += 1;
        }
        out
    }
}

impl Actions for SimActions {
    fn resolve_scan_root(&self, path: &[u8]) -> Result<Vec<u8>, ActionError> {
        self.check(path)?;
        let id = self.node(path)?;
        if self.fs.node(id).kind != Kind::Dir {
            return Err(ActionError::errno(20, "not a directory"));
        }
        Ok(path.to_vec())
    }

    fn resolve_delete(&self, path: &[u8]) -> Result<Vec<u8>, ActionError> {
        if self.check(path)? {
            return Err(ActionError::new(
                ErrCode::ProtectedRoot,
                "refusing to delete the root",
            ));
        }
        self.node(path)?;
        Ok(path.to_vec())
    }

    fn resolve_copy(&self, path: &[u8]) -> Result<Vec<u8>, ActionError> {
        self.check(path)?;
        self.node(path)?;
        Ok(path.to_vec())
    }

    fn delete(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(u64, Totals), ActionError> {
        let id = self.node(resolved)?;
        let mut totals = Totals::default();
        let mut items = 0;
        for (x, _) in self.walk(id) {
            if cancel.load(Ordering::Relaxed) {
                return Err(ActionError::cancelled());
            }
            let n = self.fs.node(x);
            if n.kind == Kind::Dir {
                totals.dirs += 1;
            } else {
                totals.files += 1;
            }
            totals.apparent += n.apparent;
            totals.disk += n.blocks * BLOCK_SIZE;
            items += 1;
            progress(items);
        }
        self.fs.delete(resolved);
        Ok((items, totals))
    }

    fn manifest(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        emit: &mut dyn FnMut(CopyItem),
    ) -> Result<(), ActionError> {
        let id = self.node(resolved)?;
        for (x, rel) in self.walk(id) {
            if cancel.load(Ordering::Relaxed) {
                return Err(ActionError::cancelled());
            }
            let n = self.fs.node(x);
            emit(CopyItem {
                rel,
                kind: n.kind,
                size: if n.kind == Kind::File { n.apparent } else { 0 },
                mode: if n.kind == Kind::Dir { 0o755 } else { 0o644 },
                mtime: n.mtime,
            });
        }
        Ok(())
    }
}

/// The daemon session over a [`SimFs`]. Cheap to clone; clones share the
/// file system.
#[derive(Clone)]
pub struct FakeDaemon {
    fs: Arc<SimFs>,
    engine_fs: Arc<dyn Fs>,
    cfg: SessionConfig,
    /// How every session served so far ended.
    pub ends: Arc<Mutex<Vec<Result<End, String>>>>,
}

fn conn_of(end: PipeEnd) -> Conn {
    let reader = end.try_clone().expect("pipe clone");
    let ctl = Mutex::new(end.try_clone().expect("pipe clone"));
    Conn {
        reader: Box::new(reader),
        writer: Box::new(end),
        shutdown: Arc::new(move || {
            let _ = ctl.lock().unwrap_or_else(|e| e.into_inner()).shutdown();
        }),
    }
}

impl FakeDaemon {
    /// A daemon over `fs` with no latency and 4 workers per scan.
    pub fn new(fs: SimFs) -> Self {
        let fs = Arc::new(fs);
        let mut cfg = SessionConfig::new([0; 32]);
        cfg.workers = 4;
        cfg.daemon_build = "fake-daemon".into();
        FakeDaemon {
            engine_fs: fs.clone(),
            fs,
            cfg,
            ends: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Slow every directory read and stat batch by `per_call`, plus
    /// `per_entry` per entry (real sleeps, see [`SlowFs`]).
    pub fn with_latency(mut self, per_call: Duration, per_entry: Duration) -> Self {
        self.engine_fs = Arc::new(SlowFs::new(Arc::clone(&self.fs), per_call, per_entry));
        self
    }

    /// Adjust the session configuration (workers, caps, outbox timing). The
    /// token is set per connection.
    pub fn with_config(mut self, f: impl FnOnce(&mut SessionConfig)) -> Self {
        f(&mut self.cfg);
        self
    }

    pub fn fs(&self) -> &Arc<SimFs> {
        &self.fs
    }

    fn services(&self) -> Services {
        Services {
            fs: Arc::clone(&self.engine_fs),
            actions: Arc::new(SimActions {
                fs: Arc::clone(&self.fs),
            }),
        }
    }

    /// Serve one connection, expecting `token`, until it ends.
    pub fn serve(&self, end: PipeEnd, token: Token) -> Result<End, HandshakeError> {
        let mut cfg = self.cfg.clone();
        cfg.token = token;
        let result = session::run(conn_of(end), &cfg, self.services());
        self.ends
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(result.as_ref().map(Clone::clone).map_err(|e| e.to_string()));
        result
    }

    /// A tunnel handler for `FakeTransport::on_tunnel`.
    pub fn handler(&self, token: Token) -> impl Fn(PipeEnd) + Send + Sync + 'static {
        let d = self.clone();
        move |end| {
            let _ = d.serve(end, token);
        }
    }

    /// Serve tunnels to `socket` on `serial` with `token`.
    pub fn install(&self, transport: &FakeTransport, serial: &str, socket: &str, token: Token) {
        transport.on_tunnel(serial, socket, self.handler(token));
    }

    /// Start a session on a fresh pipe and return the host's end.
    pub fn connect(&self, token: Token) -> (PipeEnd, JoinHandle<Result<End, HandshakeError>>) {
        let (host, daemon) = pipe();
        let d = self.clone();
        let t = std::thread::spawn(move || d.serve(daemon, token));
        (host, t)
    }
}

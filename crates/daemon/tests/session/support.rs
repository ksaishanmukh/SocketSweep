//! The in-memory session harness shared by the test files.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use socketsweep_daemon::actions::{path_to_bytes, LocalActions};
use socketsweep_daemon::session::{self, Conn, End, HandshakeError, Services, SessionConfig};
use socketsweep_engine::{DirRead, Fs, FsError, Interrupt, RootInfo, StatResult, StdFs};
use socketsweep_transport::fake::{pipe, PipeEnd};
use socketsweep_transport::Duplex;
use socketsweep_wire::{
    encode_frame, read_frame, Caps, ClientFrame, ClientMsg, DirId, Entry, Hello, HelloReply, Kind,
    Outcome, ReqId, ScanEvent, ServerFrame, ServerMsg, Token, Totals, ROOT_DIR,
};

pub const TOKEN: Token = [7; 32];
pub const WAIT: Duration = Duration::from_secs(20);

/// `StdFs` with a pause per call, so a scan is still running when the test
/// acts on it.
pub struct SlowFs(pub Duration);

impl Fs for SlowFs {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        StdFs.root(path)
    }
    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        std::thread::sleep(self.0);
        StdFs.read_names(dir, intr)
    }
    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError> {
        std::thread::sleep(self.0);
        StdFs.stat_all(dir, names, intr)
    }
    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        std::thread::sleep(self.0);
        StdFs.list_full(dir, intr)
    }
}

pub fn conn_of(end: PipeEnd) -> Conn {
    let reader = end.try_clone().unwrap();
    let ctl = Mutex::new(end.try_clone().unwrap());
    Conn {
        reader: Box::new(reader),
        writer: Box::new(end),
        shutdown: Arc::new(move || {
            let _ = ctl.lock().unwrap().shutdown();
        }),
    }
}

pub fn config() -> SessionConfig {
    let mut cfg = SessionConfig::new(TOKEN);
    cfg.workers = 4;
    cfg
}

/// A fixture tree: `a/` (a deep-ish subtree), `b/` (many files), loose files.
pub fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path().join("root");
    for d in ["a/x/y/z", "a/w", "b", "c/empty"] {
        std::fs::create_dir_all(r.join(d)).unwrap();
    }
    for (i, f) in ["a/1", "a/x/2", "a/x/y/3", "a/x/y/z/4", "a/w/5", "top"]
        .iter()
        .enumerate()
    {
        std::fs::write(r.join(f), vec![b'x'; 100 * (i + 1)]).unwrap();
    }
    for i in 0..300 {
        std::fs::write(r.join(format!("b/f{i}")), vec![b'y'; i]).unwrap();
    }
    tmp
}

pub fn root_of(tmp: &tempfile::TempDir) -> PathBuf {
    tmp.path().join("root")
}

/// Ground truth by walking with `std::fs`: files, dirs and apparent bytes
/// strictly below `dir`.
pub fn truth(dir: &Path) -> (u32, u32, u64) {
    let (mut files, mut dirs, mut bytes) = (0, 0, 0);
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let m = std::fs::symlink_metadata(e.path()).unwrap();
        bytes += m.len();
        if m.is_dir() {
            dirs += 1;
            let (f, d, b) = truth(&e.path());
            files += f;
            dirs += d;
            bytes += b;
        } else {
            files += 1;
        }
    }
    (files, dirs, bytes)
}

pub struct Host {
    pub out: PipeEnd,
    pub frames: Receiver<ServerFrame>,
    pub next: ReqId,
}

impl Host {
    pub fn send(&mut self, msg: ClientMsg) -> ReqId {
        self.next += 1;
        let id = self.next;
        self.send_as(id, msg);
        id
    }

    pub fn send_as(&mut self, id: ReqId, msg: ClientMsg) {
        let bytes = encode_frame(&ClientFrame { id, msg }).unwrap();
        self.out.write_all(&bytes).unwrap();
    }

    pub fn recv(&self) -> ServerFrame {
        self.frames.recv_timeout(WAIT).expect("a frame in time")
    }

    /// The next frame that is not a heartbeat or a scan batch.
    pub fn reply(&self) -> ServerFrame {
        loop {
            let f = self.recv();
            if !matches!(f.msg, ServerMsg::Heartbeat | ServerMsg::Scan(_)) {
                return f;
            }
        }
    }
}

pub type Session = JoinHandle<Result<End, HandshakeError>>;

pub fn start_with(cfg: SessionConfig, fs: Arc<dyn Fs>, root: &Path) -> (Session, PipeEnd) {
    let (host, daemon) = pipe();
    let services = Services {
        fs,
        actions: Arc::new(LocalActions::new(root)),
    };
    let t = std::thread::spawn(move || session::run(conn_of(daemon), &cfg, services));
    (t, host)
}

pub fn hello(token: Token, caps: Caps) -> Hello {
    Hello::new(token, [9; 16], caps)
}

/// Handshake as the host does, then read frames on a thread.
pub fn connect(cfg: SessionConfig, fs: Arc<dyn Fs>, root: &Path, caps: Caps) -> (Session, Host) {
    let (t, mut end) = start_with(cfg, fs, root);
    end.write_all(&encode_frame(&hello(TOKEN, caps)).unwrap())
        .unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert!(matches!(reply, HelloReply::Welcome(_)), "{reply:?}");
    let mut reader = end.try_clone().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(f)) = read_frame::<_, ServerFrame>(&mut reader) {
            if tx.send(f).is_err() {
                return;
            }
        }
    });
    (
        t,
        Host {
            out: end,
            frames: rx,
            next: 0,
        },
    )
}

/// Collect scan `id`'s events until `Finished`, handing every other frame to
/// `other`.
pub fn scan_events(host: &Host, id: ReqId, mut other: impl FnMut(ServerFrame)) -> Vec<ScanEvent> {
    let mut events = Vec::new();
    loop {
        let f = host.recv();
        match f.msg {
            ServerMsg::Scan(batch) if f.id == id => {
                let done = batch
                    .iter()
                    .any(|e| matches!(e, ScanEvent::Finished { .. }));
                events.extend(batch);
                if done {
                    return events;
                }
            }
            ServerMsg::Heartbeat => {}
            _ => other(f),
        }
    }
}

/// The subdirectories a listing introduces, each exactly once.
fn introduce_children(
    names: &mut HashMap<DirId, Vec<u8>>,
    parent: &mut HashMap<DirId, DirId>,
    dir: DirId,
    entries: &[Entry],
) {
    for en in entries {
        if let Some(id) = en.id {
            assert_eq!(en.kind, Kind::Dir);
            assert!(
                names.insert(id, en.name.clone()).is_none(),
                "G3: {id} twice"
            );
            parent.insert(id, dir);
        }
    }
}

/// The ordering guarantees as the host sees them, plus exactly-once listing and completion.
pub fn check_stream(events: &[ScanEvent]) -> HashMap<DirId, Vec<u8>> {
    let mut names: HashMap<DirId, Vec<u8>> = HashMap::new();
    let mut parent: HashMap<DirId, DirId> = HashMap::new();
    let mut listed: HashSet<DirId> = HashSet::new();
    let mut done: HashSet<DirId> = HashSet::new();
    let mut pruned: HashSet<DirId> = HashSet::new();
    let mut parts: HashMap<DirId, u16> = HashMap::new();
    for (i, e) in events.iter().enumerate() {
        let known = |d: &DirId, names: &HashMap<DirId, Vec<u8>>| {
            assert!(
                names.contains_key(d),
                "G1: event {i} names unknown dir {d}: {e:?}"
            );
        };
        match e {
            ScanEvent::Started { root, .. } => {
                assert_eq!(i, 0, "Started first");
                names.insert(ROOT_DIR, root.clone());
            }
            ScanEvent::Listing {
                dir,
                part,
                last,
                entries,
            } => {
                known(dir, &names);
                assert!(!listed.contains(dir), "G3: dir {dir} listed twice");
                let want = parts.entry(*dir).or_default();
                assert_eq!(part, want, "parts in order");
                *want += 1;
                introduce_children(&mut names, &mut parent, *dir, entries);
                if *last {
                    listed.insert(*dir);
                }
            }
            ScanEvent::Stats { dir, .. } | ScanEvent::DirFailed { dir, .. } => known(dir, &names),
            ScanEvent::SubtreeDone { dir, .. } => {
                known(dir, &names);
                assert!(listed.contains(dir), "G2: {dir} done before listed");
                assert!(done.insert(*dir), "G3: {dir} done twice");
                if let Some(p) = parent.get(dir) {
                    assert!(!done.contains(p), "G2: child {dir} done after parent {p}");
                }
            }
            ScanEvent::Pruned { dir } => {
                known(dir, &names);
                pruned.insert(*dir);
            }
            ScanEvent::FocusState { dir, .. } => known(dir, &names),
            ScanEvent::Finished { .. } => assert_eq!(i, events.len() - 1, "Finished last"),
            ScanEvent::SkeletonReady { .. } | ScanEvent::Progress { .. } => {}
        }
    }
    names
}

pub fn finished(events: &[ScanEvent]) -> (Outcome, Totals) {
    match events.last() {
        Some(ScanEvent::Finished {
            outcome, totals, ..
        }) => (*outcome, *totals),
        other => panic!("no Finished: {other:?}"),
    }
}

pub fn root_bytes(root: &Path) -> Vec<u8> {
    path_to_bytes(root)
}

pub fn child_path(root: &Path, rel: &str) -> Vec<u8> {
    let mut p = root_bytes(root);
    p.push(b'/');
    p.extend_from_slice(rel.as_bytes());
    p
}

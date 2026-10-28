//! The session end to end over an in-memory pipe, with the real engine on
//! `StdFs` and `LocalActions` over a temporary directory.

#![allow(clippy::too_many_lines)]

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use socketsweep_daemon::actions::{path_to_bytes, LocalActions};
use socketsweep_daemon::outbox::OutboxConfig;
use socketsweep_daemon::session::{self, Conn, End, HandshakeError, Services, SessionConfig};
use socketsweep_engine::{DirRead, Fs, FsError, Interrupt, RootInfo, StatResult, StdFs};
use socketsweep_transport::fake::{pipe, PipeEnd};
use socketsweep_transport::Duplex;
use socketsweep_wire::{
    encode_frame, read_frame, Caps, ClientFrame, ClientMsg, DirId, ErrCode, Hello, HelloReply,
    Kind, Outcome, ReqId, ScanEvent, ScanMode, ScanOpts, ServerFrame, ServerMsg, Token, Totals,
    PROTOCOL_VERSION, ROOT_DIR, SESSION_LEVEL,
};

const TOKEN: Token = [7; 32];
const WAIT: Duration = Duration::from_secs(20);

/// `StdFs` with a pause per call, so a scan is still running when the test
/// acts on it.
struct SlowFs(Duration);

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

fn conn_of(end: PipeEnd) -> Conn {
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

fn config() -> SessionConfig {
    let mut cfg = SessionConfig::new(TOKEN);
    cfg.workers = 4;
    cfg
}

/// A fixture tree: `a/` (a deep-ish subtree), `b/` (many files), loose files.
fn fixture() -> tempfile::TempDir {
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

fn root_of(tmp: &tempfile::TempDir) -> PathBuf {
    tmp.path().join("root")
}

/// Ground truth by walking with `std::fs`: files, dirs and apparent bytes
/// strictly below `dir`.
fn truth(dir: &Path) -> (u32, u32, u64) {
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

struct Host {
    out: PipeEnd,
    frames: Receiver<ServerFrame>,
    next: ReqId,
}

impl Host {
    fn send(&mut self, msg: ClientMsg) -> ReqId {
        self.next += 1;
        let id = self.next;
        self.send_as(id, msg);
        id
    }

    fn send_as(&mut self, id: ReqId, msg: ClientMsg) {
        let bytes = encode_frame(&ClientFrame { id, msg }).unwrap();
        self.out.write_all(&bytes).unwrap();
    }

    fn recv(&self) -> ServerFrame {
        self.frames.recv_timeout(WAIT).expect("a frame in time")
    }

    /// The next frame that is not a heartbeat or a scan batch.
    fn reply(&self) -> ServerFrame {
        loop {
            let f = self.recv();
            if !matches!(f.msg, ServerMsg::Heartbeat | ServerMsg::Scan(_)) {
                return f;
            }
        }
    }
}

type Session = JoinHandle<Result<End, HandshakeError>>;

fn start_with(cfg: SessionConfig, fs: Arc<dyn Fs>, root: &Path) -> (Session, PipeEnd) {
    let (host, daemon) = pipe();
    let services = Services {
        fs,
        actions: Arc::new(LocalActions::new(root)),
    };
    let t = std::thread::spawn(move || session::run(conn_of(daemon), &cfg, services));
    (t, host)
}

fn hello(token: Token, caps: Caps) -> Hello {
    Hello::new(token, [9; 16], caps)
}

/// Handshake as the host does, then read frames on a thread.
fn connect(cfg: SessionConfig, fs: Arc<dyn Fs>, root: &Path, caps: Caps) -> (Session, Host) {
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
fn scan_events(host: &Host, id: ReqId, mut other: impl FnMut(ServerFrame)) -> Vec<ScanEvent> {
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

/// G1–G3 as the host sees them, plus exactly-once listing and completion.
fn check_stream(events: &[ScanEvent]) -> HashMap<DirId, Vec<u8>> {
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
                for en in entries {
                    if let Some(id) = en.id {
                        assert_eq!(en.kind, Kind::Dir);
                        assert!(
                            names.insert(id, en.name.clone()).is_none(),
                            "G3: {id} twice"
                        );
                        parent.insert(id, *dir);
                    }
                }
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

fn finished(events: &[ScanEvent]) -> (Outcome, Totals) {
    match events.last() {
        Some(ScanEvent::Finished {
            outcome, totals, ..
        }) => (*outcome, *totals),
        other => panic!("no Finished: {other:?}"),
    }
}

fn root_bytes(root: &Path) -> Vec<u8> {
    path_to_bytes(root)
}

fn child_path(root: &Path, rel: &str) -> Vec<u8> {
    let mut p = root_bytes(root);
    p.push(b'/');
    p.extend_from_slice(rel.as_bytes());
    p
}

// ── Handshake ───────────────────────────────────────────────────────────────

#[test]
fn a_good_hello_is_welcomed_with_negotiated_caps() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root);
    let host_caps = Caps::DISK_USAGE | Caps::COPY_MANIFEST | Caps::RESUME;
    end.write_all(&encode_frame(&hello(TOKEN, host_caps)).unwrap())
        .unwrap();
    let HelloReply::Welcome(w) = read_frame(&mut end).unwrap().unwrap() else {
        panic!("not welcomed")
    };
    assert_eq!(w.version, PROTOCOL_VERSION);
    assert_eq!(w.session, [9; 16]);
    assert_eq!(w.caps, Caps::DISK_USAGE | Caps::COPY_MANIFEST);
    assert_eq!(w.workers, 4);
    assert_eq!(w.limits, socketsweep_wire::Limits::default());
    end.write_all(
        &encode_frame(&ClientFrame {
            id: 1,
            msg: ClientMsg::Shutdown,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(t.join().unwrap().unwrap(), End::Shutdown);
}

#[test]
fn a_wrong_token_is_refused_and_reveals_nothing() {
    let tmp = fixture();
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root_of(&tmp));
    // Wrong token *and* wrong version: the token is checked first.
    let mut h = hello([8; 32], Caps::all());
    h.version = 3;
    end.write_all(&encode_frame(&h).unwrap()).unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert_eq!(reply, HelloReply::Refused);
    assert!(matches!(t.join().unwrap(), Err(HandshakeError::Refused)));
    assert!(read_frame::<_, HelloReply>(&mut end).unwrap().is_none());
}

#[test]
fn a_right_token_of_another_version_is_told_ours() {
    let tmp = fixture();
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root_of(&tmp));
    let mut h = hello(TOKEN, Caps::all());
    h.version = 3;
    end.write_all(&encode_frame(&h).unwrap()).unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert_eq!(
        reply,
        HelloReply::WrongVersion {
            version: PROTOCOL_VERSION
        }
    );
    assert!(matches!(
        t.join().unwrap(),
        Err(HandshakeError::WrongVersion { host_version: 3 })
    ));
}

#[test]
fn garbage_instead_of_a_hello_is_refused() {
    let tmp = fixture();
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root_of(&tmp));
    end.write_all(&encode_frame(&ClientMsg::Shutdown).unwrap())
        .unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert_eq!(reply, HelloReply::Refused);
    assert!(t.join().unwrap().is_err());
}

// ── Requests ────────────────────────────────────────────────────────────────

#[test]
fn ping_shutdown_and_bad_ids() {
    let tmp = fixture();
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root_of(&tmp), Caps::all());
    let id = host.send(ClientMsg::Ping { nonce: 42 });
    assert_eq!(
        host.reply(),
        ServerFrame {
            id,
            msg: ServerMsg::Pong { nonce: 42 }
        }
    );

    host.send_as(
        0,
        ClientMsg::PrepareCopy {
            path: root_bytes(&root_of(&tmp)),
        },
    );
    let f = host.reply();
    assert!(
        matches!(
            f.msg,
            ServerMsg::Failed {
                code: ErrCode::BadRequest,
                ..
            }
        ),
        "{f:?}"
    );

    let id = host.send(ClientMsg::Focus {
        scan: 999,
        dir: 0,
        seq: 1,
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::BadRequest,
            ..
        }
    ));

    host.send(ClientMsg::Shutdown);
    assert_eq!(t.join().unwrap().unwrap(), End::Shutdown);
}

#[test]
fn a_duplicate_id_is_refused_while_the_first_runs() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(30)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let opts = ScanOpts::default();
    host.send_as(
        5,
        ClientMsg::StartScan {
            root: root_bytes(&root),
            opts: opts.clone(),
        },
    );
    host.send_as(
        5,
        ClientMsg::StartScan {
            root: root_bytes(&root),
            opts,
        },
    );
    let f = host.reply();
    assert_eq!(f.id, 5);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::DuplicateId,
            ..
        }
    ));
    host.send(ClientMsg::Cancel { target: 5 });
    let events = scan_events(&host, 5, |_| {});
    assert_eq!(finished(&events).0, Outcome::Cancelled);
    host.send(ClientMsg::Shutdown);
    assert_eq!(t.join().unwrap().unwrap(), End::Shutdown);
}

#[test]
fn a_scan_outside_the_root_fails_before_starting() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root.join("a"), Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::OutsideRoot,
            ..
        }
    ));
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn a_scan_streams_every_directory_once_with_true_totals() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    let events = scan_events(&host, id, |f| panic!("unexpected {f:?}"));
    let names = check_stream(&events);
    let (outcome, totals) = finished(&events);
    assert_eq!(outcome, Outcome::Complete);
    let (files, dirs, apparent) = truth(&root);
    assert_eq!(totals.files, files);
    assert_eq!(totals.dirs, dirs);
    assert_eq!(
        names.len() as u32,
        dirs + 1,
        "every directory introduced once"
    );
    // Directory sizes are platform-defined; file bytes are exact.
    assert_eq!(totals.apparent, apparent);
    assert_eq!(totals.errors, 0);
    let done = events
        .iter()
        .filter(|e| matches!(e, ScanEvent::SubtreeDone { .. }))
        .count();
    assert_eq!(done as u32, dirs + 1, "every directory finished once");
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

/// `ScanOpts::mode: Plain` (the benchmark baseline) reaches the engine: no
/// skeleton, no deferred stats, the same totals.
#[test]
fn a_plain_scan_is_honoured() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts {
            mode: ScanMode::Plain,
            ..ScanOpts::default()
        },
    });
    let events = scan_events(&host, id, |f| panic!("unexpected {f:?}"));
    check_stream(&events);
    let (outcome, totals) = finished(&events);
    assert_eq!(outcome, Outcome::Complete);
    let (files, dirs, apparent) = truth(&root);
    assert_eq!((totals.files, totals.dirs), (files, dirs));
    assert_eq!(totals.apparent, apparent);
    for e in &events {
        assert!(
            !matches!(e, ScanEvent::SkeletonReady { .. } | ScanEvent::Stats { .. }),
            "plain mode sent {e:?}"
        );
    }
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn focus_is_reported_and_cancel_ends_the_scan() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(40)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    // Wait for the root listing, then focus `a`.
    let mut events = Vec::new();
    let a = loop {
        let f = host.recv();
        if let ServerMsg::Scan(batch) = f.msg {
            events.extend(batch);
            let found = events.iter().find_map(|e| match e {
                ScanEvent::Listing {
                    dir: 0, entries, ..
                } => entries
                    .iter()
                    .find(|en| en.name == b"a")
                    .and_then(|en| en.id),
                _ => None,
            });
            if let Some(a) = found {
                break a;
            }
        }
    };
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ScanEvent::Finished { .. })),
        "finished too early: {events:?}"
    );
    host.send(ClientMsg::Focus {
        scan: id,
        dir: a,
        seq: 1,
    });
    let mut cancelled = false;
    loop {
        let f = host.recv();
        if let ServerMsg::Scan(batch) = f.msg {
            let focus = batch
                .iter()
                .any(|e| matches!(e, ScanEvent::FocusState { seq: 1, dir, .. } if *dir == a));
            let done = batch
                .iter()
                .any(|e| matches!(e, ScanEvent::Finished { .. }));
            events.extend(batch);
            if focus && !cancelled {
                host.send(ClientMsg::Cancel { target: id });
                cancelled = true;
            }
            if done {
                break;
            }
        }
    }
    assert!(cancelled, "a FocusState arrived");
    check_stream(&events);
    let (outcome, _) = finished(&events);
    // The scan may have finished on its own just before the cancel.
    assert!(matches!(outcome, Outcome::Cancelled | Outcome::Complete));
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn a_hinted_delete_prunes_first_and_totals_add_up() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (a_files, a_dirs, _) = truth(&root.join("a"));
    let (files, dirs, _) = truth(&root);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(30)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let scan = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    let mut events = Vec::new();
    let a = loop {
        let f = host.recv();
        if let ServerMsg::Scan(batch) = f.msg {
            events.extend(batch);
            let found = events.iter().find_map(|e| match e {
                ScanEvent::Listing {
                    dir: 0, entries, ..
                } => entries
                    .iter()
                    .find(|en| en.name == b"a")
                    .and_then(|en| en.id),
                _ => None,
            });
            if let Some(a) = found {
                break a;
            }
        }
    };
    let del = host.send(ClientMsg::Delete {
        path: child_path(&root, "a"),
        hint: Some((scan, a)),
    });
    let mut deleted = None;
    events.extend(scan_events(&host, scan, |f| {
        if f.id == del {
            deleted = Some(f.msg);
        }
    }));
    let deleted = deleted.unwrap_or_else(|| host.reply().msg);
    let ServerMsg::Deleted { items, totals } = deleted else {
        panic!("not deleted: {deleted:?}")
    };
    assert_eq!(items, u64::from(a_files + a_dirs + 1));
    assert_eq!(totals.files, a_files);
    assert_eq!(totals.dirs, a_dirs + 1);
    assert!(!root.join("a").exists());

    check_stream(&events);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScanEvent::Pruned { dir } if *dir == a)),
        "the subtree was pruned"
    );
    let (outcome, scan_totals) = finished(&events);
    assert_eq!(outcome, Outcome::Complete);
    assert_eq!(scan_totals.files, files - a_files);
    assert_eq!(scan_totals.dirs, dirs - a_dirs - 1);
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn deletes_are_guarded() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let cases = [
        (root_bytes(&root), ErrCode::ProtectedRoot),
        (child_path(&root, "a/.."), ErrCode::BadRequest),
        (child_path(&root, "../outside"), ErrCode::NotFound),
        (child_path(&root, "a/../.."), ErrCode::BadRequest),
        (child_path(&root, "nope"), ErrCode::NotFound),
    ];
    for (path, want) in cases {
        let id = host.send(ClientMsg::Delete { path, hint: None });
        let f = host.reply();
        assert_eq!(f.id, id);
        match f.msg {
            ServerMsg::Failed { code, .. } => assert_eq!(code, want),
            other => panic!("{other:?}"),
        }
    }
    std::fs::write(tmp.path().join("outside"), b"x").unwrap();
    let id = host.send(ClientMsg::Delete {
        path: child_path(&root, "../outside"),
        hint: None,
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::OutsideRoot,
            ..
        }
    ));
    assert!(tmp.path().join("outside").exists());

    // A file, with no scan running.
    let id = host.send(ClientMsg::Delete {
        path: child_path(&root, "top"),
        hint: None,
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(
        matches!(f.msg, ServerMsg::Deleted { items: 1, totals } if totals.files == 1 && totals.apparent == 600)
    );
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn a_copy_manifest_lists_the_subtree_in_parts() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let id = host.send(ClientMsg::PrepareCopy {
        path: child_path(&root, "a"),
    });
    let mut items = Vec::new();
    let manifest_root = loop {
        let f = host.reply();
        assert_eq!(f.id, id);
        let ServerMsg::CopyManifest { root, files, last } = f.msg else {
            panic!("{f:?}")
        };
        items.extend(files);
        if last {
            break root;
        }
    };
    let (files, dirs, _) = truth(&root.join("a"));
    assert_eq!(items.len() as u32, files + dirs + 1);
    assert_eq!(items[0].rel, b"");
    assert!(manifest_root.ends_with(b"a"));
    let five = items.iter().find(|i| i.rel == b"w/5").unwrap();
    assert_eq!((five.kind, five.size), (Kind::File, 500));

    let id = host.send(ClientMsg::PrepareCopy {
        path: root_bytes(&root),
    });
    let mut n = 0;
    loop {
        let f = host.reply();
        assert_eq!(f.id, id);
        let ServerMsg::CopyManifest { files, last, .. } = f.msg else {
            panic!("{f:?}")
        };
        n += files.len();
        if last {
            break;
        }
    }
    let (files, dirs, _) = truth(&root);
    assert_eq!(n as u32, files + dirs + 1, "the root itself may be copied");
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn copy_needs_the_capability() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::DISK_USAGE);
    let id = host.send(ClientMsg::PrepareCopy {
        path: child_path(&root, "a"),
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::Unsupported,
            ..
        }
    ));
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

// ── Keepalive and connection loss ──────────────────────────────────────────

#[test]
fn an_idle_session_sends_heartbeats() {
    let tmp = fixture();
    let mut cfg = config();
    cfg.outbox = OutboxConfig {
        heartbeat: Duration::from_millis(30),
        ..OutboxConfig::default()
    };
    let (t, mut host) = connect(cfg, Arc::new(StdFs), &root_of(&tmp), Caps::all());
    for _ in 0..3 {
        assert_eq!(
            host.recv(),
            ServerFrame {
                id: SESSION_LEVEL,
                msg: ServerMsg::Heartbeat
            }
        );
    }
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn a_silent_host_is_dropped() {
    let tmp = fixture();
    let mut cfg = config();
    cfg.outbox = OutboxConfig {
        dead_after: Duration::from_millis(200),
        ..OutboxConfig::default()
    };
    let (t, host) = connect(cfg, Arc::new(StdFs), &root_of(&tmp), Caps::all());
    let end = t.join().unwrap().unwrap();
    assert!(
        matches!(&end, End::Closed(why) if why.contains("sent nothing")),
        "{end:?}"
    );
    drop(host);
}

#[test]
fn a_lost_connection_cancels_a_running_scan() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(50)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    // The scan has started.
    loop {
        let f = host.recv();
        if f.id == id {
            break;
        }
    }
    host.out.shutdown().unwrap();
    drop(host);
    let end = t.join().unwrap().unwrap();
    assert!(matches!(end, End::Closed(_)), "{end:?}");
}

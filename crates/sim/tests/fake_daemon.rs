//! `FakeDaemon` end to end, speaking wire v4 directly: handshake, a scan
//! checked against `SimFs`'s ground truth and G1–G3, focus, and a hinted
//! delete during the scan.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use socketsweep_sim::{Checker, FakeDaemon, RandomShape, SimFs};
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_transport::{Duplex, Transport};
use socketsweep_wire::{
    encode_frame, read_frame, Caps, ClientFrame, ClientMsg, DirId, Hello, HelloReply, Outcome,
    ReqId, ScanEvent, ScanOpts, ServerFrame, ServerMsg, Token, Totals, ROOT_DIR,
};

const TOKEN: Token = [3; 32];
const WAIT: Duration = Duration::from_secs(30);

struct Host {
    out: Box<dyn Duplex>,
    frames: Receiver<ServerFrame>,
    next: ReqId,
}

impl Host {
    /// Handshake on `stream` as the host does, then read frames on a thread.
    fn connect(mut stream: Box<dyn Duplex>) -> Host {
        let hello = Hello::new(TOKEN, [1; 16], Caps::all());
        stream.write_all(&encode_frame(&hello).unwrap()).unwrap();
        let reply: HelloReply = read_frame(&mut stream).unwrap().unwrap();
        let HelloReply::Welcome(w) = reply else {
            panic!("refused: {reply:?}")
        };
        assert_eq!(w.session, [1; 16]);
        assert!(w.caps.contains(Caps::COPY_MANIFEST));
        let mut reader = stream.try_clone().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(f)) = read_frame::<_, ServerFrame>(&mut reader) {
                if tx.send(f).is_err() {
                    return;
                }
            }
        });
        Host {
            out: stream,
            frames: rx,
            next: 0,
        }
    }

    fn send(&mut self, msg: ClientMsg) -> ReqId {
        self.next += 1;
        let bytes = encode_frame(&ClientFrame { id: self.next, msg }).unwrap();
        self.out.write_all(&bytes).unwrap();
        self.next
    }

    fn recv(&self) -> ServerFrame {
        self.frames.recv_timeout(WAIT).expect("a frame in time")
    }
}

fn tree(seed: u64) -> SimFs {
    SimFs::random(
        seed,
        RandomShape {
            max_dirs: 150,
            ..RandomShape::default()
        },
    )
}

fn finished(events: &[ScanEvent]) -> (Outcome, socketsweep_wire::Totals) {
    match events.last() {
        Some(ScanEvent::Finished {
            outcome, totals, ..
        }) => (*outcome, *totals),
        other => panic!("no Finished: {other:?}"),
    }
}

#[test]
fn a_scan_through_the_fake_daemon_is_exact() {
    for seed in 1..=4 {
        let daemon = FakeDaemon::new(tree(seed));
        let (end, session) = daemon.connect(TOKEN);
        let mut host = Host::connect(Box::new(end));
        let root = daemon.fs().root_path().to_vec();
        let opts = ScanOpts::default();
        let id = host.send(ClientMsg::StartScan {
            root,
            opts: opts.clone(),
        });

        let mut checker = Checker::new();
        let mut events = Vec::new();
        loop {
            let f = host.recv();
            let ServerMsg::Scan(batch) = f.msg else {
                continue;
            };
            assert_eq!(f.id, id);
            for e in &batch {
                checker
                    .feed(e)
                    .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            }
            let done = batch
                .iter()
                .any(|e| matches!(e, ScanEvent::Finished { .. }));
            events.extend(batch);
            if done {
                break;
            }
        }
        checker.finish().unwrap();
        let truth = daemon.fs().truth(opts.max_depth, opts.one_filesystem);
        let (outcome, totals) = finished(&events);
        assert_eq!(outcome, Outcome::Complete);
        assert_eq!(totals, truth.root(), "seed {seed}");
        daemon.fs().check_counters(&truth).unwrap();

        host.send(ClientMsg::Shutdown);
        session.join().unwrap().unwrap();
    }
}

/// Scan frames fed through the order checker; every other frame is handed
/// back to the caller.
struct Observer {
    scan: ReqId,
    checker: Checker,
    events: Vec<ScanEvent>,
}

impl Observer {
    fn take(&mut self, host: &Host) -> Option<ServerFrame> {
        let f = host.recv();
        match f.msg {
            ServerMsg::Scan(batch) => {
                assert_eq!(f.id, self.scan);
                for e in &batch {
                    self.checker.feed(e).unwrap();
                }
                self.events.extend(batch);
                None
            }
            ServerMsg::Heartbeat => None,
            _ => Some(f),
        }
    }

    fn finished(&self) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, ScanEvent::Finished { .. }))
    }

    /// The root's subdirectories, once listed.
    fn root_children(&mut self, host: &Host) -> Vec<(Vec<u8>, DirId)> {
        loop {
            assert!(self.take(host).is_none());
            let found = self.events.iter().find_map(|e| match e {
                ScanEvent::Listing {
                    dir: ROOT_DIR,
                    entries,
                    ..
                } => Some(
                    entries
                        .iter()
                        .filter_map(|en| en.id.map(|id| (en.name.clone(), id)))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            });
            if let Some(c) = found {
                return c;
            }
        }
    }

    /// Until the scan has finished and request `del` has answered; the
    /// delete's item count and totals.
    fn await_delete(&mut self, host: &Host, del: ReqId) -> (u64, Totals) {
        let mut deleted = None;
        while !self.finished() || deleted.is_none() {
            if let Some(f) = self.take(host) {
                assert_eq!(f.id, del, "{f:?}");
                if let ServerMsg::Deleted { items, totals } = f.msg {
                    deleted = Some((items, totals));
                } else if !matches!(f.msg, ServerMsg::DeleteProgress { .. }) {
                    panic!("delete failed: {f:?}");
                }
            }
        }
        deleted.unwrap()
    }

    /// The number of files in a copy manifest of `root`.
    fn copy_manifest_len(&mut self, host: &mut Host, root: &[u8]) -> usize {
        let copy = host.send(ClientMsg::PrepareCopy {
            path: root.to_vec(),
        });
        let mut n = 0;
        loop {
            let f = self.take(host).expect("not a scan frame");
            assert_eq!(f.id, copy);
            let ServerMsg::CopyManifest { files, last, .. } = f.msg else {
                panic!("{f:?}")
            };
            n += files.len();
            if last {
                return n;
            }
        }
    }
}

/// A slow daemon over a random 200-directory tree, and a connected host.
fn slow_daemon_and_host() -> (FakeDaemon, Host) {
    let daemon = FakeDaemon::new(SimFs::random(
        7,
        RandomShape {
            max_dirs: 200,
            faults: false,
            ..RandomShape::default()
        },
    ))
    .with_latency(Duration::from_millis(3), Duration::ZERO);
    let transport = FakeTransport::new().with_device(FakeDevice::ready("phone"));
    daemon.install(&transport, "phone", "socketsweep-test", TOKEN);
    let host = Host::connect(transport.open_tunnel("phone", "socketsweep-test").unwrap());
    (daemon, host)
}

#[test]
fn focus_and_a_hinted_delete_during_a_slow_scan() {
    let (daemon, mut host) = slow_daemon_and_host();
    let root = daemon.fs().root_path().to_vec();
    let opts = ScanOpts::default();
    let scan = host.send(ClientMsg::StartScan {
        root: root.clone(),
        opts: opts.clone(),
    });
    let mut observer = Observer {
        scan,
        checker: Checker::new(),
        events: Vec::new(),
    };

    let children = observer.root_children(&host);
    assert!(children.len() >= 2, "the tree has subdirectories");
    let (_, focus_dir) = children[0].clone();
    let (victim_name, victim) = children[children.len() - 1].clone();

    host.send(ClientMsg::Focus {
        scan,
        dir: focus_dir,
        seq: 1,
    });
    let mut victim_path = root.clone();
    victim_path.push(b'/');
    victim_path.extend_from_slice(&victim_name);
    let del = host.send(ClientMsg::Delete {
        path: victim_path.clone(),
        hint: Some((scan, victim)),
    });

    let (items, del_totals) = observer.await_delete(&host, del);
    assert!(items >= 1);
    assert!(del_totals.dirs >= 1);
    assert!(daemon.fs().find(&victim_path).is_none());

    let events = &observer.events;
    assert!(events
        .iter()
        .any(|e| matches!(e, ScanEvent::FocusState { seq: 1, dir, .. } if *dir == focus_dir)));
    let pruned = events
        .iter()
        .any(|e| matches!(e, ScanEvent::Pruned { dir } if *dir == victim));
    let (outcome, totals) = finished(events);
    assert_eq!(outcome, Outcome::Complete);
    // Pruned (normally) or deleted before the scan reached it: either way
    // the totals are those of the tree that is left.
    let truth = daemon.fs().truth(opts.max_depth, opts.one_filesystem);
    assert_eq!(totals, truth.root(), "pruned: {pruned}");
    daemon.fs().check_counters(&truth).unwrap();

    // A copy manifest of what is left of the root.
    let n = observer.copy_manifest_len(&mut host, &root);
    assert!(n as u32 > truth.root().dirs);
    observer.checker.finish().unwrap();

    host.send(ClientMsg::Shutdown);
    let mut rest = Vec::new();
    let _ = host.out.read_to_end(&mut rest);
}

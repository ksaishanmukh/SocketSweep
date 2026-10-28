#![allow(clippy::too_many_lines)]

use super::scan::NEXT_SESSION;
use super::*;
use crate::client::ClientConfig;
use crate::lock;
use crate::testkit::{wait_for, FakeDaemon, Greeting, Responder};
use socketsweep_gui_contract::{
    self as gui, Commands, ErrorCode, Measure, NodeRef, ScanOutcome, ScanProgress, View,
};
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_wire::{ClientFrame, ClientMsg, CopyItem, Entry, Kind, Stat, Totals};
use socketsweep_wire::{Outcome, ScanEvent, ServerMsg};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Instant;

const RESOLVED: &[u8] = b"/storage/emulated/0";

struct Rig {
    app: App,
    transport: Arc<FakeTransport>,
    daemon: FakeDaemon,
    events: Mutex<mpsc::Receiver<HostEvent>>,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    fn new(daemon: FakeDaemon) -> Rig {
        let dir = std::env::temp_dir().join(format!(
            "socketsweep-app-{}-{}",
            std::process::id(),
            NEXT_SESSION.fetch_add(1000, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("daemon-arm64");
        std::fs::write(&binary, b"\x7fELF").unwrap();

        let transport = Arc::new(FakeTransport::new().with_device(FakeDevice::ready("P1")));
        daemon.install(&transport, "P1");
        let config = AppConfig {
            daemon: DaemonBinaries {
                arm64: Some(binary),
                x86_64: None,
            },
            launch: LaunchOptions {
                startup_deadline: Duration::from_secs(5),
                retry_gap: Duration::from_millis(5),
                client: ClientConfig {
                    ping_interval: Duration::from_millis(50),
                    dead_after: Duration::from_millis(500),
                    handshake_timeout: Duration::from_secs(5),
                    tick: Duration::from_millis(10),
                },
            },
            history_file: Some(dir.join("history.json")),
            default_destination: Some(dir.join("Desktop")),
            progress_interval: Duration::from_millis(20),
            ..AppConfig::default()
        };
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let t: Arc<dyn Transport> = transport.clone();
        let app = App::new(t, config, move |e| {
            let _ = lock(&tx).send(e);
        });
        Rig {
            app,
            transport,
            daemon,
            events: Mutex::new(rx),
            dir,
        }
    }

    /// Wait for an event matching `pred`.
    fn wait_event(&self, mut pred: impl FnMut(&HostEvent) -> bool) -> HostEvent {
        let rx = lock(&self.events);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let e = rx
                .recv_timeout(left)
                .expect("timed out waiting for an event");
            if pred(&e) {
                return e;
            }
        }
    }

    fn wait_finished(&self) -> ScanProgress {
        match self.wait_event(|e| matches!(e, HostEvent::ScanProgress(p) if p.finished.is_some())) {
            HostEvent::ScanProgress(p) => p,
            _ => unreachable!(),
        }
    }
}

fn stat(apparent: u64) -> Option<Stat> {
    Some(Stat {
        apparent,
        blocks: apparent.div_ceil(512),
        mtime: 0,
    })
}

fn entry(name: &str, kind: Kind, size: u64, id: Option<u32>) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind,
        stat: stat(size),
        id,
    }
}

fn totals(apparent: u64, files: u32, dirs: u32) -> Totals {
    Totals {
        apparent,
        disk: apparent.div_ceil(512) * 512,
        files,
        dirs,
        ..Totals::default()
    }
}

/// A daemon serving `/sdcard` = { DCIM/ { pic.jpg 1000 }, a.txt 100 }. The
/// scan stops after the root's listing until a `Focus` arrives (then
/// finishes) or a `Cancel` (then is cancelled).
fn phone() -> FakeDaemon {
    let scan: Arc<Mutex<Option<(u32, Responder)>>> = Arc::new(Mutex::new(None));
    FakeDaemon::new(
        move |frame: &ClientFrame, out: &Responder| match &frame.msg {
            ClientMsg::StartScan { .. } => {
                *lock(&scan) = Some((frame.id, out.clone()));
                out.send(
                    frame.id,
                    ServerMsg::Scan(vec![
                        ScanEvent::Started {
                            root: RESOLVED.to_vec(),
                            dev: 1,
                            workers: 4,
                        },
                        ScanEvent::Listing {
                            dir: 0,
                            part: 0,
                            last: true,
                            entries: vec![
                                entry("DCIM", Kind::Dir, 0, Some(1)),
                                entry("a.txt", Kind::File, 100, None),
                            ],
                        },
                    ]),
                );
            }
            ClientMsg::Focus { scan: id, dir, seq } => {
                assert_eq!(Some(*id), lock(&scan).as_ref().map(|s| s.0));
                out.send(
                    *id,
                    ServerMsg::Scan(vec![
                        ScanEvent::Listing {
                            dir: 1,
                            part: 0,
                            last: true,
                            entries: vec![entry("pic.jpg", Kind::File, 1000, None)],
                        },
                        ScanEvent::SubtreeDone {
                            dir: 1,
                            totals: totals(1000, 1, 0),
                        },
                        ScanEvent::FocusState {
                            seq: *seq,
                            dir: *dir,
                            settled: true,
                        },
                        ScanEvent::SubtreeDone {
                            dir: 0,
                            totals: totals(1100, 2, 1),
                        },
                        ScanEvent::Finished {
                            outcome: Outcome::Complete,
                            totals: totals(1100, 2, 1),
                            elapsed_ms: 5,
                        },
                    ]),
                );
            }
            ClientMsg::Cancel { target } => {
                if lock(&scan).as_ref().is_some_and(|s| s.0 == *target) {
                    out.send(
                        *target,
                        ServerMsg::Scan(vec![ScanEvent::Finished {
                            outcome: Outcome::Cancelled,
                            totals: Totals::default(),
                            elapsed_ms: 5,
                        }]),
                    );
                }
            }
            ClientMsg::Delete { .. } => {
                out.send(
                    frame.id,
                    ServerMsg::Deleted {
                        items: 1,
                        totals: totals(100, 1, 0),
                    },
                );
            }
            ClientMsg::PrepareCopy { path } => {
                let item = |rel: &[u8], kind, size| CopyItem {
                    rel: rel.to_vec(),
                    kind,
                    size,
                    mode: 0o644,
                    mtime: 0,
                };
                out.send(
                    frame.id,
                    ServerMsg::CopyManifest {
                        root: path.clone(),
                        files: vec![item(b"", Kind::Dir, 0)],
                        last: false,
                    },
                );
                out.send(
                    frame.id,
                    ServerMsg::CopyManifest {
                        root: path.clone(),
                        files: vec![
                            item(b"pic.jpg", Kind::File, 4),
                            item(b"link", Kind::Symlink, 0),
                        ],
                        last: true,
                    },
                );
            }
            _ => {}
        },
    )
}

fn row(view: &View, name: &str) -> NodeRef {
    view.rows
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no row {name} in {view:?}"))
        .node
}

#[test]
fn connect_scan_focus_delete_copy_and_disconnect() {
    let rig = Rig::new(phone());
    let app = &rig.app;

    assert_eq!(
        app.get_view(None, None, None).unwrap_err().code,
        ErrorCode::NoScan
    );
    assert_eq!(
        app.start_scan(None).unwrap_err().code,
        ErrorCode::NotConnected
    );

    let devices = app.list_devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert!(devices[0].usable);

    let connected = app.connect(None, None).unwrap();
    assert_eq!(connected.serial, "P1");
    assert_eq!(connected.root, "/sdcard");
    assert!(connected.previous.is_none());
    assert!(
        connected
            .copy_destination
            .as_deref()
            .is_some_and(|d| d.ends_with("Desktop")),
        "{connected:?}"
    );
    let log = rig.transport.shell_log("P1");
    assert!(
        log.iter().any(|c| c.contains("--root '/sdcard'")),
        "{log:?}"
    );
    assert_eq!(
        rig.transport
            .file("P1", crate::daemon::DEVICE_BIN.as_bytes())
            .unwrap()
            .mode,
        0o755
    );

    let started = app.start_scan(None).unwrap();
    assert_eq!(started.path, "/sdcard");
    let view = app.get_view(None, None, None).unwrap();
    assert_eq!(view.path, "/sdcard");
    assert_eq!(view.measure, Measure::Partial);
    let dcim = row(&view, "DCIM");
    let a = row(&view, "a.txt");
    assert_eq!(
        view.rows[1].measure,
        Measure::Unknown,
        "DCIM is not listed yet"
    );

    // Focusing a file means its folder: the root, which the scan starts in,
    // so nothing is sent. The folder itself is sent at once.
    app.focus(Some(a), None, None).unwrap();
    app.focus(Some(dcim), None, None).unwrap();
    let progress = rig.wait_finished();
    assert_eq!(progress.finished.unwrap().outcome, ScanOutcome::Complete);
    assert_eq!(progress.totals.size.apparent, 1100);
    assert_eq!(
        progress.focus.unwrap(),
        gui::FocusState {
            node: dcim,
            settled: true
        }
    );
    assert_eq!(
        progress.view.unwrap().node,
        dcim,
        "progress carries the focused folder"
    );
    let focus: Vec<_> = rig
        .daemon
        .messages()
        .into_iter()
        .filter(|m| matches!(m, ClientMsg::Focus { .. } | ClientMsg::ClearFocus { .. }))
        .collect();
    assert!(
        matches!(focus[..], [ClientMsg::Focus { dir: 1, seq: 1, .. }]),
        "{focus:?}"
    );

    let crumbs = app.get_breadcrumbs(dcim).unwrap();
    assert_eq!(crumbs.len(), 2);
    assert_eq!(app.largest_files(None, None).unwrap()[0].name, "pic.jpg");
    assert_eq!(app.search("PIC".into(), None, None).unwrap().len(), 1);
    assert_eq!(app.get_treemap(None, None, None).unwrap().children.len(), 2);
    assert_eq!(app.type_breakdown(None).unwrap()[0].label, "Photos");
    assert!(app.app_breakdown(None, None).unwrap().is_empty());

    // History records the completed scan.
    let history = History::new(rig.dir.join("history.json"));
    assert_eq!(
        history.previous("P1", "/sdcard").unwrap().size.apparent,
        1100
    );

    // Delete.
    assert_eq!(
        app.delete(started.root).unwrap_err().code,
        ErrorCode::Invalid
    );
    let deleted = app.delete(a).unwrap();
    assert_eq!(deleted.items, 1);
    assert_eq!(deleted.totals.size.apparent, 1000);
    assert_eq!(deleted.view.node, dcim);
    assert!(rig.daemon.messages().contains(&ClientMsg::Delete {
        path: b"/storage/emulated/0/a.txt".to_vec(),
        hint: None
    }));
    assert_eq!(app.delete(a).unwrap_err().code, ErrorCode::Removed);

    // Copy.
    rig.transport.put_file(
        "P1",
        b"/storage/emulated/0/DCIM/pic.jpg",
        b"jpeg".to_vec(),
        0o644,
    );
    let copied = app.copy_to_desktop(dcim, None).unwrap();
    assert_eq!((copied.files, copied.bytes, copied.skipped), (1, 4, 1));
    let local = rig.dir.join("Desktop").join("DCIM").join("pic.jpg");
    assert_eq!(std::fs::read(&local).unwrap(), b"jpeg");
    assert_eq!(
        Path::new(&copied.destination),
        rig.dir.join("Desktop").join("DCIM")
    );
    rig.wait_event(
        |e| matches!(e, HostEvent::CopyProgress(p) if p.files_done == 1 && p.totals_known),
    );

    // A rescan makes every old node stale.
    app.start_scan(None).unwrap();
    assert_eq!(
        app.get_view(Some(dcim), None, None).unwrap_err().code,
        ErrorCode::Stale
    );
    app.cancel_scan().unwrap();
    let progress = rig.wait_finished();
    assert_eq!(progress.finished.unwrap().outcome, ScanOutcome::Cancelled);

    app.disconnect().unwrap();
    assert_eq!(
        app.get_view(None, None, None).unwrap_err().code,
        ErrorCode::NoScan
    );
    assert!(rig
        .transport
        .shell_log("P1")
        .iter()
        .any(|c| c.starts_with("rm -f")));
}

#[test]
fn a_protocol_violation_fails_the_scan_and_cancels_it() {
    let daemon = FakeDaemon::new(|frame, out| {
        if let ClientMsg::StartScan { .. } = frame.msg {
            out.send(
                frame.id,
                ServerMsg::Scan(vec![
                    ScanEvent::Started {
                        root: RESOLVED.to_vec(),
                        dev: 1,
                        workers: 4,
                    },
                    ScanEvent::SubtreeDone {
                        dir: 42,
                        totals: Totals::default(),
                    },
                ]),
            );
        }
    });
    let rig = Rig::new(daemon);
    rig.app.connect(Some("P1".into()), None).unwrap();
    let started = rig.app.start_scan(None).unwrap();
    let progress = rig.wait_finished();
    let finished = progress.finished.unwrap();
    assert_eq!(finished.outcome, ScanOutcome::Failed);
    assert!(finished.message.unwrap().contains("42"));
    assert!(wait_for(Duration::from_secs(5), || rig
        .daemon
        .messages()
        .iter()
        .any(|m| matches!(m, ClientMsg::Cancel { .. }))));
    // The tree so far stays queryable.
    assert_eq!(
        rig.app
            .get_view(Some(started.root), None, None)
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[test]
fn a_dead_phone_is_reported_as_connection_lost() {
    let rig = Rig::new(FakeDaemon::new(|_, _| {}).with_greeting(Greeting::AcceptThenSilent));
    rig.app.connect(None, None).unwrap();
    let lost = rig.wait_event(|e| matches!(e, HostEvent::ConnectionLost(_)));
    let HostEvent::ConnectionLost(lost) = lost else {
        unreachable!()
    };
    assert_eq!(lost.serial, "P1");
    assert_eq!(
        rig.app.start_scan(None).unwrap_err().code,
        ErrorCode::NotConnected
    );
    assert_eq!(HostEvent::ConnectionLost(lost).name(), "connection-lost");
}

#[test]
fn bad_arguments_are_refused() {
    let rig = Rig::new(phone());
    let e = rig.app.connect(None, Some("/data".into())).unwrap_err();
    assert_eq!(e.code, ErrorCode::Invalid);
    let e = rig.app.connect(Some("nope".into()), None).unwrap_err();
    assert_eq!(e.code, ErrorCode::Device);

    rig.transport
        .set_state("P1", socketsweep_transport::DeviceState::Unauthorized);
    let e = rig.app.connect(None, None).unwrap_err();
    assert_eq!(e.code, ErrorCode::Device);
    assert!(e.message.contains("unauthorized"), "{}", e.message);
    let devices = rig.app.list_devices().unwrap();
    assert!(!devices[0].usable);
    assert!(devices[0].hint.is_some());
}

#[test]
fn a_refusing_daemon_fails_the_connect_and_cleans_up() {
    let rig = Rig::new(FakeDaemon::new(|_, _| {}).with_greeting(Greeting::Refuse));
    let e = rig.app.connect(None, None).unwrap_err();
    assert_eq!(e.code, ErrorCode::Daemon);
    let log = rig.transport.shell_log("P1");
    assert!(log.last().unwrap().starts_with("rm -f"), "{log:?}");
}

#[test]
fn progress_shows_the_view_asked_for_by_focus_and_get_view_does_not_change_it() {
    let rig = Rig::new(phone());
    let app = &rig.app;
    app.connect(None, None).unwrap();
    app.start_scan(None).unwrap();
    let dcim = row(&app.get_view(None, None, None).unwrap(), "DCIM");

    // The row cap comes from `focus`.
    app.focus(Some(dcim), Some(0), None).unwrap();
    let progress = rig.wait_finished();
    let view = progress.view.unwrap();
    assert_eq!(view.node, dcim);
    assert!(view.rows.is_empty(), "{view:?}");

    // Reading a view, whatever its cap, leaves that alone: `delete` answers
    // with the focused folder under the same cap.
    let full = app.get_view(Some(dcim), Some(10), None).unwrap();
    assert_eq!(full.rows.len(), 1);
    let a = row(&app.get_view(None, None, None).unwrap(), "a.txt");
    let deleted = app.delete(a).unwrap();
    assert_eq!(deleted.view.node, dcim);
    assert!(deleted.view.rows.is_empty(), "{:?}", deleted.view);
}

#[test]
fn a_transport_that_cannot_start_fails_the_command_and_is_retried() {
    use std::sync::atomic::AtomicUsize;

    let attempts = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&attempts);
    let app = App::lazy(
        move || {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(TransportError::Refused("adb will not start".into()))
            } else {
                Ok(FakeTransport::new().with_device(FakeDevice::ready("P1")))
            }
        },
        AppConfig::default(),
        |_| {},
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        0,
        "nothing starts up front"
    );

    let e = app.list_devices().unwrap_err();
    assert_eq!(e.code, ErrorCode::Device);
    assert!(e.message.contains("adb will not start"), "{e}");

    assert_eq!(app.list_devices().unwrap().len(), 1);
    assert_eq!(app.list_devices().unwrap().len(), 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 2, "built once it worked");
    app.shutdown();
}

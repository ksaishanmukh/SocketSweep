use super::*;
use crate::testkit::{wait_for, FakeDaemon, Greeting};
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_transport::Transport;
use socketsweep_wire::{Outcome, TOKEN_LEN};
use std::sync::atomic::AtomicBool;

const TOKEN: Token = [7; TOKEN_LEN];
const SESSION: SessionId = [1; 16];

fn quick() -> ClientConfig {
    ClientConfig {
        ping_interval: Duration::from_millis(50),
        dead_after: Duration::from_millis(400),
        handshake_timeout: Duration::from_secs(5),
        tick: Duration::from_millis(10),
    }
}

/// Connect through `FakeTransport::open_tunnel` to `daemon`, which expects
/// `daemon_token`.
fn connect_with(
    daemon: &FakeDaemon,
    daemon_token: Token,
    config: ClientConfig,
) -> Result<Client, ClientError> {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    let d = daemon.clone();
    t.on_tunnel("P1", "sock", move |end| d.serve(end, daemon_token));
    let stream = t.open_tunnel("P1", "sock").unwrap();
    Client::connect(stream, TOKEN, SESSION, Caps::all(), config)
}

fn connect(daemon: &FakeDaemon) -> Client {
    connect_with(daemon, TOKEN, quick()).unwrap()
}

fn idle() -> FakeDaemon {
    FakeDaemon::new(|_, _| {})
}

fn finished(outcome: Outcome) -> ScanEvent {
    ScanEvent::Finished {
        outcome,
        totals: Totals::default(),
        elapsed_ms: 1,
    }
}

#[test]
fn the_handshake_is_accepted_with_the_right_token() {
    let client = connect(&idle());
    assert_eq!(client.welcome().workers, 4);
    assert_eq!(client.welcome().session, SESSION);
    assert!(client.ping(Duration::from_secs(5)).is_ok());
    client.close();
}

#[test]
fn a_wrong_token_is_refused_and_not_retryable() {
    let err = connect_with(&idle(), [9; TOKEN_LEN], quick())
        .err()
        .unwrap();
    assert_eq!(err, ClientError::Refused);
    assert!(!err.retryable());

    let err = connect_with(&idle().with_greeting(Greeting::Refuse), TOKEN, quick())
        .err()
        .unwrap();
    assert_eq!(err, ClientError::Refused);
}

#[test]
fn a_daemon_of_another_version_is_reported() {
    let err = connect_with(
        &idle().with_greeting(Greeting::WrongVersion(3)),
        TOKEN,
        quick(),
    )
    .err()
    .unwrap();
    assert_eq!(err, ClientError::WrongVersion { daemon: 3 });
}

#[test]
fn a_tunnel_nobody_listens_on_is_retryable() {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    let stream = t.open_tunnel("P1", "nobody").unwrap();
    let err = Client::connect(stream, TOKEN, SESSION, Caps::all(), quick())
        .err()
        .unwrap();
    assert!(err.retryable(), "{err:?}");
}

#[test]
fn scan_events_are_routed_by_request_id_alongside_other_replies() {
    let daemon = FakeDaemon::new(|frame, out| {
        if let ClientMsg::StartScan { root, .. } = &frame.msg {
            let id = frame.id;
            out.send(
                id,
                ServerMsg::Scan(vec![ScanEvent::Started {
                    root: root.clone(),
                    dev: 1,
                    workers: 4,
                }]),
            );
            // A frame for an id nobody asked about is dropped.
            out.send(9999, ServerMsg::DeleteProgress { items: 1 });
            out.send(SESSION_LEVEL, ServerMsg::Heartbeat);
            out.send(id, ServerMsg::Scan(vec![finished(Outcome::Complete)]));
        }
    });
    let client = connect(&daemon);
    let (id, rx) = client
        .start_scan(b"/sdcard".to_vec(), ScanOpts::default())
        .unwrap();
    assert_ne!(id, SESSION_LEVEL);

    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(&first, ServerMsg::Scan(e) if matches!(e[0], ScanEvent::Started { .. })));
    let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(&second, ServerMsg::Scan(e) if matches!(e[0], ScanEvent::Finished { .. })));
    // The route ends with Finished.
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_err());
    // And the connection is still good for other requests.
    assert!(client.ping(Duration::from_secs(5)).is_ok());
    client.close();
}

#[test]
fn cancel_names_the_scan_and_the_scan_still_finishes() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let daemon = FakeDaemon::new(move |frame, out| match &frame.msg {
        ClientMsg::StartScan { .. } => {
            let (id, out, flag) = (frame.id, out.clone(), Arc::clone(&flag));
            std::thread::spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    let progress = ScanEvent::Progress {
                        listed: 1,
                        known: 2,
                        files: 0,
                        disk: 0,
                        pending: 1,
                        alloc: [0, 0, 1],
                        stalled: vec![],
                    };
                    if !out.send(id, ServerMsg::Scan(vec![progress])) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                out.send(id, ServerMsg::Scan(vec![finished(Outcome::Cancelled)]));
            });
        }
        ClientMsg::Cancel { .. } => flag.store(true, Ordering::SeqCst),
        _ => {}
    });
    let client = connect(&daemon);
    let (id, rx) = client
        .start_scan(b"/sdcard".to_vec(), ScanOpts::default())
        .unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    client.cancel(id).unwrap();

    let outcome = loop {
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            ServerMsg::Scan(events) => {
                if let Some(ScanEvent::Finished { outcome, .. }) = events.last() {
                    break *outcome;
                }
            }
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(outcome, Outcome::Cancelled);
    assert!(daemon
        .messages()
        .contains(&ClientMsg::Cancel { target: id }));
    client.close();
}

#[test]
fn delete_reports_progress_and_returns_the_reply() {
    let daemon = FakeDaemon::new(|frame, out| {
        if let ClientMsg::Delete { path, .. } = &frame.msg {
            if path == b"/sdcard/nope" {
                out.send(
                    frame.id,
                    ServerMsg::Failed {
                        code: ErrCode::NotFound,
                        errno: Some(2),
                        message: "no such file".into(),
                    },
                );
                return;
            }
            out.send(frame.id, ServerMsg::DeleteProgress { items: 2 });
            out.send(
                frame.id,
                ServerMsg::Deleted {
                    items: 3,
                    totals: Totals {
                        apparent: 10,
                        ..Totals::default()
                    },
                },
            );
        }
    });
    let client = connect(&daemon);
    let mut seen = Vec::new();
    let (items, totals) = client
        .delete(b"/sdcard/x".to_vec(), Some((5, 6)), &mut |n| seen.push(n))
        .unwrap();
    assert_eq!((items, totals.apparent), (3, 10));
    assert_eq!(seen, [2]);
    assert_eq!(
        daemon.messages(),
        [ClientMsg::Delete {
            path: b"/sdcard/x".to_vec(),
            hint: Some((5, 6))
        }]
    );

    let err = client
        .delete(b"/sdcard/nope".to_vec(), None, &mut |_| {})
        .unwrap_err();
    assert!(matches!(
        err,
        ClientError::Failed {
            code: ErrCode::NotFound,
            ..
        }
    ));
    client.close();
}

#[test]
fn a_silent_daemon_is_declared_dead_and_waiters_are_released() {
    let daemon = idle().with_greeting(Greeting::AcceptThenSilent);
    let client = connect(&daemon);
    let (tx, rx) = mpsc::channel();
    client.set_on_close(Box::new(move |why| tx.send(why).unwrap()));

    // A delete the daemon never answers is released when the peer is dead.
    let c = client.clone();
    let waiter = std::thread::spawn(move || c.delete(b"/sdcard/x".to_vec(), None, &mut |_| {}));

    let why = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(why.contains("not answered"), "{why}");
    assert!(matches!(
        waiter.join().unwrap(),
        Err(ClientError::Closed(_))
    ));
    assert!(!client.is_open());
    // It pinged while idle before giving up.
    assert!(lock(&daemon.received)
        .iter()
        .any(|f| matches!(f.msg, ClientMsg::Ping { .. })));
    assert!(matches!(
        client.ping(Duration::from_secs(1)),
        Err(ClientError::Closed(_))
    ));
}

#[test]
fn a_daemon_that_answers_pings_stays_alive() {
    let client = connect(&idle());
    std::thread::sleep(Duration::from_millis(1000));
    assert!(client.is_open(), "{:?}", client.closed_reason());
    client.close();
}

#[test]
fn a_deliberate_close_does_not_report_a_lost_connection() {
    let client = connect(&idle());
    let (tx, rx) = mpsc::channel::<String>();
    client.set_on_close(Box::new(move |why| tx.send(why).unwrap()));
    client.close();
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    assert!(!client.is_open());
}

#[test]
fn the_phone_hanging_up_is_reported_once() {
    let daemon = FakeDaemon::new(|frame, _| {
        if matches!(frame.msg, ClientMsg::PrepareCopy { .. }) {
            panic!("simulated daemon crash");
        }
    });
    let client = connect(&daemon);
    let (tx, rx) = mpsc::channel();
    client.set_on_close(Box::new(move |why| tx.send(why).unwrap()));
    let (_, replies) = client.prepare_copy(b"/sdcard/x".to_vec()).unwrap();
    assert!(replies.recv_timeout(Duration::from_secs(5)).is_err());
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(wait_for(Duration::from_secs(1), || !client.is_open()));
    // Set after the fact: called at once.
    let (tx2, rx2) = mpsc::channel();
    client.set_on_close(Box::new(move |why| tx2.send(why).unwrap()));
    rx2.recv_timeout(Duration::from_secs(1)).unwrap();
}

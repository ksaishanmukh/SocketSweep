//! Requests routed by id: scans, cancel and delete.

use super::super::*;
use super::support::*;
use crate::testkit::FakeDaemon;
use socketsweep_wire::{ClientMsg, Outcome, ScanEvent, ScanOpts, ServerMsg, Totals, SESSION_LEVEL};
use std::sync::atomic::{AtomicBool, Ordering};

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

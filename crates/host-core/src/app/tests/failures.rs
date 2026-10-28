//! Failures: protocol violations, dead phones, bad arguments.

use super::super::*;
use super::phone::phone;
use super::rig::{row, Rig, RESOLVED};
use crate::testkit::{wait_for, FakeDaemon, Greeting};
use socketsweep_gui_contract::{Commands, ErrorCode, ScanOutcome};
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_wire::{ClientMsg, ScanEvent, ServerMsg, Totals};
use std::sync::atomic::Ordering;

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

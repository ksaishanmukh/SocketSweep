//! The whole path through the app: connect, scan, focus, delete, copy and
//! disconnect.

use std::path::Path;

use socketsweep_gui_contract::{self as gui, Commands, ErrorCode, Measure, NodeRef, ScanOutcome};
use socketsweep_wire::ClientMsg;

use super::phone::phone;
use super::rig::{row, Rig};
use crate::app::History;
use crate::app::HostEvent;

#[test]
fn connect_scan_focus_delete_copy_and_disconnect() {
    let rig = Rig::new(phone());

    connect(&rig);
    let (root, dcim, a) = scan_and_focus(&rig);
    queries_and_history(&rig, dcim);
    delete_a_file(&rig, root, dcim, a);
    copy_folder(&rig, dcim);
    rescan_and_disconnect(&rig, dcim);
}

fn connect(rig: &Rig) {
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
}

/// Scan and focus a folder; the root, that folder and the file `a.txt`.
fn scan_and_focus(rig: &Rig) -> (NodeRef, NodeRef, NodeRef) {
    let app = &rig.app;
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

    (started.root, dcim, a)
}

fn queries_and_history(rig: &Rig, dcim: NodeRef) {
    let app = &rig.app;
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
}

fn delete_a_file(rig: &Rig, root: NodeRef, dcim: NodeRef, a: NodeRef) {
    let app = &rig.app;
    assert_eq!(app.delete(root).unwrap_err().code, ErrorCode::Invalid);
    let deleted = app.delete(a).unwrap();
    assert_eq!(deleted.items, 1);
    assert_eq!(deleted.totals.size.apparent, 1000);
    assert_eq!(deleted.view.node, dcim);
    assert!(rig.daemon.messages().contains(&ClientMsg::Delete {
        path: b"/storage/emulated/0/a.txt".to_vec(),
        hint: None
    }));
    assert_eq!(app.delete(a).unwrap_err().code, ErrorCode::Removed);
}

fn copy_folder(rig: &Rig, dcim: NodeRef) {
    let app = &rig.app;
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
}

fn rescan_and_disconnect(rig: &Rig, dcim: NodeRef) {
    let app = &rig.app;
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

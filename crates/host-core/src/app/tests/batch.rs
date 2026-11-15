//! The overview, and deleting and copying several nodes at once.

use socketsweep_gui_contract::{Category, Commands, ErrorCode, NodeRef};
use socketsweep_wire::ClientMsg;

use super::android::{android, scanned};
use super::rig::{row, Rig, RESOLVED};
use crate::app::HostEvent;

/// The nodes of `Android/data` and of its three package folders.
fn packages(rig: &Rig) -> (NodeRef, [NodeRef; 3]) {
    let app = &rig.app;
    let android = row(
        &app.get_view(None, None, None, None, None).unwrap(),
        "Android",
    );
    let data = row(
        &app.get_view(Some(android), None, None, None, None).unwrap(),
        "data",
    );
    let view = app.get_view(Some(data), None, None, None, None).unwrap();
    (
        data,
        [
            row(&view, "com.a"),
            row(&view, "com.b"),
            row(&view, "com.c"),
        ],
    )
}

#[test]
fn the_overview_sums_up_the_scan() {
    let rig = Rig::new(android());
    scanned(&rig);
    let overview = rig.app.overview(None).unwrap();
    assert_eq!(overview.totals.files, 4);
    assert_eq!(overview.categories[0].category, Category::Video);
    assert_eq!(overview.biggest_files[0].name, "big.mp4");
    assert_eq!(overview.biggest_files[0].parent.as_deref(), Some("/sdcard"));
    assert_eq!(overview.biggest_files.len(), 4);
    assert_eq!(overview.apps.len(), 3);
    assert_eq!(overview.apps[0].package, "com.a");
    let unreadable: Vec<_> = overview
        .unreadable
        .iter()
        .map(|u| u.path.as_str())
        .collect();
    assert_eq!(unreadable, ["/sdcard/Broken"]);
    assert!(
        overview.since_last_scan.is_none(),
        "first scan of this phone"
    );

    // The next scan compares with the one just finished.
    rig.app.start_scan(None).unwrap();
    rig.wait_finished();
    let since = rig.app.overview(None).unwrap().since_last_scan.unwrap();
    assert_eq!(since.previous.files, 4);
    assert_eq!((since.disk_delta, since.files_delta), (0, 0));
}

#[test]
fn delete_many_skips_nested_nodes_and_reports_each_failure() {
    let rig = Rig::new(android());
    scanned(&rig);
    let (data, [a, b, c]) = packages(&rig);
    let inside_a = row(
        &rig.app.get_view(Some(a), None, None, None, None).unwrap(),
        "a.bin",
    );

    let done = rig.app.delete_many(vec![a, inside_a, b, c, a]).unwrap();
    assert_eq!(done.deleted, 2, "{done:?}");
    assert_eq!(done.items, 4);
    assert_eq!(done.size.apparent, 600);
    assert_eq!(done.failures.len(), 1);
    assert_eq!(done.failures[0].node, b);
    assert_eq!(done.failures[0].path, "/sdcard/Android/data/com.b");
    assert_eq!(done.failures[0].error.code, ErrorCode::Daemon);
    let deletes = rig
        .daemon
        .messages()
        .into_iter()
        .filter(|m| matches!(m, ClientMsg::Delete { .. }))
        .count();
    assert_eq!(
        deletes, 3,
        "the file inside com.a and the repeat are not sent"
    );

    let left = rig
        .app
        .get_view(Some(data), None, None, None, None)
        .unwrap();
    let names: Vec<_> = left.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["com.b"]);
    assert_eq!(done.totals.files, 2);

    // The single delete is the same path, failing as a whole.
    assert_eq!(rig.app.delete(b).unwrap_err().code, ErrorCode::Daemon);
}

#[test]
fn delete_many_refuses_everything_for_one_bad_node() {
    let rig = Rig::new(android());
    scanned(&rig);
    let (_, [a, ..]) = packages(&rig);
    let stale = NodeRef {
        session: a.session + 1,
        id: a.id,
    };
    let root = rig.app.get_view(None, None, None, None, None).unwrap().node;
    assert_eq!(
        rig.app.delete_many(vec![a, stale]).unwrap_err().code,
        ErrorCode::Stale
    );
    assert_eq!(
        rig.app.delete_many(vec![a, root]).unwrap_err().code,
        ErrorCode::Invalid
    );
    assert!(!rig
        .daemon
        .messages()
        .iter()
        .any(|m| matches!(m, ClientMsg::Delete { .. })));
}

#[test]
fn copy_many_is_one_copy_into_the_destination() {
    let rig = Rig::new(android());
    scanned(&rig);
    let (_, [a, b, _]) = packages(&rig);
    let inside_a = row(
        &rig.app.get_view(Some(a), None, None, None, None).unwrap(),
        "a.bin",
    );
    for pkg in ["com.a", "com.b"] {
        let mut remote = RESOLVED.to_vec();
        remote.extend_from_slice(format!("/Android/data/{pkg}/f").as_bytes());
        rig.transport
            .put_file("P1", &remote, pkg.as_bytes()[..4].to_vec(), 0o644);
    }
    let dest = rig.dir.join("Out");
    let done = rig
        .app
        .copy_many(vec![a, inside_a, b], Some(dest.display().to_string()))
        .unwrap();
    assert_eq!(done.destination, dest.display().to_string());
    assert_eq!((done.files, done.bytes), (2, 8));
    assert_eq!(
        std::fs::read(dest.join("com.a").join("f")).unwrap(),
        b"com."
    );
    assert_eq!(
        std::fs::read(dest.join("com.b").join("f")).unwrap(),
        b"com."
    );

    let progress = rig.wait_event(|e| matches!(e, HostEvent::CopyProgress(p) if p.totals_known));
    let HostEvent::CopyProgress(p) = progress else {
        unreachable!()
    };
    assert_eq!(p.nodes, [a, inside_a, b]);
    assert_eq!((p.files_total, p.bytes_total), (2, 8));
}

//! The pending-delete queue through the app: staging, what views and the
//! overview show meanwhile, and the one delete that runs it.

use socketsweep_gui_contract::{ErrorCode, Measure, NodeRef, Row, ViewRequest};
use socketsweep_wire::ClientMsg;

use super::android::{android, scanned};
use super::rig::{row, Rig};
use crate::app::HostEvent;

/// The `Android/data` package folders `com.a`, `com.b`, `com.c`, and `a.bin`
/// inside `com.a`.
struct Nodes {
    android: NodeRef,
    a: NodeRef,
    b: NodeRef,
    c: NodeRef,
    a_bin: NodeRef,
}

fn view_of(rig: &Rig, node: Option<NodeRef>) -> socketsweep_gui_contract::View {
    rig.app
        .get_view(ViewRequest {
            node,
            ..Default::default()
        })
        .unwrap()
}

fn nodes(rig: &Rig) -> Nodes {
    let android = row(&view_of(rig, None), "Android");
    let data = row(&view_of(rig, Some(android)), "data");
    let packages = view_of(rig, Some(data));
    let a = row(&packages, "com.a");
    Nodes {
        android,
        a,
        b: row(&packages, "com.b"),
        c: row(&packages, "com.c"),
        a_bin: row(&view_of(rig, Some(a)), "a.bin"),
    }
}

fn staged_row<'a>(view: &'a socketsweep_gui_contract::View, name: &str) -> &'a Row {
    view.rows.iter().find(|r| r.name == name).unwrap()
}

fn wait_staged_items(rig: &Rig, items: u32) -> socketsweep_gui_contract::StagedChanged {
    match rig.wait_event(|e| matches!(e, HostEvent::StagedChanged(c) if c.totals.items == items)) {
        HostEvent::StagedChanged(c) => c,
        _ => unreachable!(),
    }
}

#[test]
fn staging_reports_the_queue_and_announces_each_change() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);

    let totals = rig.app.stage_delete(vec![n.a, n.c]).unwrap();
    assert_eq!(totals.items, 2);
    assert_eq!(totals.size.apparent, 400);
    assert_eq!(totals.size.disk, 1024, "a block each");
    assert_eq!(totals.files, 2);
    assert_eq!(totals.measure, Measure::Final);
    let changed = wait_staged_items(&rig, 2);
    assert_eq!(changed.session, n.a.session);
    assert_eq!(changed.totals, totals);

    let summary = rig.app.staged_summary().unwrap();
    let paths: Vec<_> = summary.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths,
        ["/sdcard/Android/data/com.a", "/sdcard/Android/data/com.c"]
    );
    assert_eq!(summary.totals, totals);

    assert_eq!(rig.app.unstage_delete(vec![n.a]).unwrap().items, 1);
    wait_staged_items(&rig, 1);
    assert_eq!(rig.app.clear_staged().unwrap().items, 0);
    wait_staged_items(&rig, 0);
    assert!(rig.app.staged_summary().unwrap().items.is_empty());
    assert!(
        !rig.daemon
            .messages()
            .iter()
            .any(|m| matches!(m, ClientMsg::Delete { .. })),
        "staging deletes nothing"
    );
}

#[test]
fn a_bad_node_refuses_the_whole_staging() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);
    let stale = NodeRef {
        session: n.a.session + 1,
        id: n.a.id,
    };
    let root = view_of(&rig, None).node;
    assert_eq!(
        rig.app.stage_delete(vec![n.a, stale]).unwrap_err().code,
        ErrorCode::Stale
    );
    assert_eq!(
        rig.app.stage_delete(vec![n.a, root]).unwrap_err().code,
        ErrorCode::Invalid
    );
    assert_eq!(rig.app.staged_summary().unwrap().totals.items, 0);
}

#[test]
fn a_node_staged_inside_a_staged_folder_is_not_counted_twice() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);

    rig.app.stage_delete(vec![n.a_bin]).unwrap();
    let totals = rig.app.stage_delete(vec![n.a]).unwrap();
    assert_eq!((totals.items, totals.size.apparent), (1, 300));
    let summary = rig.app.staged_summary().unwrap();
    assert_eq!(summary.items.len(), 1);
    assert_eq!(summary.items[0].node, n.a);
}

#[test]
fn views_show_what_the_queue_would_leave() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);
    rig.app.stage_delete(vec![n.a, n.c]).unwrap();

    let root = view_of(&rig, None);
    let android = staged_row(&root, "Android");
    assert_eq!(android.size.apparent, 600);
    assert_eq!(android.projected.apparent, 200, "only com.b stays");
    assert!(!android.staged);
    assert_eq!(root.projected.apparent, root.size.apparent - 400);
    let big = staged_row(&root, "big.mp4");
    assert_eq!(big.projected, big.size);

    let data = row(&view_of(&rig, Some(n.android)), "data");
    let packages = view_of(&rig, Some(data));
    let a = staged_row(&packages, "com.a");
    assert!(a.staged);
    assert_eq!(a.projected.disk, 0);
    assert!(!staged_row(&packages, "com.b").staged);

    let map = rig.app.get_treemap(None, Some(3), None).unwrap();
    assert_eq!(map.projected.disk, map.size.disk - 1024);
}

#[test]
fn the_overview_says_what_deleting_would_free() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);

    let before = rig.app.overview(None).unwrap();
    assert_eq!(before.staged.items, 0);
    assert_eq!(before.projected, before.totals.size);

    rig.app.stage_delete(vec![n.a, n.c]).unwrap();
    let after = rig.app.overview(None).unwrap();
    assert_eq!(after.staged.items, 2);
    assert_eq!(after.staged.size.disk, 1024);
    assert_eq!(after.projected.disk, after.totals.size.disk - 1024);
    assert_eq!(after.projected.apparent, after.totals.size.apparent - 400);
    assert!(after
        .biggest_files
        .iter()
        .any(|f| f.name == "a.bin" && f.staged));
}

#[test]
fn a_rescan_discards_the_queue() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);
    rig.app.stage_delete(vec![n.a]).unwrap();

    rig.app.start_scan(None).unwrap();
    rig.wait_finished();
    assert!(rig.app.staged_summary().unwrap().items.is_empty());
    assert_eq!(rig.app.overview(None).unwrap().staged.items, 0);
    // Nodes of the old tree name nothing in the new one.
    assert_eq!(
        rig.app.stage_delete(vec![n.a]).unwrap_err().code,
        ErrorCode::Stale
    );
    assert_eq!(rig.app.unstage_delete(vec![n.a]).unwrap().items, 0);
}

#[test]
fn deleting_leaves_the_queue_with_the_tree() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);
    rig.app.stage_delete(vec![n.a_bin, n.c]).unwrap();
    wait_staged_items(&rig, 2);

    // Deleted some other way, the node leaves the queue and the GUI is told.
    rig.app.delete_many(vec![n.c]).unwrap();
    wait_staged_items(&rig, 1);
    assert_eq!(rig.app.staged_summary().unwrap().items[0].node, n.a_bin);
}

#[test]
fn delete_staged_deletes_the_outermost_nodes_and_clears_the_queue() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);
    rig.app.stage_delete(vec![n.a_bin, n.a, n.c]).unwrap();
    wait_staged_items(&rig, 2);

    let done = rig.app.delete_staged().unwrap();
    assert_eq!((done.deleted, done.failures.len()), (2, 0), "{done:?}");
    let deleted: Vec<_> = rig
        .daemon
        .messages()
        .into_iter()
        .filter_map(|m| match m {
            ClientMsg::Delete { path, .. } => Some(String::from_utf8(path).unwrap()),
            _ => None,
        })
        .collect();
    assert_eq!(deleted.len(), 2, "a.bin goes with com.a: {deleted:?}");
    assert!(deleted.iter().any(|p| p.ends_with("/com.a")));
    assert!(deleted.iter().any(|p| p.ends_with("/com.c")));

    wait_staged_items(&rig, 0);
    assert!(rig.app.staged_summary().unwrap().items.is_empty());
    let data = row(&view_of(&rig, Some(n.android)), "data");
    let left: Vec<_> = view_of(&rig, Some(data))
        .rows
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(left, ["com.b"]);
}

#[test]
fn what_the_phone_refuses_stays_queued() {
    let rig = Rig::new(android());
    scanned(&rig);
    let n = nodes(&rig);
    rig.app.stage_delete(vec![n.b, n.c]).unwrap();

    let done = rig.app.delete_staged().unwrap();
    assert_eq!(done.deleted, 1);
    assert_eq!(done.failures[0].node, n.b);
    let left = rig.app.staged_summary().unwrap();
    assert_eq!(left.items.len(), 1);
    assert_eq!(left.items[0].node, n.b);
}

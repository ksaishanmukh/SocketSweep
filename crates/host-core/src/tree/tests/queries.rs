//! Node references, paths and the queries over the tree.

use super::super::*;
use super::support::*;
use socketsweep_gui_contract::{NodeRef, Row};
use socketsweep_wire::{Entry, ScanEvent, Stat};

#[test]
fn stale_and_removed_node_refs_are_refused() {
    let mut t = tree();
    apply(&mut t, &[listing(0, vec![file("f", 1)])]);
    let f = child(&t, ROOT, "f");
    assert_eq!(
        t.resolve(NodeRef {
            session: S + 1,
            id: f
        }),
        Err(TreeError::Stale)
    );
    assert_eq!(t.resolve(t.node_ref(f)), Ok(f));
    t.remove(f).unwrap();
    assert_eq!(t.resolve(t.node_ref(f)), Err(TreeError::Removed));
    assert_eq!(
        t.delete_target(t.node_ref(ROOT)),
        Err(TreeError::RootNotDeletable)
    );
}

#[test]
fn paths_are_byte_exact_and_display_paths_use_the_label() {
    let mut t = tree();
    let odd = Entry {
        name: b"\xFFodd".to_vec(),
        ..file("", 1)
    };
    apply(
        &mut t,
        &[listing(0, vec![dir("DCIM", 1)]), listing(1, vec![odd])],
    );
    let dcim = child(&t, ROOT, "DCIM");
    let odd = t.view(dcim, 10, SizeMetric::Disk).unwrap().rows[0].node.id;
    assert_eq!(t.path_of(odd).unwrap(), b"/storage/emulated/0/DCIM/\xFFodd");
    assert_eq!(t.display_path(dcim), "/sdcard/DCIM");
    let crumbs = t.breadcrumbs(odd).unwrap();
    let names: Vec<_> = crumbs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["sdcard", "DCIM", "\u{FFFD}odd"]);
}

#[test]
fn files_map_to_their_folder_for_focus() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1)]),
            listing(1, vec![file("f", 1)]),
        ],
    );
    let a = child(&t, ROOT, "A");
    let f = child(&t, a, "f");
    assert_eq!(t.focus_dir(f).unwrap(), (a, 1));
    assert_eq!(t.focus_dir(a).unwrap(), (a, 1));
    assert_eq!(t.focus_dir(ROOT).unwrap(), (ROOT, ROOT_DIR));
}

#[test]
fn queries_sort_by_the_requested_metric() {
    let mut t = tree();
    // "sparse": large apparent, little disk. "dense": the reverse.
    let sparse = Entry {
        stat: Some(Stat {
            apparent: 1_000_000,
            blocks: 1,
            mtime: 0,
        }),
        ..file("sparse.mp4", 0)
    };
    let dense = Entry {
        stat: Some(Stat {
            apparent: 10,
            blocks: 100,
            mtime: 0,
        }),
        ..file("dense.jpg", 0)
    };
    apply(&mut t, &[listing(0, vec![sparse, dense])]);

    let names = |rows: Vec<Row>| rows.into_iter().map(|r| r.name).collect::<Vec<_>>();
    assert_eq!(
        names(t.view(ROOT, 10, SizeMetric::Disk).unwrap().rows),
        ["dense.jpg", "sparse.mp4"]
    );
    assert_eq!(
        names(t.view(ROOT, 10, SizeMetric::Apparent).unwrap().rows),
        ["sparse.mp4", "dense.jpg"]
    );
    assert_eq!(
        names(t.largest_files(1, SizeMetric::Apparent)),
        ["sparse.mp4"]
    );
    let view = t.view(ROOT, 1, SizeMetric::Disk).unwrap();
    assert_eq!(view.hidden, 1);

    let groups = t.type_breakdown(SizeMetric::Disk);
    assert_eq!(groups[0].label, "Photos");
    let groups = t.type_breakdown(SizeMetric::Apparent);
    assert_eq!(groups[0].label, "Video");

    let hits = t.search("SPARSE", 10, SizeMetric::Disk);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].parent.as_deref(), Some("/sdcard"));
    assert_eq!(hits[0].parent_node, Some(t.node_ref(ROOT)));
}

#[test]
fn apps_are_summed_across_areas() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("Android", 1)]),
            listing(1, vec![dir("data", 2), dir("obb", 3)]),
            listing(2, vec![dir("com.game", 4), dir("com.small", 5)]),
            listing(3, vec![dir("com.game", 6)]),
            listing(4, vec![file("save", 100)]),
            listing(5, vec![file("x", 1)]),
            listing(6, vec![file("assets.obb", 5000)]),
        ],
    );
    let apps = t.app_breakdown(10, SizeMetric::Apparent);
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0].package, "com.game");
    assert_eq!(apps[0].size.apparent, 5100);
    assert_eq!(apps[0].files, 2);
    // Points at the larger of its folders, the obb one.
    let obb = child(&t, child(&t, ROOT, "Android"), "obb");
    assert_eq!(apps[0].node, t.node_ref(child(&t, obb, "com.game")));
    assert_eq!(t.app_breakdown(1, SizeMetric::Apparent).len(), 1);
}

#[test]
fn the_treemap_is_bounded_and_skips_empty_tiles() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), file("empty", 0)]),
            listing(1, vec![dir("B", 2)]),
            listing(2, vec![file("x", 600)]),
        ],
    );
    let map = t.treemap(ROOT, 1, SizeMetric::Apparent).unwrap();
    assert_eq!(map.children.len(), 1);
    assert!(map.children[0].children.is_empty());
    let map = t.treemap(ROOT, 9, SizeMetric::Apparent).unwrap();
    assert_eq!(map.children[0].children[0].children[0].name, "x");
}

#[test]
fn an_unreadable_folder_is_flagged() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1)]),
            ScanEvent::DirFailed {
                dir: 1,
                op: socketsweep_wire::FsOp::Open,
                errno: 13,
            },
            ScanEvent::SubtreeDone {
                dir: 1,
                totals: Totals {
                    errors: 1,
                    flags: Totals::UNREADABLE,
                    ..Totals::default()
                },
            },
        ],
    );
    let view = t.view(ROOT, 10, SizeMetric::Disk).unwrap();
    assert!(view.rows[0].unreadable);
    assert!(view.unreadable);
    assert_eq!(view.rows[0].measure, Measure::Final);
}

#[test]
fn nothing_is_accepted_after_finished() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![]),
            done(0, 0, 0, 0),
            ScanEvent::Finished {
                outcome: Outcome::Complete,
                totals: Totals::default(),
                elapsed_ms: 1,
            },
        ],
    );
    assert_eq!(t.finished(), Some(Outcome::Complete));
    assert!(t.ingest(&listing(0, vec![])).is_err());
}

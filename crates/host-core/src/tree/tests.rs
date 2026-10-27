use super::*;
use socketsweep_wire::BLOCK_SIZE;

const S: SessionId = 7;

fn file(name: &str, apparent: u64) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: Kind::File,
        stat: Some(stat(apparent)),
        id: None,
    }
}

/// A file whose stat is deferred.
fn bare(name: &str) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: Kind::File,
        stat: None,
        id: None,
    }
}

fn dir(name: &str, id: DirId) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: Kind::Dir,
        stat: Some(Stat {
            apparent: 0,
            blocks: 0,
            mtime: 0,
        }),
        id: Some(id),
    }
}

/// `apparent` bytes using whole 512-byte blocks.
fn stat(apparent: u64) -> Stat {
    Stat {
        apparent,
        blocks: apparent.div_ceil(BLOCK_SIZE),
        mtime: 0,
    }
}

fn listing(d: DirId, entries: Vec<Entry>) -> ScanEvent {
    ScanEvent::Listing {
        dir: d,
        part: 0,
        last: true,
        entries,
    }
}

fn done(d: DirId, apparent: u64, files: u32, dirs: u32) -> ScanEvent {
    ScanEvent::SubtreeDone {
        dir: d,
        totals: Totals {
            apparent,
            disk: apparent,
            files,
            dirs,
            ..Totals::default()
        },
    }
}

fn tree() -> Tree {
    Tree::new(S, b"/storage/emulated/0", "/sdcard")
}

fn apply(t: &mut Tree, events: &[ScanEvent]) {
    for e in events {
        t.ingest(e).unwrap_or_else(|err| panic!("{e:?}: {err}"));
    }
}

fn child(t: &Tree, parent: NodeId, name: &str) -> NodeId {
    t.view(parent, 1000, SizeMetric::Apparent)
        .unwrap()
        .rows
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .node
        .id
}

#[test]
fn measures_move_from_unknown_to_partial_to_final() {
    let mut t = tree();
    assert_eq!(t.root_measure(), Measure::Unknown);

    apply(&mut t, &[listing(0, vec![dir("A", 1), bare("x.jpg")])]);
    assert_eq!(t.root_measure(), Measure::Partial);
    let a = child(&t, ROOT, "A");
    let x = child(&t, ROOT, "x.jpg");
    assert_eq!(t.measure(a), Some(Measure::Unknown));
    assert_eq!(
        t.measure(x),
        Some(Measure::Unknown),
        "a file is unknown until its stat"
    );

    apply(
        &mut t,
        &[ScanEvent::Stats {
            dir: 0,
            start: 1,
            stats: vec![Some(stat(100))],
        }],
    );
    assert_eq!(t.measure(x), Some(Measure::Final));
    assert_eq!(t.size(x).unwrap().apparent, 100);
    assert_eq!(t.totals().size.apparent, 100);

    apply(&mut t, &[listing(1, vec![file("y", 50)])]);
    assert_eq!(t.measure(a), Some(Measure::Partial));
    assert_eq!(
        t.totals().size.apparent,
        150,
        "partial sums climb to the root"
    );

    apply(&mut t, &[done(1, 50, 1, 0)]);
    assert_eq!(t.measure(a), Some(Measure::Final));
    assert_eq!(t.root_measure(), Measure::Partial);
    apply(&mut t, &[done(0, 150, 2, 1)]);
    assert_eq!(t.root_measure(), Measure::Final);
}

#[test]
fn subtree_done_applies_the_daemons_figure_up_the_chain() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1)]),
            listing(1, vec![dir("B", 2), file("a", 10)]),
            listing(2, vec![file("b", 20)]),
        ],
    );
    assert_eq!(t.totals().size.apparent, 30);

    // The daemon counted more than the host saw (e.g. hardlink rules); its
    // figure wins, and the difference reaches every ancestor.
    apply(&mut t, &[done(2, 25, 1, 0)]);
    let a = child(&t, ROOT, "A");
    let b = child(&t, a, "B");
    assert_eq!(t.size(b).unwrap().apparent, 25);
    assert_eq!(t.size(a).unwrap().apparent, 35);
    assert_eq!(t.totals().size.apparent, 35);

    // And less.
    apply(&mut t, &[done(1, 5, 2, 1)]);
    assert_eq!(t.size(a).unwrap().apparent, 5);
    assert_eq!(t.totals().size.apparent, 5);
    assert_eq!(t.totals().files, 2);
}

#[test]
fn listing_parts_and_stats_by_position_across_parts() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            ScanEvent::Listing {
                dir: 0,
                part: 0,
                last: false,
                entries: vec![bare("a"), bare("b")],
            },
            ScanEvent::Listing {
                dir: 0,
                part: 1,
                last: true,
                entries: vec![bare("c")],
            },
            ScanEvent::Stats {
                dir: 0,
                start: 1,
                stats: vec![Some(stat(7)), None],
            },
        ],
    );
    assert_eq!(t.size(child(&t, ROOT, "b")).unwrap().apparent, 7);
    assert_eq!(t.measure(child(&t, ROOT, "a")), Some(Measure::Unknown));
    assert_eq!(t.measure(child(&t, ROOT, "c")), Some(Measure::Final));
    assert_eq!(t.totals().errors, 1, "a failed stat is an error");

    let err = t
        .ingest(&ScanEvent::Stats {
            dir: 0,
            start: 2,
            stats: vec![Some(stat(1)), Some(stat(1))],
        })
        .unwrap_err();
    assert!(matches!(err, TreeError::Protocol(_)), "{err:?}");
}

#[test]
fn interleaved_subtrees_out_of_order_need_no_reorder_buffer() {
    let mut t = tree();
    // Two subtrees progressing in an interleaved order, as parallel workers
    // produce: legal under G1-G3.
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), dir("B", 2)]),
            listing(2, vec![dir("B1", 3), file("b", 4)]),
            listing(1, vec![file("a", 1)]),
            listing(3, vec![file("bb", 8)]),
            done(1, 1, 1, 0),
            done(3, 8, 1, 0),
            done(2, 12, 2, 1),
            done(0, 13, 3, 3),
        ],
    );
    assert_eq!(t.totals().size.apparent, 13);
    assert_eq!(t.totals().dirs, 3);
    assert_eq!(t.root_measure(), Measure::Final);
    let b = child(&t, ROOT, "B");
    assert_eq!(t.view(b, 10, SizeMetric::Disk).unwrap().rows.len(), 2);
}

#[test]
fn an_unknown_dir_id_is_a_protocol_error() {
    let mut t = tree();
    for event in [
        listing(5, vec![]),
        done(5, 0, 0, 0),
        ScanEvent::Stats {
            dir: 9,
            start: 0,
            stats: vec![],
        },
        ScanEvent::Pruned { dir: 3 },
        ScanEvent::FocusState {
            seq: 1,
            dir: 4,
            settled: false,
        },
    ] {
        assert!(
            matches!(t.ingest(&event), Err(TreeError::Protocol(_))),
            "{event:?} should violate G1"
        );
    }
}

#[test]
fn g2_and_g3_violations_are_protocol_errors() {
    let mut t = tree();
    apply(&mut t, &[listing(0, vec![dir("A", 1)])]);
    // Listed twice.
    assert!(t.ingest(&listing(0, vec![])).is_err());
    // A parent finished before its child.
    assert!(t.ingest(&done(0, 0, 0, 1)).is_err());
    // An id introduced twice.
    assert!(t.ingest(&listing(1, vec![dir("again", 1)])).is_err());

    let mut t = tree();
    apply(&mut t, &[listing(0, vec![]), done(0, 0, 0, 0)]);
    assert!(t.ingest(&done(0, 0, 0, 0)).is_err(), "finished twice");

    let mut t = tree();
    let wrong_part = ScanEvent::Listing {
        dir: 0,
        part: 1,
        last: true,
        entries: vec![],
    };
    assert!(t.ingest(&wrong_part).is_err());
}

#[test]
fn pruning_removes_the_subtree_and_its_totals() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), file("keep", 5)]),
            listing(1, vec![dir("A1", 2), file("a", 10)]),
            listing(2, vec![file("deep", 20)]),
        ],
    );
    assert_eq!(t.totals().size.apparent, 35);
    let a = child(&t, ROOT, "A");

    apply(&mut t, &[ScanEvent::Pruned { dir: 1 }]);
    assert!(!t.exists(a));
    assert_eq!(t.totals().size.apparent, 5);
    assert_eq!(t.totals().files, 1);
    assert_eq!(t.totals().dirs, 0);
    assert!(t.search("deep", 10, SizeMetric::Disk).is_empty());

    // Events about directories inside it that were already on their way are
    // dropped, not violations, and the parent can now finish.
    apply(
        &mut t,
        &[
            done(2, 20, 1, 0),
            ScanEvent::Pruned { dir: 1 },
            done(0, 5, 1, 0),
        ],
    );
    assert_eq!(t.root_measure(), Measure::Final);
}

#[test]
fn a_local_remove_then_a_late_prune_counts_once() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), file("keep", 5)]),
            listing(1, vec![file("a", 10)]),
        ],
    );
    let a = child(&t, ROOT, "A");
    t.remove(a).unwrap();
    assert_eq!(t.totals().size.apparent, 5);
    // A listing for a directory introduced inside the removed subtree.
    apply(&mut t, &[ScanEvent::Pruned { dir: 1 }]);
    assert_eq!(t.totals().size.apparent, 5);
    apply(&mut t, &[done(0, 5, 1, 0)]);
    assert_eq!(t.nearest_live(a), ROOT);
    assert_eq!(t.remove(ROOT), Err(TreeError::RootNotDeletable));
}

#[test]
fn a_removed_directory_still_introduces_its_children() {
    let mut t = tree();
    apply(&mut t, &[listing(0, vec![dir("A", 1)])]);
    t.remove(child(&t, ROOT, "A")).unwrap();
    apply(
        &mut t,
        &[
            listing(1, vec![dir("A1", 2)]),
            listing(2, vec![file("x", 1)]),
            done(2, 1, 1, 0),
            done(1, 1, 1, 1),
            ScanEvent::Pruned { dir: 1 },
            done(0, 0, 0, 0),
        ],
    );
    assert_eq!(t.totals().size.apparent, 0);
}

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

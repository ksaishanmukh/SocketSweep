//! Applying scan events: measures, totals, pruning and protocol errors.

use super::super::*;
use super::support::*;
use socketsweep_wire::ScanEvent;

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

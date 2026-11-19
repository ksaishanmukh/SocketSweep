//! The pending-delete queue: what it counts, and how it changes sizes.

use super::super::*;
use super::support::*;
use socketsweep_gui_contract::{NodeRef, Row};
use socketsweep_wire::ScanEvent;

const BY: SizeMetric = SizeMetric::Apparent;

/// Disk usage of `apparent` bytes in whole 512-byte blocks.
fn disk(apparent: u64) -> u64 {
    apparent.div_ceil(512) * 512
}

/// `/sdcard` = { Music/ { a.mp3 500, b.flac 300 }, clip.mp4 400 }, every
/// folder finished.
fn sample() -> Tree {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("Music", 1), file("clip.mp4", 400)]),
            listing(1, vec![file("a.mp3", 500), file("b.flac", 300)]),
            done(1, 800, 2, 0),
            done(0, 1200, 3, 1),
        ],
    );
    t
}

struct Ids {
    music: NodeId,
    a: NodeId,
    clip: NodeId,
}

fn ids(t: &Tree) -> Ids {
    let music = child(t, ROOT, "Music");
    Ids {
        music,
        a: child(t, music, "a.mp3"),
        clip: child(t, ROOT, "clip.mp4"),
    }
}

fn row_of<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter().find(|r| r.name == name).unwrap()
}

#[test]
fn staging_and_unstaging_change_only_the_queue() {
    let mut t = sample();
    let ids = ids(&t);
    assert!(t.stage(&[ids.clip, ids.a]));
    assert!(!t.stage(&[ids.clip]), "already queued");

    let totals = t.staged_totals();
    assert_eq!(totals.items, 2);
    assert_eq!(totals.size.apparent, 900);
    assert_eq!(totals.files, 2);
    assert_eq!(totals.measure, Measure::Final);
    let summary = t.staged_summary();
    let names: Vec<_> = summary.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["a.mp3", "clip.mp4"], "largest first");
    assert_eq!(summary.items[0].path, "/sdcard/Music/a.mp3");

    assert!(t.unstage(&[ids.a, ids.music]));
    assert_eq!(t.staged_totals().items, 1);
    assert!(t.clear_staged());
    assert!(!t.clear_staged());
    assert_eq!(t.staged_totals(), Default::default());
    assert!(t.staged_summary().items.is_empty());
    // The tree itself is untouched.
    assert_eq!(t.totals().size.apparent, 1200);
}

#[test]
fn the_root_and_foreign_nodes_cannot_be_staged() {
    let mut t = sample();
    assert_eq!(
        t.resolve_deletable(t.node_ref(ROOT)),
        Err(TreeError::RootNotDeletable)
    );
    let foreign = NodeRef {
        session: S + 1,
        id: 1,
    };
    assert_eq!(t.resolve_deletable(foreign), Err(TreeError::Stale));
    let ids = ids(&t);
    t.remove(ids.clip).unwrap();
    assert_eq!(
        t.resolve_deletable(t.node_ref(ids.clip)),
        Err(TreeError::Removed)
    );
}

#[test]
fn a_node_inside_a_staged_folder_is_not_counted_twice() {
    let mut t = sample();
    let ids = ids(&t);
    t.stage(&[ids.a, ids.music]);

    let totals = t.staged_totals();
    assert_eq!((totals.items, totals.size.apparent), (1, 800));
    assert_eq!(totals.files, 2);
    assert_eq!(t.staged_outermost(), [ids.music]);
    assert_eq!(t.projected_total().apparent, 400);

    // Unstaging the folder leaves the inner pick queued.
    t.unstage(&[ids.music]);
    assert_eq!(t.staged_outermost(), [ids.a]);
    assert_eq!(t.staged_totals().size.apparent, 500);
}

#[test]
fn ancestors_show_their_size_without_what_is_staged_inside() {
    let mut t = sample();
    let ids = ids(&t);
    t.stage(&[ids.a]);

    let root = t.view(ROOT, 10, BY).unwrap();
    assert_eq!(root.size.apparent, 1200);
    assert_eq!(root.projected.apparent, 700);
    assert!(!root.staged);
    let music = row_of(&root.rows, "Music");
    assert_eq!((music.size.apparent, music.projected.apparent), (800, 300));
    assert!(!music.staged);
    let clip = row_of(&root.rows, "clip.mp4");
    assert_eq!(clip.projected, clip.size);

    let inside = t.view(ids.music, 10, BY).unwrap();
    let a = row_of(&inside.rows, "a.mp3");
    assert!(a.staged);
    assert_eq!(a.projected, Size::default());
    let b = row_of(&inside.rows, "b.flac");
    assert_eq!((b.staged, b.projected), (false, b.size));

    let map = t.treemap(ROOT, 2, BY).unwrap();
    assert_eq!(map.projected.apparent, 700);
    let music = map.children.iter().find(|c| c.name == "Music").unwrap();
    assert_eq!(music.projected.apparent, 300);
    assert!(music.children.iter().any(|c| c.name == "a.mp3" && c.staged));
}

#[test]
fn everything_under_a_staged_folder_reads_as_staged() {
    let mut t = sample();
    let ids = ids(&t);
    t.stage(&[ids.music]);

    let root = t.view(ROOT, 10, BY).unwrap();
    let music = row_of(&root.rows, "Music");
    assert!(music.staged);
    assert_eq!(music.projected, Size::default());
    assert_eq!(root.projected.apparent, 400);

    let inside = t.view(ids.music, 10, BY).unwrap();
    assert!(inside.staged && inside.rows.iter().all(|r| r.staged));
    let hits = t.search("flac", 10, BY);
    assert!(hits[0].staged, "search hits are flagged too");
}

#[test]
fn nothing_is_flagged_while_the_queue_is_empty() {
    let t = sample();
    let root = t.view(ROOT, 10, BY).unwrap();
    assert!(!root.staged && root.projected == root.size);
    assert!(root.rows.iter().all(|r| !r.staged && r.projected == r.size));
    assert_eq!(t.projected_total(), t.totals().size);
}

#[test]
fn both_metrics_are_projected() {
    let mut t = sample();
    let ids = ids(&t);
    t.stage(&[ids.a]);
    let root = t.view(ROOT, 10, BY).unwrap();
    assert_eq!(root.projected.disk, root.size.disk - disk(500));
    assert_eq!(t.staged_totals().size.disk, disk(500));
}

#[test]
fn an_unfinished_staged_folder_keeps_the_total_partial() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("Music", 1)]),
            listing(1, vec![file("a.mp3", 500)]),
        ],
    );
    let music = child(&t, ROOT, "Music");
    t.stage(&[music]);
    let totals = t.staged_totals();
    assert_eq!(totals.measure, Measure::Partial);
    assert_eq!(totals.size.apparent, 500, "what is known so far");
    let root = t.view(ROOT, 10, BY).unwrap();
    assert_eq!(root.measure, Measure::Partial);
}

#[test]
fn staged_nodes_leave_the_queue_with_the_tree() {
    let mut t = sample();
    let ids = ids(&t);
    t.stage(&[ids.a, ids.clip]);
    assert!(!t.take_staged_dropped(), "staging is not a drop");

    // The scan prunes Music, as when it is deleted during a scan.
    t.ingest(&ScanEvent::Pruned { dir: 1 }).unwrap();
    assert!(t.take_staged_dropped());
    assert!(!t.take_staged_dropped(), "told once");
    assert_eq!(t.staged_outermost(), [ids.clip]);

    t.remove(ids.clip).unwrap();
    assert!(t.take_staged_dropped());
    assert_eq!(t.staged_totals().items, 0);
}

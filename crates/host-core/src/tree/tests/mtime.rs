//! The newest modification time of a folder: files' own, directories'
//! running and final maxima.

use super::super::*;
use super::support::*;
use socketsweep_wire::{DirId, Entry, ScanEvent, Stat};

fn at(mut e: Entry, mtime: i64) -> Entry {
    if let Some(s) = e.stat.as_mut() {
        s.mtime = mtime;
    }
    e
}

fn done_at(d: DirId, newest: i64) -> ScanEvent {
    ScanEvent::SubtreeDone {
        dir: d,
        totals: Totals {
            files: 1,
            newest_mtime: newest,
            ..Totals::default()
        },
    }
}

#[test]
fn a_folder_is_as_new_as_the_newest_thing_inside_it() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![at(dir("A", 1), 50), at(file("top", 1), 10)]),
            listing(1, vec![at(file("a", 1), 300), at(file("old", 1), 20)]),
        ],
    );
    let a = child(&t, ROOT, "A");
    let top = child(&t, ROOT, "top");
    let a_file = child(&t, a, "a");
    assert_eq!(t.newest_mtime(top), Some(10));
    assert_eq!(t.newest_mtime(a_file), Some(300));
    // Partial: the running maximum, the directory's own mtime included.
    assert_eq!(t.newest_mtime(a), Some(300));
    assert_eq!(t.newest_mtime(ROOT), Some(300));

    // Final: the daemon's figure arrives and still wins over a lower own
    // mtime.
    apply(&mut t, &[done_at(1, 400)]);
    assert_eq!(t.newest_mtime(a), Some(400));
    assert_eq!(t.newest_mtime(ROOT), Some(400));
}

#[test]
fn deferred_stats_raise_the_maximum_and_unknown_is_none() {
    let mut t = tree();
    apply(&mut t, &[listing(0, vec![bare("f"), dir("A", 1)])]);
    let f = child(&t, ROOT, "f");
    assert_eq!(t.newest_mtime(f), None);
    assert_eq!(t.newest_mtime(ROOT), None);

    let stat = Stat {
        mtime: 77,
        ..stat(1)
    };
    apply(
        &mut t,
        &[ScanEvent::Stats {
            dir: 0,
            start: 0,
            stats: vec![Some(stat)],
        }],
    );
    assert_eq!(t.newest_mtime(f), Some(77));
    assert_eq!(t.newest_mtime(ROOT), Some(77));
    assert_eq!(t.newest_mtime(9999), None);
}

#[test]
fn deleting_does_not_lower_the_high_water_mark() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), at(file("keep", 1), 5)]),
            listing(1, vec![at(file("new", 1), 900)]),
        ],
    );
    assert_eq!(t.newest_mtime(ROOT), Some(900));
    apply(&mut t, &[ScanEvent::Pruned { dir: 1 }]);
    assert_eq!(t.newest_mtime(ROOT), Some(900));
}

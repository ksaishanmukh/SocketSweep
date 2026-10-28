//! Shared checks for the simulator and pool tests.
#![allow(dead_code)]

use socketsweep_sim::{done_totals, Checker, SimFs};
use socketsweep_wire::{Outcome, ScanEvent, ScanOpts, Totals};

/// Everything a complete scan must satisfy: G1–G3, totals equal to the ground
/// truth of the live tree (every directory when `per_dir`, else the root),
/// and each directory listed and each entry stat'ed exactly once. Without
/// `per_dir` (deletes, hardlinks) flags may be a superset and the newest mtime
/// higher: a prune clears neither (wire `Totals::subtract`).
pub fn check_complete(fs: &SimFs, opts: &ScanOpts, events: &[ScanEvent], per_dir: bool) {
    Checker::check(events).unwrap_or_else(|e| panic!("event stream: {e}"));
    let Some(ScanEvent::Finished {
        outcome, totals, ..
    }) = events.last()
    else {
        panic!("no Finished at the end");
    };
    assert_eq!(*outcome, Outcome::Complete);
    let truth = fs.truth(opts.max_depth, opts.one_filesystem);
    let want = truth.root();
    if per_dir {
        assert_eq!(*totals, want, "root totals");
    } else {
        assert_eq!(totals.flags & want.flags, want.flags, "root flags");
        // Deletes during the scan may leave the newest mtime above the
        // survivors' maximum: it is a high-water mark.
        assert!(
            totals.newest_mtime >= want.newest_mtime,
            "root newest mtime"
        );
        assert_eq!(
            Totals {
                flags: 0,
                newest_mtime: 0,
                ..*totals
            },
            Totals {
                flags: 0,
                newest_mtime: 0,
                ..want
            },
            "root totals"
        );
    }
    if per_dir {
        for (path, t) in done_totals(events) {
            let id = fs
                .lookup(&path)
                .unwrap_or_else(|| panic!("unknown dir {path:?}"));
            let want: Totals = truth.totals[id as usize].expect("walked");
            assert_eq!(t, want, "totals of {}", String::from_utf8_lossy(&path));
        }
    }
    fs.check_counters(&truth)
        .unwrap_or_else(|e| panic!("counters: {e}"));
}

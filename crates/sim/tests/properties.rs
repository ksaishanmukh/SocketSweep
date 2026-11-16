//! Engine properties, over random trees and navigation scripts, driving the
//! real threaded pool: whatever the interleaving, the stream obeys G1–G3 and
//! the totals are exact.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use socketsweep_engine::{Fs, RecordingSink, Scan};
use socketsweep_sim::{Checker, RandomShape, SimFs, SlowFs};
use socketsweep_wire::{DirId, Outcome, ScanEvent, ScanMode, ScanOpts};

fn small_opts(workers: u8, mode: ScanMode) -> ScanOpts {
    ScanOpts {
        workers: Some(workers),
        mode,
    }
}

/// A little real delay per call, so tasks overlap and the script's steps land
/// mid-scan.
fn slow(fs: SimFs) -> Arc<SlowFs<SimFs>> {
    Arc::new(SlowFs::new(
        fs,
        Duration::from_micros(50),
        Duration::from_micros(2),
    ))
}

/// Directories introduced so far, in order.
fn introduced(events: &[ScanEvent]) -> Vec<DirId> {
    events
        .iter()
        .filter_map(|e| match e {
            ScanEvent::Listing { entries, .. } => Some(entries.iter().filter_map(|en| en.id)),
            _ => None,
        })
        .flatten()
        .collect()
}

/// One step of a navigation script: `kind` 0 focuses a directory, 1 clears the
/// focus, 2 deletes a directory (when deletes are allowed); `pick` chooses
/// which.
type Step = (u8, u32);

/// Scan `fs` on a pool while this thread plays `steps`, as the session does:
/// a prune first, then the delete on the file system. Returns the events and
/// how many directories were pruned.
fn run_script(
    fs: &Arc<SlowFs<SimFs>>,
    opts: &ScanOpts,
    steps: &[Step],
    deletes: bool,
) -> (Vec<ScanEvent>, usize) {
    let sink = Arc::new(RecordingSink::new());
    let scan = Scan::new(
        fs.root(fs.inner.root_path()).unwrap(),
        opts,
        Arc::clone(&sink),
    );
    let runner = {
        let (scan, fs) = (scan.clone(), Arc::clone(fs));
        std::thread::spawn(move || scan.run(&*fs))
    };
    let (mut pruned, mut seq) = (0, 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    for &(kind, pick) in steps {
        let known = loop {
            let known = introduced(&sink.events());
            if !known.is_empty() || scan.is_finished() || Instant::now() > deadline {
                break known;
            }
            std::thread::sleep(Duration::from_micros(200));
        };
        if scan.is_finished() || known.is_empty() {
            break;
        }
        let dir = known[pick as usize % known.len()];
        seq += 1;
        match kind {
            0 => {
                scan.focus(seq, dir);
            }
            1 => {
                scan.clear_focus(seq);
            }
            _ if deletes => {
                if let Some(path) = scan.path_of(dir).filter(|_| scan.prune(dir)) {
                    assert!(fs.inner.delete(&path));
                    pruned += 1;
                }
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_micros(300));
    }
    assert_eq!(runner.join().unwrap(), Outcome::Complete);
    (sink.events(), pruned)
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec((0u8..3, any::<u32>()), 0..14)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    /// Exactly-once listing and stats, totals equal to the ground truth (per
    /// directory without deletes, at the root with them) and G1–G3, in both
    /// modes, under focus changes and deletes from another thread.
    #[test]
    fn scans_are_exact_and_ordered(
        tree in any::<u64>(),
        workers in 1u8..=8,
        script in steps(),
        deletes in any::<bool>(),
        mode in prop_oneof![Just(ScanMode::Focus), Just(ScanMode::Plain)],
    ) {
        let fs = slow(SimFs::random(tree, RandomShape::default()));
        let opts = small_opts(workers, mode);
        let (events, pruned) = run_script(&fs, &opts, &script, deletes);
        common::check_complete(&fs.inner, &events, pruned == 0);
        let n = events.iter().filter(|e| matches!(e, ScanEvent::Pruned { .. })).count();
        prop_assert_eq!(n, pruned);
    }

    /// Hardlinked files count once per (dev, ino) at the root.
    #[test]
    fn hardlinks_count_once(tree in any::<u64>(), workers in 1u8..=8) {
        let shape = RandomShape { hardlinks: true, ..RandomShape::default() };
        let fs = slow(SimFs::random(tree, shape));
        let opts = small_opts(workers, ScanMode::Focus);
        let (events, _) = run_script(&fs, &opts, &[], false);
        common::check_complete(&fs.inner, &events, false);
    }

    /// A cancelled scan still ends with `Finished { Cancelled }` (or completes
    /// if it was already done) and a valid stream, and never reads anything
    /// twice.
    #[test]
    fn cancel_ends_cleanly(tree in any::<u64>(), workers in 1u8..=8, after_us in 0u64..20_000) {
        let fs = slow(SimFs::random(tree, RandomShape::default()));
        let opts = small_opts(workers, ScanMode::Focus);
        let sink = Arc::new(RecordingSink::new());
        let scan = Scan::new(fs.root(fs.inner.root_path()).unwrap(), &opts, Arc::clone(&sink));
        let runner = {
            let (scan, fs) = (scan.clone(), Arc::clone(&fs));
            std::thread::spawn(move || scan.run(&*fs))
        };
        std::thread::sleep(Duration::from_micros(after_us));
        scan.cancel();
        let outcome = runner.join().unwrap();
        prop_assert!(matches!(outcome, Outcome::Cancelled | Outcome::Complete));
        let events = sink.events();
        Checker::check(&events).map_err(TestCaseError::fail)?;
        let last = events.last();
        prop_assert!(
            matches!(last, Some(ScanEvent::Finished { outcome: o, .. }) if *o == outcome),
            "{last:?}"
        );
        for id in 0..fs.inner.len() as u32 {
            prop_assert!(fs.inner.lists(id) <= 1 && fs.inner.stats(id) <= 1);
        }
    }
}

/// Stale focus messages are ignored, and a focus on a finished folder is
/// reported settled at once.
#[test]
fn focus_seq_and_settled() {
    use socketsweep_engine::Interrupt;
    use socketsweep_engine::{Pick, ScanState, TaskOutput};
    let fs = SimFs::random(
        3,
        RandomShape {
            faults: false,
            ..RandomShape::default()
        },
    );
    let root = fs.root(fs.root_path()).unwrap();
    let mut st = ScanState::new(root, &ScanOpts::default(), 1, 0);
    let mut events = st.take_events();
    let mut now = 0;
    while !st.is_finished() {
        now += 1;
        match st.pick(0, now) {
            Pick::Run(t) => {
                let out: TaskOutput = t.run(&fs, Interrupt::NEVER);
                st.apply(0, out, now);
            }
            other => panic!("single worker got {other:?}"),
        }
        events.extend(st.take_events());
        if now == 3 {
            assert!(st.set_focus(5, 1, now));
            assert!(!st.set_focus(4, 2, now), "stale seq");
            assert!(!st.clear_focus(5, now), "same seq is stale too");
            events.extend(st.take_events());
        }
    }
    Checker::check(&events).unwrap();
    let settled: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, ScanEvent::FocusState { settled: true, .. }))
        .collect();
    assert_eq!(settled.len(), 1, "focus on dir 1 settles once");
    // After the scan, nothing changes.
    assert!(!st.set_focus(9, 1, now));
}

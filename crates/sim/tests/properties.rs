//! Spec section 5 properties, over random trees and navigation scripts,
//! driving `ScanState` through the discrete-event simulator.

mod common;

use proptest::prelude::*;
use socketsweep_sim::{
    random_script, run_v4, Action, Checker, Latency, RandomShape, Script, SimConfig, SimFs,
};
use socketsweep_wire::{Outcome, ScanEvent, ScanOpts};

fn small_cfg(workers: u8, max_depth: u16) -> SimConfig {
    SimConfig {
        workers,
        opts: ScanOpts {
            max_depth,
            // Small trees scan in tens of virtual milliseconds: scale the
            // ramp and the skeleton budget down with them.
            ramp_ms: 5,
            skeleton_budget_ms: 10,
            ..ScanOpts::default()
        },
        // Random trees take ~40 virtual ms at W = 8 with these costs, long
        // enough for the 5 ms ramp and the convergence check to matter.
        latency: Latency::STANDARD,
        tick: 1_000,
        check_tiers: true,
        validate_every: 5,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// Exactly-once listing and stats, totals equal to the ground truth (per
    /// directory without deletes, at the root with them), G1–G3, the Back
    /// floor and convergence of each tier to its target.
    #[test]
    fn scans_are_exact_and_ordered(
        tree in any::<u64>(),
        script_seed in any::<u64>(),
        workers in 1u8..=8,
        steps in 0usize..14,
        deletes in any::<bool>(),
        max_depth in prop_oneof![Just(64u16), 1u16..5],
    ) {
        let fs = SimFs::random(tree, RandomShape::default());
        let cfg = small_cfg(workers, max_depth);
        let script = random_script(&fs, script_seed, steps, 60_000, deletes);
        let run = run_v4(&fs, &cfg, &script);
        let events: Vec<ScanEvent> = run.events().cloned().collect();
        common::check_complete(&fs, &cfg.opts, &events, run.deleted.is_empty());
        prop_assert!(run.violations.is_empty(), "tier properties: {:#?}", run.violations);
        let n = events.iter().filter(|e| matches!(e, ScanEvent::Pruned { .. })).count();
        prop_assert_eq!(n, run.deleted.len());
    }

    /// Hardlinked files count once per (dev, ino) at the root.
    #[test]
    fn hardlinks_count_once(tree in any::<u64>(), workers in 1u8..=8) {
        let fs = SimFs::random(tree, RandomShape { hardlinks: true, ..RandomShape::default() });
        let cfg = small_cfg(workers, 64);
        let run = run_v4(&fs, &cfg, &Script::default());
        let events: Vec<ScanEvent> = run.events().cloned().collect();
        common::check_complete(&fs, &cfg.opts, &events, false);
    }

    /// A cancelled scan still ends with `Finished { Cancelled }` and a valid
    /// stream, and never reads anything twice.
    #[test]
    fn cancel_ends_cleanly(tree in any::<u64>(), workers in 1u8..=8, at in 0u64..60_000) {
        let fs = SimFs::random(tree, RandomShape::default());
        let cfg = small_cfg(workers, 64);
        let script = Script { steps: vec![(at, Action::Cancel)] };
        let run = run_v4(&fs, &cfg, &script);
        let events: Vec<ScanEvent> = run.events().cloned().collect();
        Checker::check(&events).map_err(TestCaseError::fail)?;
        let outcome = run.outcome.expect("finished");
        prop_assert!(matches!(outcome, Outcome::Cancelled | Outcome::Complete));
        for id in 0..fs.len() as u32 {
            prop_assert!(fs.lists(id) <= 1 && fs.stats(id) <= 1);
        }
    }
}

/// Stale focus messages are ignored, and a focus on a finished folder is
/// reported settled at once.
#[test]
fn focus_seq_and_settled() {
    use socketsweep_engine::Interrupt;
    use socketsweep_engine::{Fs, Pick, ScanState, TaskOutput};
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

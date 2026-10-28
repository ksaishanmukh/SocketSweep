//! The engine's plain mode, the benchmark baseline: a
//! plain parallel walk with no skeleton, no deferred stats, and a focus that
//! is acknowledged but does not steer the workers.

mod common;

use socketsweep_sim::{
    run_baseline, run_v4, script_a, script_b, script_c, RandomShape, SimConfig, SimFs,
};
use socketsweep_wire::{ScanEvent, ScanMode};

fn plain() -> SimConfig {
    let mut cfg = SimConfig::standard();
    cfg.opts.mode = ScanMode::Plain;
    cfg
}

#[test]
fn plain_mode_reads_every_directory_once_with_full_tasks() {
    for seed in 0..6u64 {
        let fs = SimFs::random(
            seed,
            RandomShape {
                max_dirs: 300,
                ..RandomShape::default()
            },
        );
        let mut cfg = plain();
        cfg.validate_every = 7;
        let run = run_v4(&fs, &cfg, &script_c(&fs, seed));
        let events: Vec<ScanEvent> = run.events().cloned().collect();
        common::check_complete(&fs, &cfg.opts, &events, true);
        assert_eq!(run.tasks[0], 0, "no Names tasks");
        assert_eq!(run.tasks[1], 0, "no StatDir tasks");
        assert!(run.skeleton_at.is_none(), "no SkeletonReady");
        assert!(!events
            .iter()
            .any(|e| matches!(e, ScanEvent::SkeletonReady { .. })));
        assert!(run.violations.is_empty(), "{:#?}", run.violations);
    }
}

/// The focus is acknowledged (`FocusState`), and settles exactly when the
/// folder's `SubtreeDone` is sent, but it does not change the schedule: the
/// event stream is the same with or without the clicks, apart from the
/// `FocusState` events themselves.
#[test]
fn plain_mode_ignores_the_focus_for_scheduling() {
    let fs = SimFs::sdcard(3, false);
    let cfg = plain();
    let (script, _) = script_b(&fs);
    let clicked = run_v4(&fs, &cfg, &script);
    let quiet = run_v4(&SimFs::sdcard(3, false), &cfg, &script_a());
    let strip = |r: &socketsweep_sim::V4Run| -> Vec<ScanEvent> {
        r.events()
            .filter(|e| !matches!(e, ScanEvent::FocusState { .. } | ScanEvent::Progress { .. }))
            .cloned()
            .collect()
    };
    assert_eq!(
        strip(&clicked),
        strip(&quiet),
        "the clicks changed the walk"
    );
    assert_eq!(clicked.finished_at, quiet.finished_at);
    assert_eq!(clicked.focus.len(), 2);
    // Only the folder open at the time settles: a click superseded before
    // its folder finished never does.
    for (i, f) in clicked.focus.iter().enumerate() {
        assert!(f.dir.is_some(), "{f:?}");
        let done = clicked.done_at[&f.path].max(f.at);
        let open_until = clicked.focus.get(i + 1).map_or(u64::MAX, |n| n.at);
        let want = (done < open_until).then_some(done);
        assert_eq!(f.settled_at, want, "{f:?}");
    }
    assert!(clicked.focus.last().unwrap().settled_at.is_some());
}

/// On the simulator, the plain mode is in the same league as the jwalk-like
/// reference engine (per-worker LIFO with stealing): it is the baseline the
/// focus mode is measured against on devices, so it must not be a straw man.
#[test]
fn plain_mode_keeps_up_with_a_work_stealing_walk() {
    for seed in [1, 2] {
        let fs = SimFs::sdcard(seed, false);
        let cfg = plain();
        let run = run_v4(&fs, &cfg, &script_a());
        let base = run_baseline(&fs, cfg.workers, cfg.latency, &cfg.opts);
        let ratio = run.finished_at as f64 / base.finished_at as f64;
        println!(
            "seed {seed}: plain {} us, work-stealing {} us, ratio {ratio:.3}",
            run.finished_at, base.finished_at
        );
        assert!(
            ratio <= 1.10,
            "plain mode is {ratio:.3}x the work-stealing walk"
        );
    }
}

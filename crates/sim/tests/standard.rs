//! The standard `/sdcard` tree: skeleton timing, the v4-vs-baseline
//! comparison, and a golden trace.

mod common;

use socketsweep_sim::{
    compare_standard, format_report, largest_child, run_v4, script_a, trace_hash, Action,
    RandomShape, Script, SimConfig, SimFs,
};
use socketsweep_wire::{Kind, ScanEvent, ROOT_DIR};

/// `SkeletonReady` within 1 s of virtual time, covering at least two levels
/// (the root and every top-level folder listed), on a complete, exact scan.
#[test]
fn skeleton_is_ready_within_a_second() {
    let fs = SimFs::standard(1);
    let cfg = SimConfig::standard();
    let run = run_v4(&fs, &cfg, &script_a());
    let at = run.skeleton_at.expect("SkeletonReady");
    assert!(at <= 1_000_000, "skeleton ready at {at} us");
    let events: Vec<ScanEvent> = run.events().cloned().collect();
    let ready = events
        .iter()
        .position(|e| matches!(e, ScanEvent::SkeletonReady { .. }))
        .unwrap();
    let mut top = Vec::new();
    let mut listed = std::collections::HashSet::new();
    for e in &events[..ready] {
        if let ScanEvent::Listing {
            dir, entries, last, ..
        } = e
        {
            if *dir == ROOT_DIR {
                top.extend(entries.iter().filter_map(|e| e.id));
            }
            if *last {
                listed.insert(*dir);
            }
        }
    }
    assert!(listed.contains(&ROOT_DIR));
    assert!(
        top.iter().all(|d| listed.contains(d)),
        "every top-level folder listed"
    );
    common::check_complete(&fs, &cfg.opts, &events, true);
    assert!(run.violations.is_empty(), "{:#?}", run.violations);
}

/// The comparison on the simulator. Script (a): total ≤ 1.05× the baseline.
/// Script (b): the folder open at the end is measured, from the click, in at
/// most half the baseline's time (the drill target is already measured when
/// the user gets there), and the first click's wait is well below the
/// baseline's. Script (c): total ≤ 1.10× and the tier properties hold.
#[test]
fn v4_against_the_baseline() {
    for seed in [1, 5] {
        let cfg = SimConfig::standard();
        let r = compare_standard(seed, &cfg);
        let report = format_report(&r);
        println!("seed {seed}\n{report}");
        let (a, b, c) = (&r[0], &r[1], &r[2]);
        assert!(a.total_ratio() <= 1.05, "script a total\n{report}");
        assert!(c.total_ratio() <= 1.10, "script c total\n{report}");
        for s in [a, b, c] {
            assert_eq!(s.violations, 0, "{}: tier properties\n{report}", s.name);
        }
        assert_eq!(b.clicks.len(), 2, "both clicks of script b resolved");
        let last = b.last_click().unwrap();
        let (v4, base) = (last.v4_wait().unwrap(), last.base_wait().unwrap());
        assert!(
            base > 0,
            "the baseline has not finished the folder at the drill\n{report}"
        );
        assert!(v4 * 2 <= base, "script b final folder wait\n{report}");
        let first = &b.clicks[0];
        let (v4, base) = (first.v4_wait().unwrap(), first.base_wait().unwrap());
        assert!(
            v4 as f64 <= 0.65 * base as f64,
            "script b first click wait\n{report}"
        );
        assert!(
            last.v4_done.unwrap() as f64 <= 0.7 * last.base_done.unwrap() as f64,
            "script b folder done\n{report}"
        );
    }
}

/// A fixed seed, tree and script always produce the same event stream. The
/// constant changes only when the scheduling changes on purpose.
#[test]
fn golden_trace() {
    let fs = SimFs::random(
        0x5eed,
        RandomShape {
            max_dirs: 400,
            ..RandomShape::default()
        },
    );
    let mut cfg = SimConfig::standard();
    cfg.opts.ramp_ms = 5;
    let top = largest_child(&fs, 0).unwrap();
    let inner = largest_child(&fs, top).unwrap();
    let other = fs
        .children(0)
        .find(|&c| c != top && fs.node(c).kind == Kind::Dir && fs.node(c).fail_open.is_none())
        .unwrap();
    let script = Script {
        steps: vec![
            (3_000, Action::Focus(fs.path_of(top))),
            (9_000, Action::Focus(fs.path_of(inner))),
            (15_000, Action::Delete(fs.path_of(other))),
            (20_000, Action::ClearFocus),
        ],
    };
    let run = run_v4(&fs, &cfg, &script);
    let events: Vec<ScanEvent> = run.events().cloned().collect();
    common::check_complete(&fs, &cfg.opts, &events, false);
    assert!(run.tier_samples > 0);
    assert_eq!(run.deleted.len(), 1, "the delete hit a known directory");
    assert!(
        run.focus.iter().all(|f| f.dir.is_some()),
        "both focus paths resolved"
    );
    let h = trace_hash(&events);
    assert_eq!(
        h,
        0xf048_30d2_0322_7cce,
        "golden trace hash is {h:#018x} ({} events)",
        events.len()
    );
}

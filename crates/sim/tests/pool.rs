//! The real threaded pool against `SimFs`: exactly-once work, exact totals
//! and G1–G3 under true concurrency, with focus, prunes, congestion and
//! cancel delivered from another thread.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use socketsweep_engine::{Fs, RecordingSink, Scan};
use socketsweep_sim::{Checker, RandomShape, SimFs, SlowFs};
use socketsweep_wire::{DirId, Outcome, ScanEvent, ScanOpts};

fn opts(workers: u8) -> ScanOpts {
    ScanOpts {
        workers: Some(workers),
        ..ScanOpts::default()
    }
}

/// Paths of introduced directories, from the events so far.
fn known_dirs(events: &[ScanEvent]) -> Vec<(DirId, Vec<u8>)> {
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::new();
    for e in events {
        match e {
            ScanEvent::Started { root, .. } => paths = vec![root.clone()],
            ScanEvent::Listing { dir, entries, .. } => {
                for en in entries {
                    if let Some(id) = en.id {
                        let mut p = paths[*dir as usize].clone();
                        p.push(b'/');
                        p.extend_from_slice(&en.name);
                        if paths.len() <= id as usize {
                            paths.resize(id as usize + 1, Vec::new());
                        }
                        paths[id as usize] = p.clone();
                        out.push((id, p));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn pool_is_exact_on_random_trees() {
    for seed in 0..12u64 {
        for workers in [1u8, 3, 8] {
            let fs = SimFs::random(
                seed,
                RandomShape {
                    max_dirs: 300,
                    ..RandomShape::default()
                },
            );
            let o = opts(workers);
            let sink = Arc::new(RecordingSink::new());
            let scan = Scan::new(fs.root(fs.root_path()).unwrap(), &o, Arc::clone(&sink));
            assert_eq!(scan.run(&fs), Outcome::Complete);
            common::check_complete(&fs, &o, &sink.events(), true);
        }
    }
}

#[test]
fn pool_handles_focus_prune_and_congestion_from_another_thread() {
    let mut total_pruned = 0;
    for seed in 0..4u64 {
        let fs = Arc::new(SlowFs::new(
            SimFs::random(
                seed,
                RandomShape {
                    max_dirs: 400,
                    ..RandomShape::default()
                },
            ),
            Duration::from_micros(200),
            Duration::from_micros(5),
        ));
        let o = opts(8);
        let sink = Arc::new(RecordingSink::new());
        let scan = Scan::new(
            fs.root(fs.inner.root_path()).unwrap(),
            &o,
            Arc::clone(&sink),
        );
        let runner = {
            let (scan, fs) = (scan.clone(), Arc::clone(&fs));
            std::thread::spawn(move || scan.run(&*fs))
        };
        let start = Instant::now();
        let mut seq = 0;
        let mut pruned = 0;
        let mut congested_once = false;
        while !scan.is_finished() && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(3));
            let known = known_dirs(&sink.events());
            if known.len() < 4 {
                continue;
            }
            seq += 1;
            let (d, _) = &known[seq as usize * 7 % known.len()];
            scan.focus(seq, *d);
            if seq % 4 == 0 && pruned < 2 {
                // As the session does: prune first, then delete.
                let (d, path) = &known[known.len() - 1 - seq as usize % known.len()];
                if scan.prune(*d) {
                    assert!(fs.inner.delete(path));
                    pruned += 1;
                }
            }
            if !congested_once && seq == 3 {
                sink.set_congested(true);
                std::thread::sleep(Duration::from_millis(20));
                sink.set_congested(false);
                congested_once = true;
            }
        }
        assert_eq!(runner.join().unwrap(), Outcome::Complete);
        common::check_complete(&fs.inner, &o, &sink.events(), pruned == 0);
        total_pruned += pruned;
    }
    assert!(total_pruned > 0, "no prune happened during a scan");
}

#[test]
fn pool_cancel_ends_with_finished_cancelled() {
    let fs = Arc::new(SlowFs::new(
        SimFs::random(
            9,
            RandomShape {
                max_dirs: 400,
                ..RandomShape::default()
            },
        ),
        Duration::from_millis(1),
        Duration::from_micros(20),
    ));
    let sink = Arc::new(RecordingSink::new());
    let scan = Scan::new(
        fs.root(fs.inner.root_path()).unwrap(),
        &opts(4),
        Arc::clone(&sink),
    );
    let runner = {
        let (scan, fs) = (scan.clone(), Arc::clone(&fs));
        std::thread::spawn(move || scan.run(&*fs))
    };
    std::thread::sleep(Duration::from_millis(15));
    scan.cancel();
    let t = Instant::now();
    assert_eq!(runner.join().unwrap(), Outcome::Cancelled);
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "workers stop promptly"
    );
    let events = sink.events();
    Checker::check(&events).unwrap();
    assert!(matches!(
        events.last(),
        Some(ScanEvent::Finished {
            outcome: Outcome::Cancelled,
            ..
        })
    ));
    for id in 0..fs.inner.len() as u32 {
        assert!(fs.inner.lists(id) <= 1 && fs.inner.stats(id) <= 1);
    }
}

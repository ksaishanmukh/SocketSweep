//! The benchmark driver against `FakeDaemon` (the real session and engine on
//! a `SimFs`), so everything but adb and the phone is exercised offline.

use std::time::Duration;

use socketsweep_bench::bench::{self, Config, RunRecord, ScriptKind};
use socketsweep_bench::{fixture, summary};
use socketsweep_host_core::{Client, ClientConfig};
use socketsweep_sim::{FakeDaemon, RandomShape, SimFs};
use socketsweep_wire::{Caps, Token};

const TOKEN: Token = [5; 32];

fn client(fs: SimFs, per_call: Duration) -> Client {
    let daemon = FakeDaemon::new(fs).with_latency(per_call, Duration::ZERO);
    let (end, _session) = daemon.connect(TOKEN);
    Client::connect(
        Box::new(end),
        TOKEN,
        [4; 16],
        Caps::all(),
        ClientConfig::default(),
    )
    .expect("handshake")
}

/// One record of the report: same tree as the first, and the mode's and the
/// script's own invariants.
fn check_record(r: &RunRecord, first: &RunRecord) {
    assert_eq!(
        (r.files, r.dirs, r.apparent, r.disk),
        (first.files, first.dirs, first.apparent, first.disk),
        "every scan measures the same tree"
    );
    assert_eq!(r.stats, first.stats);
    assert!(r.first_listing_ms.is_some() && r.bytes > 0);
    match r.mode {
        "plain" => {
            assert!(r.skeleton_ms.is_none(), "plain mode has no skeleton");
            assert_eq!(r.deferred, 0, "plain mode defers no stats");
            assert_eq!(r.opens, r.listed);
        }
        "focus" => {
            let levels = r.skeleton_levels.expect("SkeletonReady");
            assert!(levels >= 1, "{r:?}");
            assert!(r.opens >= r.listed);
        }
        m => panic!("mode {m}"),
    }
    let want = match r.script {
        _ if r.warmup => 0,
        "a" => 0,
        "b" => 2,
        "c" => 10,
        s => panic!("script {s}"),
    };
    assert_eq!(r.clicks.len(), want, "{} clicks", r.script);
    for k in &r.clicks {
        // A click after (or racing) the end of the scan is not sent.
        assert!(
            k.sent_ms.is_some() || k.at_ms + 100.0 >= r.total_ms,
            "every click during the scan was sent: {k:?}"
        );
        assert!(k.wait_ms.is_some(), "every opened folder finished: {k:?}");
    }
}

#[test]
fn the_benchmark_runs_both_modes_and_every_script() {
    let fs = SimFs::random(
        7,
        RandomShape {
            max_dirs: 400,
            ..RandomShape::default()
        },
    );
    let root = fs.root_path().to_vec();
    let c = client(fs, Duration::from_millis(4));
    let cfg = Config {
        root,
        runs: 2,
        warmup: 1,
        timeout: Duration::from_secs(60),
        ..Config::default()
    };
    let mut lines = 0;
    let mut sent = 0;
    let report = bench::run(&c, "fake", &cfg, &mut |_| lines += 1).expect("benchmark");
    // Reference (focus) + plain warm-up + 3 scripts x 2 modes x 2 runs.
    assert_eq!(report.records.len(), 2 + 3 * 2 * 2);
    assert_eq!(lines, report.records.len());
    let first = &report.records[0];
    for r in &report.records {
        check_record(r, first);
    }
    for r in &report.records {
        sent += r.clicks.iter().filter(|k| k.sent_ms.is_some()).count();
    }
    assert!(sent > 0, "no click was sent during a scan");
    let b = &report.scripts[1];
    assert_eq!(b.script, "b");
    assert_eq!(b.clicks.len(), 2);
    assert!(b.clicks[1].1.starts_with(&b.clicks[0].1));
    assert!(report
        .summary
        .iter()
        .any(|r| r.metric == "total ms" && r.script == "a" && r.ratio.is_some()));
    let table = summary::table(&report.summary, &report.criteria);
    assert!(table.contains("total ms"), "{table}");
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"mode\":\"plain\""));
}

/// The device fixture scans to its expected totals (directories' own sizes
/// aside), in both modes.
#[test]
fn the_fixture_scans_to_its_expected_totals() {
    let fs = fixture::build(1, b"/sdcard/ssfixture");
    let want = fixture::expected(&fs);
    let c = client(fs, Duration::ZERO);
    let cfg = Config {
        root: b"/sdcard/ssfixture".to_vec(),
        runs: 1,
        warmup: 1,
        scripts: vec![ScriptKind::B],
        timeout: Duration::from_secs(120),
        ..Config::default()
    };
    let report = bench::run(&c, "fake", &cfg, &mut |_| {}).expect("benchmark");
    for r in &report.records {
        assert_eq!((r.files, r.dirs, r.errors), (want.files, want.dirs, 0));
        // SimFs gives every directory 4096 bytes.
        assert_eq!(r.apparent, want.file_bytes + 4096 * u64::from(want.dirs));
    }
    // The largest top-level folder is Movies (by its big files), and the
    // drill goes into its only child folder.
    assert_eq!(
        report.scripts[0]
            .clicks
            .iter()
            .map(|c| c.1.as_str())
            .collect::<Vec<_>>(),
        [
            "/sdcard/ssfixture/Movies",
            "/sdcard/ssfixture/Movies/.thumbcache"
        ]
    );
}

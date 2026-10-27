//! A small comparison report: v4 against the baseline on the standard tree
//! for the three navigation scripts of spec section 6.

use std::fmt::Write as _;

use socketsweep_engine::Micros;

use crate::des::{run_baseline, run_v4, SimConfig};
use crate::script::{script_a, script_b, script_c};
use crate::simfs::SimFs;

/// One click of a script: how long the user waited for the folder's size to
/// become final, from the click, in each engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    pub path: Vec<u8>,
    pub at: Micros,
    /// When the folder's `SubtreeDone` came (absolute).
    pub v4_done: Option<Micros>,
    pub base_done: Option<Micros>,
}

impl Click {
    /// Time from the click to the answer (0 if it was already there).
    pub fn v4_wait(&self) -> Option<Micros> {
        Some(self.v4_done?.saturating_sub(self.at))
    }

    pub fn base_wait(&self) -> Option<Micros> {
        Some(self.base_done?.saturating_sub(self.at))
    }
}

/// Figures for one script.
#[derive(Debug, Clone, Default)]
pub struct ScriptResult {
    pub name: &'static str,
    pub v4_total: Micros,
    pub base_total: Micros,
    pub skeleton: Option<Micros>,
    /// Clicks that resolved to a folder, in order.
    pub clicks: Vec<Click>,
    pub violations: usize,
    /// v4 tasks run: names, statdir, full.
    pub tasks: [u64; 3],
    /// Storage work (microseconds at full speed) of each engine.
    pub v4_work: f64,
    pub base_work: f64,
}

impl ScriptResult {
    pub fn total_ratio(&self) -> f64 {
        self.v4_total as f64 / self.base_total as f64
    }

    /// The last click (the folder open at the end).
    pub fn last_click(&self) -> Option<&Click> {
        self.clicks.last()
    }
}

/// Run scripts (a), (b) and (c) on `SimFs::standard(seed)` for both engines.
pub fn compare_standard(seed: u64, cfg: &SimConfig) -> Vec<ScriptResult> {
    compare_on(&SimFs::standard(seed), seed, cfg)
}

/// Run scripts (a), (b) and (c) on `fs` for both engines.
pub fn compare_on(fs: &SimFs, seed: u64, cfg: &SimConfig) -> Vec<ScriptResult> {
    fs.reset_counters();
    let base = run_baseline(fs, cfg.workers, cfg.latency, &cfg.opts);
    let (b, _) = script_b(fs);
    let scripts = [
        ("a: no navigation", script_a()),
        ("b: open largest at 300 ms, its largest child at 2 s", b),
        ("c: 10 clicks in 1 s", script_c(fs, seed)),
    ];
    let mut out = Vec::new();
    for (name, script) in scripts {
        fs.reset_counters();
        let v4 = run_v4(fs, cfg, &script);
        let clicks = v4
            .focus
            .iter()
            .filter(|f| f.dir.is_some())
            .map(|f| Click {
                path: f.path.clone(),
                at: f.at,
                v4_done: v4.done_at.get(&f.path).copied(),
                base_done: base.done_at.get(&f.path).copied(),
            })
            .collect();
        out.push(ScriptResult {
            name,
            v4_total: v4.finished_at,
            base_total: base.finished_at,
            skeleton: v4.skeleton_at,
            clicks,
            violations: v4.violations.len(),
            tasks: v4.tasks,
            v4_work: v4.work,
            base_work: base.work,
        });
    }
    out
}

fn ms(t: Option<Micros>) -> String {
    t.map_or("-".into(), |t| format!("{:.0} ms", t as f64 / 1000.0))
}

fn ratio(a: Option<Micros>, b: Option<Micros>) -> String {
    match (a, b) {
        (Some(a), Some(b)) if b > 0 => format!("{:.2}x", a as f64 / b as f64),
        _ => "-".into(),
    }
}

/// Format [`compare_on`]'s results.
pub fn format_report(results: &[ScriptResult]) -> String {
    let mut s = String::new();
    for r in results {
        let _ = writeln!(s, "script {}", r.name);
        let _ = writeln!(
            s,
            "  total: v4 {} vs baseline {} ({:.3}x); skeleton ready at {}",
            ms(Some(r.v4_total)),
            ms(Some(r.base_total)),
            r.total_ratio(),
            ms(r.skeleton),
        );
        let _ = writeln!(
            s,
            "  v4 tasks names/statdir/full {:?}; storage work v4/baseline {:.3}x; tier-property violations {}",
            r.tasks,
            r.v4_work / r.base_work,
            r.violations
        );
        let shown: Vec<&Click> = if r.clicks.len() > 3 {
            r.clicks.last().into_iter().collect()
        } else {
            r.clicks.iter().collect()
        };
        for c in shown {
            let _ = writeln!(
                s,
                "  click {} at {}: wait v4 {} vs baseline {} ({}); done at v4 {} vs baseline {} ({})",
                String::from_utf8_lossy(&c.path),
                ms(Some(c.at)),
                ms(c.v4_wait()),
                ms(c.base_wait()),
                ratio(c.v4_wait(), c.base_wait()),
                ms(c.v4_done),
                ms(c.base_done),
                ratio(c.v4_done, c.base_done),
            );
        }
    }
    s
}

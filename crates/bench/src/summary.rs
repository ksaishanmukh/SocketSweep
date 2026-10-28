//! Medians, 90th percentiles, the table and the acceptance criteria of
//! `docs/architecture.md`, section 6.

use std::fmt::Write as _;

use serde::Serialize;

use crate::bench::RunRecord;

/// One metric of one script, both modes.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub script: &'static str,
    pub metric: &'static str,
    pub focus_median: Option<f64>,
    pub focus_p90: Option<f64>,
    pub plain_median: Option<f64>,
    pub plain_p90: Option<f64>,
    /// focus / plain, at the median.
    pub ratio: Option<f64>,
}

/// One acceptance criterion as measured.
#[derive(Debug, Clone, Serialize)]
pub struct Criterion {
    pub name: String,
    pub measured: String,
    /// `None` when there was nothing to measure (e.g. the script was not run).
    pub pass: Option<bool>,
}

/// The `q` quantile (nearest rank) of `v`; `None` if empty.
pub fn quantile(v: &[f64], q: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let rank = ((q * s.len() as f64).ceil() as usize).clamp(1, s.len());
    Some(s[rank - 1])
}

pub fn median(v: &[f64]) -> Option<f64> {
    quantile(v, 0.5)
}

pub fn p90(v: &[f64]) -> Option<f64> {
    quantile(v, 0.9)
}

type Metric = (&'static str, fn(&RunRecord) -> Option<f64>);

fn waits(r: &RunRecord) -> Vec<f64> {
    r.clicks.iter().filter_map(|c| c.wait_ms).collect()
}

const METRICS: &[Metric] = &[
    ("total ms", |r| Some(r.total_ms)),
    ("first listing ms", |r| r.first_listing_ms),
    ("skeleton ms", |r| r.skeleton_ms),
    ("skeleton levels", |r| r.skeleton_levels.map(f64::from)),
    ("first click wait ms", |r| {
        r.clicks.first().and_then(|c| c.wait_ms)
    }),
    ("last click wait ms", |r| {
        r.clicks.last().and_then(|c| c.wait_ms)
    }),
    ("mean click wait ms", |r| {
        let w = waits(r);
        (!w.is_empty()).then(|| w.iter().sum::<f64>() / w.len() as f64)
    }),
    ("max click wait ms", |r| {
        waits(r).into_iter().max_by(f64::total_cmp)
    }),
    ("dir opens", |r| Some(r.opens as f64)),
    ("stats", |r| Some(r.stats as f64)),
    ("KiB received", |r| Some(r.bytes as f64 / 1024.0)),
];

fn values(
    records: &[RunRecord],
    script: &str,
    mode: &str,
    f: fn(&RunRecord) -> Option<f64>,
) -> Vec<f64> {
    records
        .iter()
        .filter(|r| !r.warmup && r.script == script && r.mode == mode)
        .filter_map(f)
        .collect()
}

fn scripts(records: &[RunRecord]) -> Vec<&'static str> {
    let mut s: Vec<&'static str> = records
        .iter()
        .filter(|r| !r.warmup)
        .map(|r| r.script)
        .collect();
    s.sort();
    s.dedup();
    s
}

/// Median and p90 of every metric that has values, per script.
pub fn summarise(records: &[RunRecord]) -> Vec<Row> {
    let mut rows = Vec::new();
    for script in scripts(records) {
        for &(metric, f) in METRICS {
            let fv = values(records, script, "focus", f);
            let pv = values(records, script, "plain", f);
            if fv.is_empty() && pv.is_empty() {
                continue;
            }
            let (fm, pm) = (median(&fv), median(&pv));
            rows.push(Row {
                script,
                metric,
                focus_median: fm,
                focus_p90: p90(&fv),
                plain_median: pm,
                plain_p90: p90(&pv),
                ratio: match (fm, pm) {
                    (Some(f), Some(p)) if p > 0.0 => Some(f / p),
                    _ => None,
                },
            });
        }
    }
    rows
}

fn ratio_at(records: &[RunRecord], script: &str, metric: Metric, q: f64) -> Option<f64> {
    let f = quantile(&values(records, script, "focus", metric.1), q)?;
    let p = quantile(&values(records, script, "plain", metric.1), q)?;
    (p > 0.0).then_some(f / p)
}

fn metric(name: &str) -> Metric {
    *METRICS.iter().find(|m| m.0 == name).expect("known metric")
}

fn check(name: &str, measured: Option<f64>, limit: f64, fmt: &str) -> Criterion {
    Criterion {
        name: name.to_string(),
        measured: measured.map_or("-".into(), |m| format!("{m:.3}{fmt}")),
        pass: measured.map(|m| m <= limit),
    }
}

/// The acceptance criteria that can be measured from the host.
pub fn criteria(records: &[RunRecord]) -> Vec<Criterion> {
    let total = metric("total ms");
    let mut out = vec![
        check(
            "1. dir opens <= 1.01x baseline (script a, median)",
            ratio_at(records, "a", metric("dir opens"), 0.5),
            1.01,
            "x",
        ),
        check(
            "2. script a total <= 1.05x baseline (median)",
            ratio_at(records, "a", total, 0.5),
            1.05,
            "x",
        ),
        check(
            "2. script a total <= 1.10x baseline (p90)",
            ratio_at(records, "a", total, 0.9),
            1.10,
            "x",
        ),
        check(
            "3. script c total <= 1.10x baseline (median)",
            ratio_at(records, "c", total, 0.5),
            1.10,
            "x",
        ),
    ];
    let sk = values(records, "a", "focus", metric("skeleton ms").1);
    out.push(check(
        "4. SkeletonReady within 1000 ms (script a, p90)",
        p90(&sk),
        1000.0,
        " ms",
    ));
    let levels = values(records, "a", "focus", metric("skeleton levels").1);
    out.push(Criterion {
        name: "4. SkeletonReady covers >= 2 levels (script a, min)".into(),
        measured: levels
            .iter()
            .copied()
            .min_by(f64::total_cmp)
            .map_or("-".into(), |m| format!("{m}")),
        pass: levels
            .iter()
            .copied()
            .min_by(f64::total_cmp)
            .map(|m| m >= 2.0),
    });
    out.push(check(
        "5. script b open folder's answer <= 0.5x baseline (median)",
        ratio_at(records, "b", metric("last click wait ms"), 0.5),
        0.5,
        "x",
    ));
    out
}

fn cell(v: Option<f64>) -> String {
    match v {
        None => "-".into(),
        Some(v) if v.abs() >= 100.0 => format!("{v:.0}"),
        Some(v) => format!("{v:.2}"),
    }
}

/// The summary as a text table, then the criteria.
pub fn table(rows: &[Row], criteria: &[Criterion]) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{:<6} {:<22} {:>12} {:>12} {:>12} {:>12} {:>8}",
        "script", "metric", "focus p50", "focus p90", "plain p50", "plain p90", "ratio"
    );
    for r in rows {
        let _ = writeln!(
            s,
            "{:<6} {:<22} {:>12} {:>12} {:>12} {:>12} {:>8}",
            r.script,
            r.metric,
            cell(r.focus_median),
            cell(r.focus_p90),
            cell(r.plain_median),
            cell(r.plain_p90),
            r.ratio.map_or("-".into(), |x| format!("{x:.3}"))
        );
    }
    let _ = writeln!(s);
    for c in criteria {
        let verdict = match c.pass {
            Some(true) => "PASS",
            Some(false) => "FAIL",
            None => "n/a ",
        };
        let _ = writeln!(s, "[{verdict}] {} : {}", c.name, c.measured);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_are_nearest_rank() {
        let v: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(median(&v), Some(5.0));
        assert_eq!(p90(&v), Some(9.0));
        assert_eq!(quantile(&v, 1.0), Some(10.0));
        assert_eq!(median(&[3.0]), Some(3.0));
        assert_eq!(p90(&[2.0, 1.0, 3.0]), Some(3.0));
        assert_eq!(median(&[]), None);
    }
}

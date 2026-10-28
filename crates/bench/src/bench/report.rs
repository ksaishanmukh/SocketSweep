//! What a benchmark records and reports.

use std::time::Duration;

use serde::Serialize;

/// One click, as it went.
#[derive(Debug, Clone, Serialize)]
pub struct ClickRecord {
    pub path: String,
    pub at_ms: f64,
    /// When `Focus` was sent: at `at`, or as soon as the folder was
    /// introduced if it was not yet. `None`: never introduced.
    pub sent_ms: Option<f64>,
    /// From the click to the folder's `SubtreeDone` (0 if it was already
    /// done). The comparable "answer" time in both modes.
    pub wait_ms: Option<f64>,
    /// From the click to `FocusState { settled }` (focus mode; plain mode
    /// sends it too, for the folder still open, at its `SubtreeDone`).
    pub settled_ms: Option<f64>,
}

/// One scan.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub script: &'static str,
    pub mode: &'static str,
    /// 0 for warm-up runs, which are not summarised.
    pub run: usize,
    pub warmup: bool,
    /// From sending `StartScan` to receiving `Finished`.
    pub total_ms: f64,
    /// The daemon's own `Finished.elapsed_ms`.
    pub daemon_ms: u32,
    pub first_listing_ms: Option<f64>,
    pub skeleton_ms: Option<f64>,
    pub skeleton_levels: Option<u32>,
    pub skeleton_listed: Option<u32>,
    pub clicks: Vec<ClickRecord>,
    /// Directory opens: one per listing, plus one per deferred `StatDir`
    /// (derived from the listings by `Recorder::deferred`).
    pub opens: u64,
    pub listed: u64,
    pub deferred: u64,
    /// Entries stat'ed (each exactly once in either mode).
    pub stats: u64,
    pub bytes: u64,
    pub frames: u64,
    pub events: u64,
    pub files: u32,
    pub dirs: u32,
    pub apparent: u64,
    pub disk: u64,
    pub errors: u32,
}

pub(super) fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Everything a benchmark produced; written as JSON.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub root: String,
    pub device: String,
    pub daemon_build: String,
    pub workers: u8,
    pub runs: usize,
    pub seed: u64,
    pub scripts: Vec<ScriptSetup>,
    pub records: Vec<RunRecord>,
    pub summary: Vec<crate::summary::Row>,
    pub criteria: Vec<crate::summary::Criterion>,
}

/// The folders a script opens.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptSetup {
    pub script: &'static str,
    pub clicks: Vec<(f64, String)>,
}

//! The benchmark of `docs/architecture.md`, section 6: the engine's focus
//! mode against its plain mode (the baseline), on one connection, with the
//! navigation scripts (a), (b) and (c), interleaved.

use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use serde::Serialize;
use socketsweep_host_core::Client;
use socketsweep_sim::Rng;
use socketsweep_wire::{Outcome, ScanMode, ScanOpts};

use crate::record::Recorder;

/// One navigation script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScriptKind {
    /// (a) No navigation.
    A,
    /// (b) Open the largest top-level folder at 300 ms, then its largest
    /// child at 2 s.
    B,
    /// (c) Ten opens of random top-level folders within 1 s.
    C,
}

impl ScriptKind {
    pub fn name(self) -> &'static str {
        match self {
            ScriptKind::A => "a",
            ScriptKind::B => "b",
            ScriptKind::C => "c",
        }
    }

    pub fn parse(s: &str) -> Option<ScriptKind> {
        match s {
            "a" => Some(ScriptKind::A),
            "b" => Some(ScriptKind::B),
            "c" => Some(ScriptKind::C),
            _ => None,
        }
    }
}

pub fn mode_name(mode: ScanMode) -> &'static str {
    match mode {
        ScanMode::Focus => "focus",
        ScanMode::Plain => "plain",
    }
}

/// A folder opened at a time after `StartScan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    pub at: Duration,
    /// The folder's full path on the phone.
    pub path: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub kind: ScriptKind,
    pub clicks: Vec<Click>,
}

/// The tree from a complete reference scan, to choose the folders the
/// scripts open: the same ones in every run and both modes.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    /// Top-level folders with their disk usage (subtree plus own entry is
    /// not needed for ranking: the subtree's totals).
    pub top: Vec<(Vec<u8>, u64)>,
    /// For each top-level folder, its child folders with their disk usage.
    pub children: Vec<Vec<(Vec<u8>, u64)>>,
}

impl Reference {
    pub fn from_scan(r: &Recorder) -> Reference {
        let mut dirs: Vec<(Vec<u8>, u32, u64)> = r
            .finished_dirs()
            .map(|(p, d, t)| (p.to_vec(), d, t.disk))
            .collect();
        dirs.sort();
        let top: Vec<(Vec<u8>, u64)> = dirs
            .iter()
            .filter(|d| d.1 == 1)
            .map(|d| (d.0.clone(), d.2))
            .collect();
        let children = top
            .iter()
            .map(|(tp, _)| {
                dirs.iter()
                    .filter(|d| d.1 == 2 && parent_of(&d.0) == tp.as_slice())
                    .map(|d| (d.0.clone(), d.2))
                    .collect()
            })
            .collect();
        Reference { top, children }
    }

    fn largest(v: &[(Vec<u8>, u64)]) -> Option<usize> {
        // Largest by disk, ties to the first by path.
        let mut best: Option<usize> = None;
        for (i, (_, n)) in v.iter().enumerate() {
            if best.is_none_or(|b| *n > v[b].1) {
                best = Some(i);
            }
        }
        best
    }

    /// The script `kind`; `seed` drives (c).
    pub fn script(&self, kind: ScriptKind, seed: u64) -> Script {
        let ms = Duration::from_millis;
        let clicks = match kind {
            ScriptKind::A => Vec::new(),
            ScriptKind::B => match Self::largest(&self.top) {
                None => Vec::new(),
                Some(t) => {
                    let mut v = vec![Click {
                        at: ms(300),
                        path: self.top[t].0.clone(),
                    }];
                    if let Some(c) = Self::largest(&self.children[t]) {
                        v.push(Click {
                            at: ms(2000),
                            path: self.children[t][c].0.clone(),
                        });
                    }
                    v
                }
            },
            ScriptKind::C if self.top.is_empty() => Vec::new(),
            ScriptKind::C => {
                let mut rng = Rng::new(seed);
                (0..10)
                    .map(|i| Click {
                        at: ms(100 + i * 100),
                        path: self.top[rng.below(self.top.len() as u64) as usize]
                            .0
                            .clone(),
                    })
                    .collect()
            }
        };
        Script { kind, clicks }
    }
}

fn parent_of(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|&b| b == b'/') {
        Some(i) => &p[..i],
        None => &p[..0],
    }
}

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
    /// (derived from the listings: see `Recorder::deferred`).
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

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Run one scan of `root` in `mode` under `script`, on `client`.
pub fn run_once(
    client: &Client,
    root: &[u8],
    opts: &ScanOpts,
    script: &Script,
    timeout: Duration,
) -> Result<(RunRecord, Recorder), String> {
    let (req, rx) = client
        .start_scan(root.to_vec(), opts.clone())
        .map_err(|e| format!("StartScan: {e}"))?;
    let t0 = Instant::now();
    let mut rec = Recorder::new(t0);
    // Per click: (seq, sent at).
    let mut sent: Vec<Option<(u32, Duration)>> = vec![None; script.clicks.len()];
    let mut seq = 0u32;
    let deadline = t0 + timeout;
    while !rec.is_done() {
        let now = Instant::now();
        if now >= deadline {
            let _ = client.cancel(req);
            return Err(format!("the scan took longer than {timeout:?}"));
        }
        // Wake for the next click that is due and not yet sent.
        let next = script
            .clicks
            .iter()
            .zip(&sent)
            .filter(|(_, s)| s.is_none())
            .map(|(c, _)| t0 + c.at)
            .min();
        let wait = next
            .map_or(deadline, |n| n.min(deadline))
            .saturating_duration_since(now)
            .max(Duration::from_millis(1));
        match rx.recv_timeout(wait) {
            Ok(msg) => rec.on_msg(req, &msg, Instant::now())?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(client
                    .closed_reason()
                    .unwrap_or_else(|| "the scan's channel closed".into()))
            }
        }
        let now = Instant::now();
        for (c, s) in script.clicks.iter().zip(sent.iter_mut()) {
            if s.is_some() || now < t0 + c.at || rec.is_done() {
                continue;
            }
            if let Some(dir) = rec.dir(&c.path) {
                seq += 1;
                client
                    .focus(req, dir, seq)
                    .map_err(|e| format!("Focus: {e}"))?;
                *s = Some((seq, rec.elapsed(Instant::now())));
            }
        }
    }
    let end = rec.end.expect("done");
    if end.outcome != Outcome::Complete {
        return Err(format!("the scan ended {:?}", end.outcome));
    }
    let clicks = script
        .clicks
        .iter()
        .zip(&sent)
        .map(|(c, s)| {
            let dir = rec.dir(&c.path);
            let done = dir.and_then(|d| rec.done_at(d));
            ClickRecord {
                path: String::from_utf8_lossy(&c.path).into_owned(),
                at_ms: ms(c.at),
                sent_ms: s.map(|s| ms(s.1)),
                wait_ms: done.map(|d| ms(d.saturating_sub(c.at))),
                settled_ms: s
                    .and_then(|(seq, _)| rec.settled.get(&seq))
                    .map(|t| ms(t.saturating_sub(c.at))),
            }
        })
        .collect();
    let record = RunRecord {
        script: script.kind.name(),
        mode: mode_name(opts.mode),
        run: 0,
        warmup: false,
        total_ms: ms(end.at),
        daemon_ms: end.elapsed_ms,
        first_listing_ms: rec.first_listing.map(ms),
        skeleton_ms: rec.skeleton.map(|s| ms(s.at)),
        skeleton_levels: rec.skeleton.map(|s| s.levels),
        skeleton_listed: rec.skeleton.map(|s| s.listed),
        clicks,
        opens: rec.listed + rec.deferred,
        listed: rec.listed,
        deferred: rec.deferred,
        stats: rec.entries,
        bytes: rec.bytes,
        frames: rec.frames,
        events: rec.events,
        files: end.totals.files,
        dirs: end.totals.dirs,
        apparent: end.totals.apparent,
        disk: end.totals.disk,
        errors: end.totals.errors,
    };
    Ok((record, rec))
}

/// How to run the benchmark.
#[derive(Debug, Clone)]
pub struct Config {
    /// The scan root on the phone.
    pub root: Vec<u8>,
    /// Measured runs per script and mode.
    pub runs: usize,
    /// Unmeasured runs per mode before the measured ones (the first scan
    /// also chooses the scripts' folders).
    pub warmup: usize,
    pub scripts: Vec<ScriptKind>,
    pub workers: Option<u8>,
    /// Seed of script (c).
    pub seed: u64,
    /// Longest one scan may take.
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            root: b"/sdcard".to_vec(),
            runs: 10,
            warmup: 1,
            scripts: vec![ScriptKind::A, ScriptKind::B, ScriptKind::C],
            workers: None,
            seed: 1,
            timeout: Duration::from_secs(600),
        }
    }
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

fn opts_for(cfg: &Config, mode: ScanMode) -> ScanOpts {
    ScanOpts {
        workers: cfg.workers,
        mode,
        ..ScanOpts::default()
    }
}

/// Run the whole benchmark on `client`. `log` gets one line per scan.
pub fn run(
    client: &Client,
    device: &str,
    cfg: &Config,
    log: &mut dyn FnMut(&str),
) -> Result<Report, String> {
    let modes = [ScanMode::Focus, ScanMode::Plain];
    let none = Script {
        kind: ScriptKind::A,
        clicks: Vec::new(),
    };
    // The reference scan: complete, so every folder's size is known.
    let (first, reference_scan) = run_once(
        client,
        &cfg.root,
        &opts_for(cfg, ScanMode::Focus),
        &none,
        cfg.timeout,
    )?;
    let reference = Reference::from_scan(&reference_scan);
    let root = String::from_utf8_lossy(reference_scan.root_path().unwrap_or(&cfg.root)).into();
    let mut records = vec![RunRecord {
        warmup: true,
        ..first
    }];
    log(&format!(
        "reference scan: {} files, {} dirs, {:.0} ms",
        records[0].files, records[0].dirs, records[0].total_ms
    ));
    let scripts: Vec<Script> = cfg
        .scripts
        .iter()
        .map(|&k| reference.script(k, cfg.seed))
        .collect();
    for w in 0..cfg.warmup {
        for &mode in &modes {
            // The reference scan was the first focus warm-up.
            if w == 0 && mode == ScanMode::Focus {
                continue;
            }
            let (r, _) = run_once(client, &cfg.root, &opts_for(cfg, mode), &none, cfg.timeout)?;
            log(&format!(
                "warm-up {}: {:.0} ms",
                mode_name(mode),
                r.total_ms
            ));
            records.push(RunRecord { warmup: true, ..r });
        }
    }
    for i in 0..cfg.runs {
        for script in &scripts {
            // Interleaved A/B, alternating which goes first.
            let order = if i % 2 == 0 {
                modes
            } else {
                [modes[1], modes[0]]
            };
            for mode in order {
                let (r, _) =
                    run_once(client, &cfg.root, &opts_for(cfg, mode), script, cfg.timeout)?;
                log(&format!(
                    "run {}/{} script {} {}: {:.0} ms",
                    i + 1,
                    cfg.runs,
                    script.kind.name(),
                    mode_name(mode),
                    r.total_ms
                ));
                records.push(RunRecord { run: i + 1, ..r });
            }
        }
    }
    let summary = crate::summary::summarise(&records);
    let criteria = crate::summary::criteria(&records);
    Ok(Report {
        root,
        device: device.to_string(),
        daemon_build: client.welcome().daemon_build.clone(),
        workers: cfg.workers.unwrap_or(client.welcome().workers),
        runs: cfg.runs,
        seed: cfg.seed,
        scripts: scripts
            .iter()
            .map(|s| ScriptSetup {
                script: s.kind.name(),
                clicks: s
                    .clicks
                    .iter()
                    .map(|c| (ms(c.at), String::from_utf8_lossy(&c.path).into_owned()))
                    .collect(),
            })
            .collect(),
        records,
        summary,
        criteria,
    })
}

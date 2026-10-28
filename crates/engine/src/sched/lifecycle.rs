//! Starting a scan, its accessors, the passing of time and how it ends.

use std::collections::{HashMap, HashSet, VecDeque};

use socketsweep_wire::{
    Limits, Outcome, ScanEvent, ScanMode, ScanOpts, TierCfg, Totals, ROOT_DIR, STALL_THRESHOLD,
};

use super::{DirRec, Micros, ScanState, WorkerSlot, NONE, PROGRESS_INTERVAL};
use crate::fs::RootInfo;

/// Resolved scan options.
#[derive(Debug, Clone)]
pub(crate) struct Cfg {
    pub workers: u8,
    pub max_depth: u8,
    pub skel_budget: Micros,
    pub skel_min: u8,
    pub skel_max: u8,
    pub one_fs: bool,
    pub tiers: [TierCfg; 3],
    pub ramp: Micros,
    /// [`ScanMode::Plain`]: no skeleton phase, `Full` tasks only, and the
    /// focus does not steer the workers.
    pub plain: bool,
}

impl ScanState {
    /// Start a scan of `root` with `workers` workers (clamped to
    /// `1..=Limits::max_workers`). Emits [`ScanEvent::Started`], which
    /// introduces [`ROOT_DIR`].
    pub fn new(root: RootInfo, opts: &ScanOpts, workers: u8, now: Micros) -> Self {
        let limits = Limits::default();
        let workers = workers.clamp(1, limits.max_workers);
        let cfg = Cfg {
            workers,
            max_depth: opts.max_depth.min(limits.max_depth).min(u16::from(u8::MAX)) as u8,
            skel_budget: u64::from(opts.skeleton_budget_ms) * 1000,
            skel_min: opts.skeleton_min_depth,
            skel_max: opts.skeleton_max_depth,
            one_fs: opts.one_filesystem,
            tiers: opts.tiers,
            ramp: u64::from(opts.ramp_ms) * 1000,
            plain: opts.mode == ScanMode::Plain,
        };
        // Plain mode never has a skeleton phase: with no buckets, every
        // pending directory is read with `Full`, and `SkeletonReady` is never
        // sent.
        let skeleton = (!cfg.plain).then(|| vec![VecDeque::from([ROOT_DIR])]);
        let mut root_rec = DirRec::new(NONE, Box::default(), 0);
        root_rec.pending_desc = 1;
        let mut s = ScanState {
            cfg,
            root_path: root.path.clone(),
            root_dev: root.dev,
            dirs: vec![root_rec],
            deferred: HashMap::new(),
            workers: vec![WorkerSlot::default(); usize::from(workers)],
            focus: None,
            last_seq: None,
            skeleton,
            skeleton_ready_at: None,
            unlisted_by_depth: vec![1],
            unlisted: 1,
            listed: 0,
            big: Vec::new(),
            links: HashSet::new(),
            files_seen: 0,
            disk_seen: 0,
            start: now,
            last_progress: now,
            progress_interval: PROGRESS_INTERVAL,
            outcome: None,
            out: Vec::new(),
        };
        s.out.push(ScanEvent::Started {
            root: root.path,
            dev: root.dev,
            workers,
        });
        s
    }

    /// Change how often [`ScanState::tick`] emits `Progress` (0 = never).
    pub fn set_progress_interval(&mut self, every: Micros) {
        self.progress_interval = every;
    }

    /// Remove and return the events produced so far, in order. The caller
    /// must hand them to the sink before releasing whatever lock guards this
    /// state.
    pub fn take_events(&mut self) -> Vec<ScanEvent> {
        std::mem::take(&mut self.out)
    }

    pub fn has_events(&self) -> bool {
        !self.out.is_empty()
    }

    /// Workers this scan uses.
    pub fn workers(&self) -> u8 {
        self.cfg.workers
    }

    /// How the scan ended, once [`ScanEvent::Finished`] has been emitted.
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// The root's totals so far (final once the scan is complete).
    pub fn root_totals(&self) -> Totals {
        self.dirs[0].totals
    }

    /// The mode this scan runs in.
    pub fn mode(&self) -> ScanMode {
        if self.cfg.plain {
            ScanMode::Plain
        } else {
            ScanMode::Focus
        }
    }

    /// When the skeleton phase ended, if it has (never in plain mode).
    pub fn skeleton_ready_at(&self) -> Option<Micros> {
        self.skeleton_ready_at
    }

    /// Directories known so far (the next new id).
    pub fn known(&self) -> u32 {
        self.dirs.len() as u32
    }

    /// Workers with a task in flight.
    pub fn busy(&self) -> usize {
        self.workers.iter().filter(|w| w.task.is_some()).count()
    }

    /// Advance time: ends the skeleton phase when due and emits `Progress`
    /// every [`PROGRESS_INTERVAL`]. Drivers call it periodically even when
    /// no task finishes (the pool's ticker thread, the simulator's tick
    /// event); every other input calls it too.
    pub fn tick(&mut self, now: Micros) {
        if self.is_finished() {
            return;
        }
        self.check_skeleton(now);
        if self.progress_interval > 0
            && now.saturating_sub(self.last_progress) >= self.progress_interval
        {
            self.last_progress = now;
            self.emit_progress(now);
        }
    }

    pub(crate) fn elapsed_ms(&self, now: Micros) -> u32 {
        (now.saturating_sub(self.start) / 1000).min(u64::from(u32::MAX)) as u32
    }

    fn emit_progress(&mut self, now: Micros) {
        let stall = STALL_THRESHOLD.as_micros() as u64;
        let stalled = self
            .workers
            .iter()
            .filter_map(|w| w.task)
            .filter(|t| now.saturating_sub(t.since) > stall)
            .map(|t| (t.dir, (now.saturating_sub(t.since) / 1000) as u32))
            .collect();
        let active = self.tier_active();
        self.out.push(ScanEvent::Progress {
            listed: self.listed,
            known: self.dirs.len() as u32,
            files: self.files_seen,
            disk: self.disk_seen,
            pending: self.dirs[0].pending_desc,
            alloc: active.map(|a| a.min(255) as u8),
            stalled,
        });
    }

    /// `ClientMsg::Cancel`: emits `Finished { Cancelled }` with the totals so
    /// far. Results still in flight are discarded when applied.
    pub fn cancel(&mut self, now: Micros) {
        if self.is_finished() {
            return;
        }
        self.finish(Outcome::Cancelled, now);
    }

    pub(crate) fn finish(&mut self, outcome: Outcome, now: Micros) {
        self.outcome = Some(outcome);
        self.skeleton = None;
        self.out.push(ScanEvent::Finished {
            outcome,
            totals: self.dirs[0].totals,
            elapsed_ms: self.elapsed_ms(now),
        });
    }
}

//! The directory table, the focus rule and choosing work.
//!
//! [`ScanState`] is a pure state machine: no threads, no clock (every method
//! that needs the time takes `now`, in microseconds since an arbitrary
//! origin), no I/O. Inputs are picks, task results, focus changes, prunes and
//! ticks; outputs are wire [`ScanEvent`]s, appended to an internal queue at
//! the moment the state changes and drained with
//! [`ScanState::take_events`]. The threaded [`pool`](crate::pool) drives it,
//! and tests can drive it directly.
//!
//! Applying results, the completion cascade and prunes live in
//! [`aggregate`](crate::aggregate).

use std::collections::{HashMap, HashSet, VecDeque};

use socketsweep_wire::{DirId, Limits, Outcome, ScanEvent, ScanMode, ScanOpts, ROOT_DIR};

use crate::fs::RootInfo;

mod drain;
mod focus;
mod limits;
mod pick;
mod status;
mod table;
mod task;
#[cfg(test)]
mod tests;

pub use limits::{
    BIG_JOB_ENTRIES, MAX_DEPTH, PROGRESS_INTERVAL, SKELETON_BUDGET, SKELETON_MAX_DEPTH,
    SKELETON_MIN_DEPTH,
};
pub(crate) use table::{DeferredEnt, DirRec, Focus, Own, Running, St, StatSt, WorkerSlot};
pub use task::{Pick, Task, TaskKind, TaskOutput};

/// Microseconds, on whatever clock the driver uses.
pub type Micros = u64;

/// "No directory" in the table's links.
pub(crate) const NONE: DirId = DirId::MAX;

/// The scheduler and aggregator for one scan.
#[derive(Debug)]
pub struct ScanState {
    pub(crate) workers_n: u8,
    /// [`ScanMode::Plain`]: no skeleton phase, `Full` tasks only, and the
    /// focus does not steer the workers.
    pub(crate) plain: bool,
    pub(crate) root_path: Vec<u8>,
    pub(crate) root_dev: u64,
    pub(crate) dirs: Vec<DirRec>,
    pub(crate) deferred: HashMap<DirId, Vec<DeferredEnt>>,
    pub(crate) workers: Vec<WorkerSlot>,
    pub(crate) focus: Option<Focus>,
    pub(crate) last_seq: Option<u32>,
    /// Per-depth FIFO buckets of `Pending` directories while the skeleton
    /// phase lasts (lazily cleaned).
    pub(crate) skeleton: Option<Vec<VecDeque<DirId>>>,
    pub(crate) skeleton_ready_at: Option<Micros>,
    /// `Pending` + `InFlight` directories per depth.
    pub(crate) unlisted_by_depth: Vec<u32>,
    pub(crate) unlisted: u32,
    pub(crate) listed: u32,
    /// Directories whose deferred stats are a big job, with their size.
    pub(crate) big: Vec<(u32, DirId)>,
    pub(crate) links: HashSet<(u64, u64)>,
    pub(crate) start: Micros,
    pub(crate) last_progress: Micros,
    pub(crate) progress_interval: Micros,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) out: Vec<ScanEvent>,
}

impl ScanState {
    /// Start a scan of `root` with `workers` workers (clamped to
    /// `1..=Limits::max_workers`). Emits [`ScanEvent::Started`], which
    /// introduces [`ROOT_DIR`].
    pub fn new(root: RootInfo, opts: &ScanOpts, workers: u8, now: Micros) -> Self {
        let workers = workers.clamp(1, Limits::default().max_workers);
        let plain = opts.mode == ScanMode::Plain;
        // With no buckets every pending directory is read with `Full` and
        // `SkeletonReady` is never sent.
        let skeleton = (!plain).then(|| vec![VecDeque::from([ROOT_DIR])]);
        let mut s = ScanState {
            workers_n: workers,
            plain,
            root_path: root.path.clone(),
            root_dev: root.dev,
            dirs: vec![DirRec::new(NONE, Box::default(), 0)],
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

    /// Advance time: ends the skeleton phase when due and emits `Progress`
    /// every [`PROGRESS_INTERVAL`]. Drivers call it periodically even when
    /// no task finishes (the pool's ticker thread); every other input calls
    /// it too.
    pub fn tick(&mut self, now: Micros) {
        if self.is_finished() {
            return;
        }
        self.check_skeleton(now);
        if self.progress_interval > 0
            && now.saturating_sub(self.last_progress) >= self.progress_interval
        {
            self.last_progress = now;
            self.out.push(ScanEvent::Progress {
                listed: self.listed,
                known: self.dirs.len() as u32,
            });
        }
    }

    pub(crate) fn elapsed_ms(&self, now: Micros) -> u32 {
        (now.saturating_sub(self.start) / 1000).min(u64::from(u32::MAX)) as u32
    }

    /// Ends the skeleton phase when the frontier holds at least 4 × W
    /// directories and the depth reached is at least [`SKELETON_MIN_DEPTH`],
    /// or the depth reached is [`SKELETON_MAX_DEPTH`], or the budget ran out.
    /// "Depth reached" is the shallowest depth that still has an unlisted
    /// directory (every level above it is listed); "frontier" counts the
    /// unlisted directories.
    fn check_skeleton(&mut self, now: Micros) {
        if self.skeleton.is_none() {
            return;
        }
        let end = match self.unlisted_by_depth.iter().position(|&n| n > 0) {
            None => true,
            Some(depth) => {
                depth >= SKELETON_MAX_DEPTH
                    || (self.unlisted >= 4 * u32::from(self.workers_n)
                        && depth >= SKELETON_MIN_DEPTH)
                    || now.saturating_sub(self.start) >= SKELETON_BUDGET
            }
        };
        if end {
            self.skeleton = None;
            self.skeleton_ready_at = Some(now);
            self.out.push(ScanEvent::SkeletonReady {
                listed: self.listed,
                frontier: self.unlisted,
                elapsed_ms: self.elapsed_ms(now),
            });
        }
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

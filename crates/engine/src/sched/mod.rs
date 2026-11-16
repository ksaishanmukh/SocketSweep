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

use socketsweep_wire::{DirId, Limits, Outcome, ScanEvent, ScanMode, ScanOpts, Totals, ROOT_DIR};

use crate::fs::RootInfo;

mod focus;
mod pick;
mod table;
mod task;
#[cfg(test)]
mod tests;

pub(crate) use table::{DeferredEnt, DirRec, Focus, Own, Running, St, StatSt, WorkerSlot};
pub use task::{Pick, Task, TaskKind, TaskOutput};

/// Microseconds, on whatever clock the driver uses.
pub type Micros = u64;

/// "No directory" in the table's links.
pub(crate) const NONE: DirId = DirId::MAX;

/// A deferred stat job of at least this many entries is a "big job": it is
/// handed out before other work on its side of the focus, so one huge folder
/// does not become the scan's tail.
pub const BIG_JOB_ENTRIES: u32 = 2048;

/// How often [`ScanState::tick`] emits [`ScanEvent::Progress`].
pub const PROGRESS_INTERVAL: Micros = 500_000;

/// Directories deeper than this below the root are not walked; they are
/// counted as entries and their parent gets [`Totals::DEPTH_LIMIT`]. Bounds
/// the chain of parents every counter update walks.
pub const MAX_DEPTH: u8 = 64;

/// The skeleton phase ends after this long at the latest.
pub const SKELETON_BUDGET: Micros = 1_000_000;
/// The skeleton phase does not end on frontier size above this depth.
pub const SKELETON_MIN_DEPTH: usize = 2;
/// The skeleton phase ends once every level above this depth is listed.
pub const SKELETON_MAX_DEPTH: usize = 4;

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
        self.workers_n
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
        if self.plain {
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

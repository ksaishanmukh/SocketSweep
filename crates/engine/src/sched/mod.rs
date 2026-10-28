//! The directory table, tiers, allocation and choosing work.
//!
//! [`ScanState`] is a pure state machine: no threads, no clock (every method
//! that needs the time takes `now`, in microseconds since an arbitrary
//! origin), no I/O. Inputs are picks, task results, focus changes, prunes and
//! ticks; outputs are wire [`ScanEvent`]s, appended to an internal queue at
//! the moment the state changes and drained with
//! [`ScanState::take_events`]. The threaded [`pool`](crate::pool) and a
//! simulator can both drive it.
//!
//! Applying results, the completion cascade and prunes live in
//! [`aggregate`](crate::aggregate).

use std::collections::{HashMap, HashSet, VecDeque};

use socketsweep_wire::{DirId, Outcome, ScanEvent};

mod anchor;
mod focus;
mod lifecycle;
mod pick;
mod skeleton;
mod table;
mod task;
mod tiers;
mod validate;

pub(crate) use lifecycle::Cfg;
pub(crate) use table::{DeferredEnt, DirRec, Focus, Own, Running, St, StatSt, WorkerSlot};
pub use task::{Pick, Task, TaskKind, TaskOutput};
pub use tiers::TierSnapshot;

/// Microseconds, on whatever clock the driver uses.
pub type Micros = u64;

/// "No directory" in the table's links.
pub(crate) const NONE: DirId = DirId::MAX;

/// A deferred stat job of at least this many entries is a "big job": it is
/// handed out before anything else in its tier, so the scan's tail stays
/// short.
pub const BIG_JOB_ENTRIES: u32 = 2048;

/// How often [`ScanState::tick`] emits [`ScanEvent::Progress`].
pub const PROGRESS_INTERVAL: Micros = 500_000;

/// The scheduler and aggregator for one scan.
#[derive(Debug)]
pub struct ScanState {
    pub(crate) cfg: Cfg,
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
    pub(crate) files_seen: u64,
    pub(crate) disk_seen: u64,
    pub(crate) start: Micros,
    pub(crate) last_progress: Micros,
    pub(crate) progress_interval: Micros,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) out: Vec<ScanEvent>,
}

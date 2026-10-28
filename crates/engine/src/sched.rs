//! `sched`: the directory table, tiers, allocation and choosing work.
//!
//! [`ScanState`] is a pure state machine: no threads, no clock (every method
//! that needs the time takes `now`, in microseconds since an arbitrary
//! origin), no I/O. Inputs are picks, task results, focus changes, prunes and
//! ticks; outputs are wire [`ScanEvent`]s, appended to an internal queue at
//! the moment the state changes and drained with
//! [`ScanState::take_events`]. The threaded [`pool`](crate::pool) and the
//! simulator in `socketsweep-sim` both drive it.
//!
//! The aggregation half (applying results, the completion cascade, prunes)
//! is in [`aggregate`](crate::aggregate).

use std::collections::{HashMap, HashSet, VecDeque};

use socketsweep_wire::{
    DirId, FsOp, Limits, Outcome, ScanEvent, ScanMode, ScanOpts, Tier, TierCfg, Totals, ROOT_DIR,
    STALL_THRESHOLD,
};

use crate::alloc::{floor_seats, ramped_weight, seats, TierInput};
use crate::fs::{DirRead, Fs, FsError, Interrupt, RootInfo, StatResult};

/// Microseconds, on whatever clock the driver uses.
pub type Micros = u64;

/// "No directory" in the table's links.
pub(crate) const NONE: DirId = DirId::MAX;

/// A deferred stat job of at least this many entries is a "big job": it is
/// handed out before anything else in its tier, so the scan's tail stays
/// short (spec: "the biggest jobs start first").
pub const BIG_JOB_ENTRIES: u32 = 2048;

/// How often [`ScanState::tick`] emits [`ScanEvent::Progress`].
pub const PROGRESS_INTERVAL: Micros = 500_000;

/// What a task does to its directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    /// `getdents` + `d_type` only (skeleton listing); the stats are owed.
    Names,
    /// The deferred stats of a directory listed by `Names`.
    StatDir,
    /// `getdents` and `lstat` together.
    Full,
}

/// A unit of work handed to a worker by [`ScanState::pick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub dir: DirId,
    pub kind: TaskKind,
    /// The tier the task was taken from.
    pub tier: Tier,
    /// The directory's full path.
    pub path: Vec<u8>,
    /// [`TaskKind::StatDir`] only: the names to stat, in order.
    pub names: Vec<Vec<u8>>,
}

impl Task {
    /// Run the task against `fs`. This is the "do task" step of the worker
    /// loop, done without the lock.
    pub fn run<F: Fs + ?Sized>(&self, fs: &F, intr: Interrupt<'_>) -> TaskOutput {
        match self.kind {
            TaskKind::Names => fs.read_names(&self.path, intr).into(),
            TaskKind::Full => fs.list_full(&self.path, intr).into(),
            TaskKind::StatDir => match fs.stat_all(&self.path, &self.names, intr) {
                Ok(v) => TaskOutput::Stats(v),
                Err(FsError::Failed { op, errno }) => TaskOutput::Failed { op, errno },
                Err(FsError::Interrupted) => TaskOutput::Interrupted,
            },
        }
    }
}

/// The result of a [`Task`], given back to [`ScanState::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutput {
    /// `Names` or `Full`.
    Listing(DirRead),
    /// `StatDir`: one result per name, in the task's order.
    Stats(Vec<StatResult>),
    Failed {
        op: FsOp,
        errno: i32,
    },
    /// The interrupt was raised (the scan was cancelled).
    Interrupted,
}

impl From<Result<DirRead, FsError>> for TaskOutput {
    fn from(r: Result<DirRead, FsError>) -> Self {
        match r {
            Ok(d) => TaskOutput::Listing(d),
            Err(FsError::Failed { op, errno }) => TaskOutput::Failed { op, errno },
            Err(FsError::Interrupted) => TaskOutput::Interrupted,
        }
    }
}

/// What [`ScanState::pick`] tells a worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Run(Task),
    /// Nothing runnable right now: wait for another worker's result.
    Idle,
    /// The scan has finished or was cancelled: exit.
    Exit,
}

/// Where a directory is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum St {
    /// Known (introduced by its parent's listing), not yet read.
    Pending,
    /// A `Names` or `Full` task is reading it.
    InFlight,
    /// Listed by `Names`; its stats are owed.
    NamesOnly,
    /// Listed and stat'ed; waiting for its children (or its parent's stats).
    Listed,
    /// `SubtreeDone` sent.
    Done,
    /// Deleted during the scan (this or an ancestor).
    Pruned,
}

/// The deferred-stat job of a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatSt {
    Pending,
    InFlight,
    /// Done, or never needed (`Full` listing).
    Done,
}

/// What a directory's own entry contributed to its parent's totals, so a
/// prune can take it out again.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Own {
    /// The parent has applied this entry's stat.
    pub counted: bool,
    /// ... and it failed.
    pub err: bool,
    pub apparent: u64,
    pub disk: u64,
}

/// One row of the directory table. Children of a directory get consecutive
/// ids when it is listed, so they are `first_child .. first_child +
/// n_children` (the spec's sibling list, without the links).
#[derive(Debug, Clone)]
pub(crate) struct DirRec {
    pub parent: DirId,
    pub name: Box<[u8]>,
    pub depth: u8,
    pub first_child: DirId,
    pub n_children: u32,
    pub state: St,
    pub stat: StatSt,
    /// Runnable tasks in this subtree, including this directory's own.
    pub pending_desc: u32,
    /// Workers currently running a task in this subtree.
    pub active_desc: u16,
    /// Child directories not yet `Done` (pruned ones are subtracted).
    pub open_children: u32,
    /// Sums over everything strictly below this directory.
    pub totals: Totals,
    pub own: Own,
}

impl DirRec {
    pub(crate) fn new(parent: DirId, name: Box<[u8]>, depth: u8) -> Self {
        DirRec {
            parent,
            name,
            depth,
            first_child: NONE,
            n_children: 0,
            state: St::Pending,
            stat: StatSt::Done,
            pending_desc: 1,
            active_desc: 0,
            open_children: 0,
            totals: Totals::default(),
            own: Own::default(),
        }
    }

    pub(crate) fn children(&self) -> std::ops::Range<DirId> {
        if self.n_children == 0 {
            0..0
        } else {
            self.first_child..self.first_child + self.n_children
        }
    }
}

/// One entry of a `Names` listing whose stat is owed.
#[derive(Debug, Clone)]
pub(crate) struct DeferredEnt {
    /// Moved into the `StatDir` task when it is handed out.
    pub name: Vec<u8>,
    /// Already stat'ed because of `DT_UNKNOWN`.
    pub early: Option<StatResult>,
    /// The child's id when this entry is a directory that is walked.
    pub child: DirId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Running {
    pub dir: DirId,
    pub kind: TaskKind,
    pub since: Micros,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkerSlot {
    pub task: Option<Running>,
    pub prev_tier: Option<Tier>,
    /// The subtree the worker last descended in, or [`NONE`].
    pub anchor: Option<DirId>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Focus {
    pub seq: u32,
    pub dir: DirId,
    pub since: Micros,
    pub settled_sent: bool,
}

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

/// A snapshot of the allocation, for progress reports and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TierSnapshot {
    /// Workers running a task in each tier.
    pub active: [u32; 3],
    /// Runnable tasks waiting in each tier.
    pub pending: [u32; 3],
    /// The D'Hondt seats each tier should have.
    pub target: [u8; 3],
    /// Each tier's floor.
    pub floor: [u8; 3],
}

/// Choice of where to look for a task inside a tier.
#[derive(Debug, Clone, Copy)]
enum Anchor {
    /// Take this directory's own task only (its subtree straddles tiers).
    Own(DirId),
    /// Descend in this subtree, which lies wholly in the tier.
    Sub(DirId),
}

/// The scheduler and aggregator for one scan. See the module docs.
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

    /// The full path of a known directory.
    pub fn path_of(&self, dir: DirId) -> Vec<u8> {
        let mut parts: Vec<&[u8]> = Vec::new();
        let mut x = dir;
        while x != ROOT_DIR {
            let r = &self.dirs[x as usize];
            parts.push(&r.name);
            x = r.parent;
        }
        let mut p = self.root_path.clone();
        for name in parts.iter().rev() {
            if p.last() != Some(&b'/') {
                p.push(b'/');
            }
            p.extend_from_slice(name);
        }
        p
    }

    // ── Focus ───────────────────────────────────────────────────────────────

    /// `ClientMsg::Focus`. A `seq` not above the last one seen (for `Focus`
    /// or `ClearFocus`) is stale and ignored, as is an unknown or pruned
    /// directory (its `seq` is still used up). Returns whether the focus
    /// changed. Emits [`ScanEvent::FocusState`] at once, `settled` if the
    /// folder is already measured.
    pub fn set_focus(&mut self, seq: u32, dir: DirId, now: Micros) -> bool {
        if self.is_finished() || self.last_seq.is_some_and(|l| seq <= l) {
            return false;
        }
        self.last_seq = Some(seq);
        let Some(rec) = self.dirs.get(dir as usize) else {
            return false;
        };
        if rec.state == St::Pruned {
            return false;
        }
        let done = rec.state == St::Done;
        // Re-opening the folder already open keeps its dwell.
        let since = match self.focus {
            Some(f) if f.dir == dir => f.since,
            _ => now,
        };
        self.focus = Some(Focus {
            seq,
            dir,
            since,
            settled_sent: done,
        });
        self.out.push(ScanEvent::FocusState {
            seq,
            dir,
            settled: done,
        });
        self.tick(now);
        true
    }

    /// `ClientMsg::ClearFocus`: schedule as a plain parallel walk. Stale
    /// `seq`s are ignored as for [`ScanState::set_focus`].
    pub fn clear_focus(&mut self, seq: u32, now: Micros) -> bool {
        if self.is_finished() || self.last_seq.is_some_and(|l| seq <= l) {
            return false;
        }
        self.last_seq = Some(seq);
        self.focus = None;
        self.tick(now);
        true
    }

    /// The focused folder F and its parent P, when the focus is below the
    /// root. Focus at the root behaves as no focus: everything is Open. In
    /// plain mode the focus is only acknowledged (`FocusState`), never used
    /// for scheduling, so this is always `None` there.
    pub(crate) fn focus_fp(&self) -> Option<(DirId, DirId)> {
        if self.cfg.plain {
            return None;
        }
        let f = self.focus?;
        if f.dir == ROOT_DIR {
            None
        } else {
            Some((f.dir, self.dirs[f.dir as usize].parent))
        }
    }

    /// Whether `d` is `a` or below it.
    pub(crate) fn within(&self, mut d: DirId, a: DirId) -> bool {
        let ad = self.dirs[a as usize].depth;
        while self.dirs[d as usize].depth > ad {
            d = self.dirs[d as usize].parent;
        }
        d == a
    }

    pub(crate) fn tier_of(&self, d: DirId) -> Tier {
        match self.focus_fp() {
            None => Tier::Open,
            Some((f, p)) => {
                if self.within(d, f) {
                    Tier::Open
                } else if self.within(d, p) {
                    Tier::Near
                } else {
                    Tier::Back
                }
            }
        }
    }

    fn tier_pending(&self) -> [u32; 3] {
        let root = self.dirs[0].pending_desc;
        match self.focus_fp() {
            None => [root, 0, 0],
            Some((f, p)) => {
                let pf = self.dirs[f as usize].pending_desc;
                let pp = self.dirs[p as usize].pending_desc;
                [pf, pp - pf, root - pp]
            }
        }
    }

    fn tier_active(&self) -> [u32; 3] {
        let mut a = [0u32; 3];
        for w in &self.workers {
            if let Some(t) = w.task {
                a[self.tier_of(t.dir).index()] += 1;
            }
        }
        a
    }

    fn floors(&self) -> [u8; 3] {
        let w = self.cfg.workers;
        self.cfg.tiers.map(|t| t.floor_for(w))
    }

    /// The current allocation.
    pub fn tiers(&self, now: Micros) -> TierSnapshot {
        let active = self.tier_active();
        let pending = self.tier_pending();
        TierSnapshot {
            active,
            pending,
            target: self.targets(now, active, pending),
            floor: self.floors(),
        }
    }

    fn tier_inputs(&self, now: Micros, active: [u32; 3], pending: [u32; 3]) -> [TierInput; 3] {
        let floors = self.floors();
        let mut weights = self.cfg.tiers.map(|t| t.weight);
        if let Some(f) = self.focus {
            weights[0] = ramped_weight(weights[0], now.saturating_sub(f.since), self.cfg.ramp);
        }
        [0, 1, 2].map(|i| TierInput {
            weight: weights[i],
            floor: floors[i],
            cap: self.cfg.tiers[i].cap,
            active: active[i],
            pending: pending[i],
        })
    }

    fn targets(&self, now: Micros, active: [u32; 3], pending: [u32; 3]) -> [u8; 3] {
        seats(self.cfg.workers, &self.tier_inputs(now, active, pending))
    }

    // ── Picking work ────────────────────────────────────────────────────────

    /// Hand worker `w` its next task (spec `pick(worker)`). The worker must
    /// have applied its previous result first; that released its seat.
    /// Congestion is the driver's business: it does not call `pick` while
    /// the sink is congested.
    pub fn pick(&mut self, w: usize, now: Micros) -> Pick {
        self.tick(now);
        if self.is_finished() {
            return Pick::Exit;
        }
        assert!(self.workers[w].task.is_none(), "worker {w} picked twice");
        let pending = self.tier_pending();
        if pending.iter().all(|&p| p == 0) {
            return Pick::Idle;
        }
        let active = self.tier_active();
        let target = self.targets(now, active, pending);
        let floors = floor_seats(self.cfg.workers, &self.tier_inputs(now, active, pending));
        let tier = self.choose_tier(w, active, pending, target, floors);
        let found = self.find_task(w, tier).or_else(|| {
            debug_assert!(false, "tier {tier:?} has pending work but no task");
            Tier::ALL
                .into_iter()
                .filter(|&t| t != tier && pending[t.index()] > 0)
                .find_map(|t| self.find_task(w, t).map(|x| (x, t)))
                .map(|(x, _)| x)
        });
        let Some((dir, kind)) = found else {
            return Pick::Idle;
        };
        let tier = self.tier_of(dir);
        Pick::Run(self.start_task(w, dir, kind, tier, now))
    }

    /// Step 2 of `pick`: floors first (a tier with work below its floor, as
    /// the allocation resolves floors: limited by the workers, back first;
    /// the back floor before any other),
    /// then the largest `target − active`; ties go to the worker's previous
    /// tier, then to the nearer tier. Only tiers with waiting work count.
    fn choose_tier(
        &self,
        w: usize,
        active: [u32; 3],
        pending: [u32; 3],
        target: [u8; 3],
        floors: [u32; 3],
    ) -> Tier {
        let prev = self.workers[w].prev_tier;
        let best = |key: &dyn Fn(usize) -> Option<i64>| -> Option<Tier> {
            let mut best: Option<(i64, bool, Tier)> = None;
            for t in Tier::ALL {
                let i = t.index();
                if pending[i] == 0 {
                    continue;
                }
                let Some(k) = key(i) else { continue };
                let sticky = prev == Some(t);
                let better = match best {
                    None => true,
                    Some((bk, bs, _)) => k > bk || (k == bk && sticky && !bs),
                };
                if better {
                    best = Some((k, sticky, t));
                }
            }
            best.map(|b| b.2)
        };
        // The back floor is the one the scheduler guarantees: it wins outright.
        let b = Tier::Back.index();
        if pending[b] > 0 && active[b] < floors[b] {
            return Tier::Back;
        }
        let below_floor = best(&|i| {
            let f = i64::from(floors[i]);
            let a = i64::from(active[i]);
            (a < f).then_some(f - a)
        });
        below_floor
            .or_else(|| best(&|i| Some(i64::from(target[i]) - i64::from(active[i]))))
            .expect("some tier has pending work")
    }

    /// The tier that does the skeleton walk: Back, or everything when there
    /// is no focus.
    fn skeleton_tier(&self) -> Tier {
        if self.focus_fp().is_none() {
            Tier::Open
        } else {
            Tier::Back
        }
    }

    /// Steps 3 and 4 of `pick`: find a task in `tier`.
    fn find_task(&mut self, w: usize, tier: Tier) -> Option<(DirId, TaskKind)> {
        if let Some(d) = self.take_big(tier) {
            return Some((d, TaskKind::StatDir));
        }
        if self.skeleton.is_some() && tier == self.skeleton_tier() {
            if let Some(d) = self.bucket_pick(tier) {
                return Some((d, TaskKind::Names));
            }
        }
        match self.choose_anchor(w, tier)? {
            Anchor::Own(d) => Some((d, self.own_task(d)?)),
            Anchor::Sub(a) => {
                self.workers[w].anchor = Some(a);
                Some(self.descend(a))
            }
        }
    }

    /// A directory's own runnable task, if any. Outside the skeleton walk a
    /// pending directory is read with `Full`.
    pub(crate) fn own_task(&self, d: DirId) -> Option<TaskKind> {
        let r = &self.dirs[d as usize];
        match r.state {
            St::Pending => Some(TaskKind::Full),
            St::NamesOnly if r.stat == StatSt::Pending => Some(TaskKind::StatDir),
            _ => None,
        }
    }

    /// The largest waiting big job in `tier`.
    fn take_big(&mut self, tier: Tier) -> Option<DirId> {
        let dirs = &self.dirs;
        self.big.retain(|&(_, d)| {
            dirs[d as usize].state == St::NamesOnly && dirs[d as usize].stat == StatSt::Pending
        });
        let mut best: Option<(u32, DirId)> = None;
        for &(n, d) in &self.big {
            if self.tier_of(d) == tier && best.is_none_or(|(bn, _)| n > bn) {
                best = Some((n, d));
            }
        }
        best.map(|b| b.1)
    }

    /// The shallowest pending directory in `tier` from the skeleton buckets
    /// (FIFO within a depth).
    fn bucket_pick(&mut self, tier: Tier) -> Option<DirId> {
        let all = self.focus_fp().is_none();
        let mut buckets = self.skeleton.take()?;
        let mut found = None;
        'depth: for b in buckets.iter_mut() {
            while b
                .front()
                .is_some_and(|&d| self.dirs[d as usize].state != St::Pending)
            {
                b.pop_front();
            }
            for (i, &d) in b.iter().enumerate() {
                if self.dirs[d as usize].state == St::Pending && (all || self.tier_of(d) == tier) {
                    b.remove(i);
                    found = Some(d);
                    break 'depth;
                }
            }
        }
        self.skeleton = Some(buckets);
        found
    }

    /// Whether `a`'s whole subtree lies in `tier`, so a worker may descend
    /// in it.
    fn anchor_in_tier(&self, a: DirId, tier: Tier) -> bool {
        match self.focus_fp() {
            None => tier == Tier::Open,
            Some((f, p)) => match tier {
                Tier::Open => self.within(a, f),
                Tier::Near => a != p && self.within(a, p) && !self.within(a, f),
                Tier::Back => !self.within(a, p) && !self.within(p, a),
            },
        }
    }

    /// Step 3 of `pick`: keep the previous anchor if it is still in the tier
    /// and has work; otherwise choose by the tier's rule. A tier root (F, P,
    /// or an ancestor of P) with its own task is taken first: its subtree
    /// straddles tiers, and its stats gate its children's completion.
    fn choose_anchor(&self, w: usize, tier: Tier) -> Option<Anchor> {
        if let Some(a) = self.workers[w].anchor {
            let r = &self.dirs[a as usize];
            if r.state != St::Pruned && r.pending_desc > 0 && self.anchor_in_tier(a, tier) {
                return Some(Anchor::Sub(a));
            }
        }
        let own = |d: DirId| self.own_task(d).is_some();
        // (fewest workers, then most work) or (fewest workers, then least work)
        let pick_child =
            |parent: DirId, skip: DirId, most_work: bool, best: &mut Option<(u16, i64, DirId)>| {
                for c in self.dirs[parent as usize].children() {
                    let r = &self.dirs[c as usize];
                    if c == skip || r.pending_desc == 0 || r.state == St::Pruned {
                        continue;
                    }
                    let work = if most_work {
                        -i64::from(r.pending_desc)
                    } else {
                        i64::from(r.pending_desc)
                    };
                    let key = (r.active_desc, work, c);
                    if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                        *best = Some(key);
                    }
                }
            };
        let mut best = None;
        match self.focus_fp() {
            // No focus: everything is Open, scheduled by the Back rule (a
            // plain parallel walk wants the biggest jobs first).
            None => {
                if own(ROOT_DIR) {
                    return Some(Anchor::Own(ROOT_DIR));
                }
                pick_child(ROOT_DIR, NONE, true, &mut best);
            }
            Some((f, p)) => match tier {
                Tier::Open => {
                    if own(f) {
                        return Some(Anchor::Own(f));
                    }
                    pick_child(f, NONE, false, &mut best);
                }
                Tier::Near => {
                    if own(p) {
                        return Some(Anchor::Own(p));
                    }
                    pick_child(p, f, true, &mut best);
                }
                Tier::Back => {
                    let mut cur = p;
                    while cur != ROOT_DIR {
                        let a = self.dirs[cur as usize].parent;
                        if own(a) {
                            return Some(Anchor::Own(a));
                        }
                        pick_child(a, cur, true, &mut best);
                        cur = a;
                    }
                }
            },
        }
        best.map(|b| Anchor::Sub(b.2))
    }

    /// Step 4 of `pick`: depth-first from `a`: a directory's own task first,
    /// else the highest-id child with work.
    fn descend(&self, a: DirId) -> (DirId, TaskKind) {
        let mut x = a;
        loop {
            if let Some(k) = self.own_task(x) {
                return (x, k);
            }
            x = self.dirs[x as usize]
                .children()
                .rev()
                .find(|&c| {
                    self.dirs[c as usize].pending_desc > 0
                        && self.dirs[c as usize].state != St::Pruned
                })
                .expect("pending_desc > 0 implies a task below");
        }
    }

    fn start_task(
        &mut self,
        w: usize,
        dir: DirId,
        kind: TaskKind,
        tier: Tier,
        now: Micros,
    ) -> Task {
        let names = match kind {
            TaskKind::Names | TaskKind::Full => {
                debug_assert_eq!(self.dirs[dir as usize].state, St::Pending);
                self.dirs[dir as usize].state = St::InFlight;
                Vec::new()
            }
            TaskKind::StatDir => {
                debug_assert_eq!(self.dirs[dir as usize].stat, StatSt::Pending);
                self.dirs[dir as usize].stat = StatSt::InFlight;
                self.deferred
                    .get_mut(&dir)
                    .map(|ents| {
                        ents.iter_mut()
                            .filter(|e| e.early.is_none())
                            .map(|e| std::mem::take(&mut e.name))
                            .collect()
                    })
                    .unwrap_or_default()
            }
        };
        self.add_pending(dir, -1);
        self.add_active(dir, 1);
        let slot = &mut self.workers[w];
        slot.task = Some(Running {
            dir,
            kind,
            since: now,
        });
        slot.prev_tier = Some(tier);
        Task {
            dir,
            kind,
            tier,
            path: self.path_of(dir),
            names,
        }
    }

    // ── Counters ────────────────────────────────────────────────────────────

    /// Add `delta` to `pending_desc` of `d` and every ancestor.
    pub(crate) fn add_pending(&mut self, d: DirId, delta: i64) {
        let mut x = d;
        while x != NONE {
            let r = &mut self.dirs[x as usize];
            r.pending_desc = (i64::from(r.pending_desc) + delta) as u32;
            x = r.parent;
        }
    }

    /// Add `delta` to `active_desc` of `d` and every ancestor.
    pub(crate) fn add_active(&mut self, d: DirId, delta: i32) {
        let mut x = d;
        while x != NONE {
            let r = &mut self.dirs[x as usize];
            r.active_desc = (i32::from(r.active_desc) + delta) as u16;
            x = r.parent;
        }
    }

    // ── Time ────────────────────────────────────────────────────────────────

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

    /// Spec "Phase end": the frontier holds at least 4 × W directories and
    /// the depth reached is at least `skeleton_min_depth`, or the budget ran
    /// out, or the depth reached `skeleton_max_depth`. "Depth reached" is the
    /// shallowest depth that still has an unlisted directory (every level
    /// above it is listed); "frontier" counts the unlisted directories.
    pub(crate) fn check_skeleton(&mut self, now: Micros) {
        if self.skeleton.is_none() {
            return;
        }
        let fd = self.unlisted_by_depth.iter().position(|&n| n > 0);
        let end = match fd {
            None => true,
            Some(fd) => {
                let fd = fd as u64;
                fd >= u64::from(self.cfg.skel_max)
                    || (self.unlisted >= 4 * u32::from(self.cfg.workers)
                        && fd >= u64::from(self.cfg.skel_min))
                    || now.saturating_sub(self.start) >= self.cfg.skel_budget
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

    /// Recompute every counter from scratch and compare (tests).
    pub fn validate(&self) -> Result<(), String> {
        let n = self.dirs.len();
        let mut pending = vec![0u32; n];
        let mut active = vec![0u32; n];
        let mut open = vec![0u32; n];
        for (i, r) in self.dirs.iter().enumerate() {
            if r.state == St::Pruned {
                continue;
            }
            let mut own = u32::from(r.state == St::Pending);
            own += u32::from(r.state == St::NamesOnly && r.stat == StatSt::Pending);
            let mut x = i as DirId;
            while x != NONE {
                pending[x as usize] += own;
                x = self.dirs[x as usize].parent;
            }
            if r.parent != NONE && r.state != St::Done {
                open[r.parent as usize] += 1;
            }
        }
        for w in &self.workers {
            if let Some(t) = w.task {
                let mut x = t.dir;
                while x != NONE {
                    active[x as usize] += 1;
                    x = self.dirs[x as usize].parent;
                }
            }
        }
        for (i, r) in self.dirs.iter().enumerate() {
            if r.state == St::Pruned {
                continue;
            }
            if r.pending_desc != pending[i] {
                return Err(format!(
                    "dir {i}: pending_desc {} != {}",
                    r.pending_desc, pending[i]
                ));
            }
            if u32::from(r.active_desc) != active[i] {
                return Err(format!(
                    "dir {i}: active_desc {} != {}",
                    r.active_desc, active[i]
                ));
            }
            let listed = !matches!(r.state, St::Pending | St::InFlight);
            if listed && r.open_children != open[i] {
                return Err(format!(
                    "dir {i}: open_children {} != {}",
                    r.open_children, open[i]
                ));
            }
        }
        let unlisted = self
            .dirs
            .iter()
            .filter(|r| matches!(r.state, St::Pending | St::InFlight))
            .count() as u32;
        if unlisted != self.unlisted {
            return Err(format!("unlisted {} != {unlisted}", self.unlisted));
        }
        Ok(())
    }
}

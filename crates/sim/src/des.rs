//! A discrete-event simulator: a virtual clock and W virtual workers driving
//! [`ScanState`] directly, on a [`SimFs`] with a [`Latency`] model.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use socketsweep_engine::Fs;
use socketsweep_engine::{Interrupt, Micros, Pick, ScanState, TaskKind, TaskOutput};
use socketsweep_wire::{DirId, Outcome, ScanEvent, ScanOpts, Tier, Totals};

use crate::latency::{Latency, Storage};
use crate::simfs::SimFs;

/// One scripted action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open the folder at this path (skipped if not yet introduced).
    Focus(Vec<u8>),
    ClearFocus,
    /// Delete the directory at this path, with a prune hint (skipped if it
    /// has no `DirId` yet).
    Delete(Vec<u8>),
    Cancel,
}

/// Actions at virtual times (microseconds from the scan start).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    pub steps: Vec<(Micros, Action)>,
}

/// Simulator settings.
#[derive(Debug, Clone)]
pub struct SimConfig {
    pub workers: u8,
    pub opts: ScanOpts,
    pub latency: Latency,
    /// Virtual time between ticks.
    pub tick: Micros,
    /// Check the tier properties (Back floor, convergence to targets).
    pub check_tiers: bool,
    /// Run [`ScanState::validate`] every this many steps (0: never).
    pub validate_every: usize,
}

impl SimConfig {
    pub fn standard() -> Self {
        SimConfig {
            workers: 8,
            opts: ScanOpts::default(),
            latency: Latency::STANDARD,
            tick: 10_000,
            check_tiers: true,
            validate_every: 0,
        }
    }
}

/// What happened to one focus action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRecord {
    pub seq: u32,
    pub path: Vec<u8>,
    /// `None`: the folder was not introduced yet, so the action was skipped.
    pub dir: Option<DirId>,
    pub at: Micros,
    pub settled_at: Option<Micros>,
}

/// The result of a simulated v4 scan.
#[derive(Debug, Clone, Default)]
pub struct V4Run {
    pub events: Vec<(Micros, ScanEvent)>,
    pub finished_at: Micros,
    pub outcome: Option<Outcome>,
    pub totals: Totals,
    pub skeleton_at: Option<Micros>,
    /// `SubtreeDone` time per path.
    pub done_at: HashMap<Vec<u8>, Micros>,
    pub focus: Vec<FocusRecord>,
    pub deleted: Vec<Vec<u8>>,
    pub deletes_skipped: usize,
    /// Tier-property violations (empty when `check_tiers` is off).
    pub violations: Vec<String>,
    /// Instants at which the tier-convergence property was checked (after
    /// a ramp), so tests can tell it was exercised.
    pub tier_samples: u64,
    /// Tasks run, per kind: names, statdir, full.
    pub tasks: [u64; 3],
    /// Total work given to the storage service, in microseconds at full
    /// speed.
    pub work: f64,
}

impl V4Run {
    pub fn events(&self) -> impl Iterator<Item = &ScanEvent> {
        self.events.iter().map(|(_, e)| e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    Step(usize),
    Tick,
}

/// Paths of introduced directories, as a host would track them.
#[derive(Debug, Default)]
struct Ids {
    path: Vec<Vec<u8>>,
    by_path: HashMap<Vec<u8>, DirId>,
}

impl Ids {
    fn see(&mut self, ev: &ScanEvent) {
        match ev {
            ScanEvent::Started { root, .. } => {
                self.path = vec![root.clone()];
                self.by_path.insert(root.clone(), 0);
            }
            ScanEvent::Listing { dir, entries, .. } => {
                let base = self.path[*dir as usize].clone();
                for e in entries {
                    if let Some(id) = e.id {
                        let mut p = base.clone();
                        if p.last() != Some(&b'/') {
                            p.push(b'/');
                        }
                        p.extend_from_slice(&e.name);
                        if self.path.len() <= id as usize {
                            self.path.resize(id as usize + 1, Vec::new());
                        }
                        self.path[id as usize] = p.clone();
                        self.by_path.insert(p, id);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Simulate a v4 scan of `fs` under `script`.
pub fn run_v4(fs: &SimFs, cfg: &SimConfig, script: &Script) -> V4Run {
    let root = fs.root(fs.root_path()).expect("sim root");
    let mut st = ScanState::new(root, &cfg.opts, cfg.workers, 0);
    let w = usize::from(st.workers());
    let mut run = V4Run::default();
    let mut ids = Ids::default();
    let mut storage = Storage::new(cfg.latency, w);
    let mut inflight: Vec<Option<(TaskKind, TaskOutput)>> = vec![None; w];
    let mut q: BinaryHeap<Reverse<(Micros, u64, Ev)>> = BinaryHeap::new();
    let mut qseq = 0u64;
    let mut push = |q: &mut BinaryHeap<_>, t: Micros, ev: Ev| {
        qseq += 1;
        q.push(Reverse((t, qseq, ev)));
    };
    for (i, (t, _)) in script.steps.iter().enumerate() {
        push(&mut q, *t, Ev::Step(i));
    }
    push(&mut q, cfg.tick, Ev::Tick);
    let mut steps_left = script.steps.len();
    let mut seq = 0u32;
    let mut now: Micros = 0;
    let mut focus_since: Option<Micros> = None;
    let mut mismatch: Option<(Micros, Micros)> = None;
    let mut n_steps = 0usize;
    let ramp = Micros::from(cfg.opts.ramp_ms) * 1000;

    let record = |run: &mut V4Run, ids: &mut Ids, st: &mut ScanState, now: Micros| {
        for ev in st.take_events() {
            ids.see(&ev);
            match &ev {
                ScanEvent::SubtreeDone { dir, .. } => {
                    run.done_at.insert(ids.path[*dir as usize].clone(), now);
                }
                ScanEvent::FocusState {
                    seq, settled: true, ..
                } => {
                    if let Some(f) = run.focus.iter_mut().find(|f| f.seq == *seq) {
                        f.settled_at.get_or_insert(now);
                    }
                }
                ScanEvent::SkeletonReady { .. } => run.skeleton_at = Some(now),
                ScanEvent::Finished {
                    outcome, totals, ..
                } => {
                    run.outcome = Some(*outcome);
                    run.totals = *totals;
                    run.finished_at = now;
                }
                _ => {}
            }
            run.events.push((now, ev));
        }
    };
    record(&mut run, &mut ids, &mut st, now);

    loop {
        // Hand idle workers work, at this instant. (Indexed: the body
        // also starts the worker's slot in `storage`.)
        #[allow(clippy::needless_range_loop)]
        for wi in 0..w {
            if inflight[wi].is_some() || st.is_finished() {
                continue;
            }
            let before = cfg.check_tiers.then(|| st.tiers(now));
            match st.pick(wi, now) {
                Pick::Run(task) => {
                    if let Some(s) = before {
                        let b = Tier::Back.index();
                        if task.tier != Tier::Back
                            && s.pending[b] > 0
                            && s.active[b] < u32::from(s.floor[b])
                        {
                            run.violations.push(format!(
                                "t={now}: worker {wi} took a {:?} task while Back ({} active, floor {}) had {} waiting",
                                task.tier, s.active[b], s.floor[b], s.pending[b]
                            ));
                        }
                    }
                    let out = task.run(fs, Interrupt::NEVER);
                    let work = cfg.latency.cost(task.kind, &out, task.names.len());
                    run.tasks[match task.kind {
                        TaskKind::Names => 0,
                        TaskKind::StatDir => 1,
                        TaskKind::Full => 2,
                    }] += 1;
                    storage.start(wi, work);
                    inflight[wi] = Some((task.kind, out));
                }
                Pick::Idle | Pick::Exit => {}
            }
            record(&mut run, &mut ids, &mut st, now);
        }
        n_steps += 1;
        if cfg.validate_every > 0 && n_steps.is_multiple_of(cfg.validate_every) {
            if let Err(e) = st.validate() {
                panic!("t={now}: invariant broken: {e}");
            }
        }
        if st.is_finished() {
            break;
        }

        // "Within one task length after the ramp, each tier's active count is
        // within 1 of its target."
        if cfg.check_tiers {
            let settled = focus_since.is_some_and(|t| now >= t + ramp);
            run.tier_samples += u64::from(settled);
            let s = st.tiers(now);
            let off = (0..3).any(|i| s.active[i].abs_diff(u32::from(s.target[i])) > 1);
            if settled && off {
                match mismatch {
                    None => mismatch = Some((now, storage.horizon(now))),
                    Some((since, deadline)) if now > deadline => {
                        run.violations.push(format!(
                            "t={now}: tiers off target since {since}: active {:?} target {:?} pending {:?}",
                            s.active, s.target, s.pending
                        ));
                        mismatch = None;
                    }
                    Some(_) => {}
                }
            } else {
                mismatch = None;
            }
        }

        if storage.is_idle() && steps_left == 0 {
            st.validate().expect("invariants");
            panic!("t={now}: deadlock: no task in flight and the scan is not finished");
        }

        // Advance to the next completion or queued event (completions first
        // on ties).
        let done = storage.next_done(now);
        let next_q = q.peek().map(|Reverse((t, _, _))| *t);
        let take_done = match (done, next_q) {
            (Some((_, td)), Some(tq)) => td <= tq,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if take_done {
            let (wi, t) = done.expect("checked");
            storage.advance(t - now);
            now = t;
            storage.finish(wi);
            let (_, out) = inflight[wi].take().expect("in flight");
            st.apply(wi, out, now);
        } else {
            let Reverse((t, _, ev)) = q.pop().expect("event queue never empties while ticking");
            storage.advance(t - now);
            now = t;
            match ev {
                Ev::Tick => {
                    st.tick(now);
                    push(&mut q, now + cfg.tick, Ev::Tick);
                }
                Ev::Step(i) => {
                    steps_left -= 1;
                    match &script.steps[i].1 {
                        Action::Focus(path) => {
                            seq += 1;
                            let dir = ids.by_path.get(path).copied();
                            run.focus.push(FocusRecord {
                                seq,
                                path: path.clone(),
                                dir,
                                at: now,
                                settled_at: None,
                            });
                            if let Some(d) = dir {
                                if st.set_focus(seq, d, now) {
                                    focus_since = Some(now);
                                    mismatch = None;
                                }
                            }
                        }
                        Action::ClearFocus => {
                            seq += 1;
                            st.clear_focus(seq, now);
                            focus_since = None;
                            mismatch = None;
                        }
                        Action::Delete(path) => match ids.by_path.get(path).copied() {
                            Some(d) if st.prune(d, now) => {
                                assert!(fs.delete(path), "prunable dir exists");
                                run.deleted.push(path.clone());
                            }
                            _ => run.deletes_skipped += 1,
                        },
                        Action::Cancel => st.cancel(now),
                    }
                }
            }
        }
        record(&mut run, &mut ids, &mut st, now);
    }
    run.work = storage.work_done;
    run
}

/// The result of a simulated baseline scan.
#[derive(Debug, Clone, Default)]
pub struct BaselineRun {
    pub finished_at: Micros,
    /// Completion time of each directory's subtree, per path.
    pub done_at: HashMap<Vec<u8>, Micros>,
    pub tasks: u64,
    pub work: f64,
}

/// A jwalk-like engine: W workers, each with its own deque; a worker reads a
/// directory with names and stats together, pushes the subdirectories onto
/// its own deque, and takes the newest from it (LIFO), or steals the oldest
/// from another worker when its own is empty. No focus, no deferred stats.
pub fn run_baseline(fs: &SimFs, workers: u8, lat: Latency, opts: &ScanOpts) -> BaselineRun {
    struct BDir {
        path: Vec<u8>,
        parent: usize,
        depth: u16,
        open: u32,
        listed: bool,
    }
    let w = usize::from(workers.max(1));
    let root = fs.root(fs.root_path()).expect("sim root");
    let mut dirs = vec![BDir {
        path: root.path.clone(),
        parent: usize::MAX,
        depth: 0,
        open: 0,
        listed: false,
    }];
    let mut deques: Vec<std::collections::VecDeque<usize>> = vec![Default::default(); w];
    deques[0].push_back(0);
    let mut storage = Storage::new(lat, w);
    let mut inflight: Vec<Option<(usize, TaskOutput)>> = vec![None; w];
    let mut run = BaselineRun::default();
    let mut now: Micros = 0;
    let mut root_done = false;
    while !root_done {
        for wi in 0..w {
            if inflight[wi].is_some() {
                continue;
            }
            let next = deques[wi]
                .pop_back()
                .or_else(|| (1..w).find_map(|k| deques[(wi + k) % w].pop_front()));
            if let Some(d) = next {
                let out: TaskOutput = fs.list_full(&dirs[d].path, Interrupt::NEVER).into();
                let work = lat.cost(TaskKind::Full, &out, 0);
                storage.start(wi, work);
                run.tasks += 1;
                inflight[wi] = Some((d, out));
            }
        }
        let (wi, t) = storage.next_done(now).expect("baseline deadlock");
        storage.advance(t - now);
        now = t;
        storage.finish(wi);
        let (d, out) = inflight[wi].take().expect("in flight");
        dirs[d].listed = true;
        if let TaskOutput::Listing(read) = out {
            let other_fs = d != 0 && opts.one_filesystem && read.dev != root.dev;
            if !other_fs && dirs[d].depth < opts.max_depth {
                for e in read.entries {
                    if e.kind == socketsweep_wire::Kind::Dir {
                        let mut p = dirs[d].path.clone();
                        p.push(b'/');
                        p.extend_from_slice(&e.name);
                        let id = dirs.len();
                        dirs.push(BDir {
                            path: p,
                            parent: d,
                            depth: dirs[d].depth + 1,
                            open: 0,
                            listed: false,
                        });
                        dirs[d].open += 1;
                        deques[wi].push_back(id);
                    }
                }
            }
        }
        // Completion cascade.
        let mut x = d;
        while dirs[x].listed && dirs[x].open == 0 {
            run.done_at.insert(dirs[x].path.clone(), now);
            if x == 0 {
                root_done = true;
                break;
            }
            let p = dirs[x].parent;
            dirs[p].open -= 1;
            x = p;
        }
    }
    run.finished_at = now;
    run.work = storage.work_done;
    run
}

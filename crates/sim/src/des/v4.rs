//! The v4 simulation loop: [`ScanState`] driven by virtual workers.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use socketsweep_engine::Fs;
use socketsweep_engine::{Interrupt, Micros, Pick, ScanState, TaskKind, TaskOutput};
use socketsweep_wire::{ScanEvent, Tier};

use super::ids::Ids;
use super::{Action, FocusRecord, Script, SimConfig, V4Run};
use crate::latency::Storage;
use crate::simfs::SimFs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    Step(usize),
    Tick,
}

/// Everything the loop mutates, between iterations.
struct V4Sim<'a> {
    fs: &'a SimFs,
    cfg: &'a SimConfig,
    script: &'a Script,
    st: ScanState,
    workers: usize,
    run: V4Run,
    ids: Ids,
    storage: Storage,
    inflight: Vec<Option<(TaskKind, TaskOutput)>>,
    queue: BinaryHeap<Reverse<(Micros, u64, Ev)>>,
    queue_seq: u64,
    steps_left: usize,
    seq: u32,
    now: Micros,
    focus_since: Option<Micros>,
    mismatch: Option<(Micros, Micros)>,
    n_steps: usize,
    ramp: Micros,
}

/// Simulate a v4 scan of `fs` under `script`.
pub fn run_v4(fs: &SimFs, cfg: &SimConfig, script: &Script) -> V4Run {
    let mut sim = V4Sim::new(fs, cfg, script);
    sim.record();
    loop {
        sim.dispatch_idle_workers();
        sim.n_steps += 1;
        if cfg.validate_every > 0 && sim.n_steps.is_multiple_of(cfg.validate_every) {
            if let Err(e) = sim.st.validate() {
                panic!("t={}: invariant broken: {e}", sim.now);
            }
        }
        if sim.st.is_finished() {
            break;
        }
        sim.check_tier_convergence();
        sim.assert_no_deadlock();
        sim.advance();
        sim.record();
    }
    sim.run.work = sim.storage.work_done;
    sim.run
}

impl<'a> V4Sim<'a> {
    fn new(fs: &'a SimFs, cfg: &'a SimConfig, script: &'a Script) -> Self {
        let root = fs.root(fs.root_path()).expect("sim root");
        let st = ScanState::new(root, &cfg.opts, cfg.workers, 0);
        let workers = usize::from(st.workers());
        let mut sim = V4Sim {
            fs,
            cfg,
            script,
            st,
            workers,
            run: V4Run::default(),
            ids: Ids::default(),
            storage: Storage::new(cfg.latency, workers),
            inflight: vec![None; workers],
            queue: BinaryHeap::new(),
            queue_seq: 0,
            steps_left: script.steps.len(),
            seq: 0,
            now: 0,
            focus_since: None,
            mismatch: None,
            n_steps: 0,
            ramp: Micros::from(cfg.opts.ramp_ms) * 1000,
        };
        for (i, (t, _)) in script.steps.iter().enumerate() {
            sim.push(*t, Ev::Step(i));
        }
        sim.push(cfg.tick, Ev::Tick);
        sim
    }

    fn push(&mut self, t: Micros, ev: Ev) {
        self.queue_seq += 1;
        self.queue.push(Reverse((t, self.queue_seq, ev)));
    }

    /// Drain the engine's events into the run.
    fn record(&mut self) {
        let now = self.now;
        for ev in self.st.take_events() {
            self.ids.see(&ev);
            match &ev {
                ScanEvent::SubtreeDone { dir, .. } => {
                    self.run
                        .done_at
                        .insert(self.ids.path[*dir as usize].clone(), now);
                }
                ScanEvent::FocusState {
                    seq, settled: true, ..
                } => {
                    if let Some(f) = self.run.focus.iter_mut().find(|f| f.seq == *seq) {
                        f.settled_at.get_or_insert(now);
                    }
                }
                ScanEvent::SkeletonReady { .. } => self.run.skeleton_at = Some(now),
                ScanEvent::Finished {
                    outcome, totals, ..
                } => {
                    self.run.outcome = Some(*outcome);
                    self.run.totals = *totals;
                    self.run.finished_at = now;
                }
                _ => {}
            }
            self.run.events.push((now, ev));
        }
    }

    /// Hand idle workers work, at this instant.
    fn dispatch_idle_workers(&mut self) {
        for wi in 0..self.workers {
            if self.inflight[wi].is_some() || self.st.is_finished() {
                continue;
            }
            self.pick_for(wi);
            self.record();
        }
    }

    fn pick_for(&mut self, wi: usize) {
        let now = self.now;
        let before = self.cfg.check_tiers.then(|| self.st.tiers(now));
        match self.st.pick(wi, now) {
            Pick::Run(task) => {
                if let Some(s) = before {
                    let b = Tier::Back.index();
                    if task.tier != Tier::Back
                        && s.pending[b] > 0
                        && s.active[b] < u32::from(s.floor[b])
                    {
                        self.run.violations.push(format!(
                            "t={now}: worker {wi} took a {:?} task while Back ({} active, floor {}) had {} waiting",
                            task.tier, s.active[b], s.floor[b], s.pending[b]
                        ));
                    }
                }
                let out = task.run(self.fs, Interrupt::NEVER);
                let work = self.cfg.latency.cost(task.kind, &out, task.names.len());
                self.run.tasks[match task.kind {
                    TaskKind::Names => 0,
                    TaskKind::StatDir => 1,
                    TaskKind::Full => 2,
                }] += 1;
                self.storage.start(wi, work);
                self.inflight[wi] = Some((task.kind, out));
            }
            Pick::Idle | Pick::Exit => {}
        }
    }

    /// "Within one task length after the ramp, each tier's active count is
    /// within 1 of its target."
    fn check_tier_convergence(&mut self) {
        if !self.cfg.check_tiers {
            return;
        }
        let now = self.now;
        let settled = self.focus_since.is_some_and(|t| now >= t + self.ramp);
        self.run.tier_samples += u64::from(settled);
        let s = self.st.tiers(now);
        let off = (0..3).any(|i| s.active[i].abs_diff(u32::from(s.target[i])) > 1);
        if settled && off {
            match self.mismatch {
                None => self.mismatch = Some((now, self.storage.horizon(now))),
                Some((since, deadline)) if now > deadline => {
                    self.run.violations.push(format!(
                        "t={now}: tiers off target since {since}: active {:?} target {:?} pending {:?}",
                        s.active, s.target, s.pending
                    ));
                    self.mismatch = None;
                }
                Some(_) => {}
            }
        } else {
            self.mismatch = None;
        }
    }

    fn assert_no_deadlock(&self) {
        if self.storage.is_idle() && self.steps_left == 0 {
            self.st.validate().expect("invariants");
            panic!(
                "t={}: deadlock: no task in flight and the scan is not finished",
                self.now
            );
        }
    }

    /// Advance to the next completion or queued event (completions first
    /// on ties).
    fn advance(&mut self) {
        let done = self.storage.next_done(self.now);
        let next_q = self.queue.peek().map(|Reverse((t, _, _))| *t);
        let take_done = match (done, next_q) {
            (Some((_, td)), Some(tq)) => td <= tq,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if take_done {
            let (wi, t) = done.expect("checked");
            self.storage.advance(t - self.now);
            self.now = t;
            self.storage.finish(wi);
            let (_, out) = self.inflight[wi].take().expect("in flight");
            self.st.apply(wi, out, self.now);
        } else {
            let Reverse((t, _, ev)) = self
                .queue
                .pop()
                .expect("event queue never empties while ticking");
            self.storage.advance(t - self.now);
            self.now = t;
            match ev {
                Ev::Tick => {
                    self.st.tick(self.now);
                    self.push(self.now + self.cfg.tick, Ev::Tick);
                }
                Ev::Step(i) => {
                    self.steps_left -= 1;
                    let script = self.script;
                    self.apply_action(&script.steps[i].1);
                }
            }
        }
    }

    fn apply_action(&mut self, action: &Action) {
        let now = self.now;
        match action {
            Action::Focus(path) => {
                self.seq += 1;
                let dir = self.ids.by_path.get(path).copied();
                self.run.focus.push(FocusRecord {
                    seq: self.seq,
                    path: path.clone(),
                    dir,
                    at: now,
                    settled_at: None,
                });
                if let Some(d) = dir {
                    if self.st.set_focus(self.seq, d, now) {
                        self.focus_since = Some(now);
                        self.mismatch = None;
                    }
                }
            }
            Action::ClearFocus => {
                self.seq += 1;
                self.st.clear_focus(self.seq, now);
                self.focus_since = None;
                self.mismatch = None;
            }
            Action::Delete(path) => match self.ids.by_path.get(path).copied() {
                Some(d) if self.st.prune(d, now) => {
                    assert!(self.fs.delete(path), "prunable dir exists");
                    self.run.deleted.push(path.clone());
                }
                _ => self.run.deletes_skipped += 1,
            },
            Action::Cancel => self.st.cancel(now),
        }
    }
}

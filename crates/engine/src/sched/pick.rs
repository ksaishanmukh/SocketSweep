//! Handing a worker its next task.

use socketsweep_wire::{DirId, Tier};

use super::anchor::Anchor;
use super::{Micros, Pick, Running, ScanState, St, StatSt, Task, TaskKind};
use crate::alloc::floor_seats;

impl ScanState {
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
}

//! The three tiers around the focus and the allocation of workers to them.

use socketsweep_wire::{DirId, Tier};

use super::{Micros, ScanState};
use crate::alloc::{ramped_weight, seats, TierInput};

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

impl ScanState {
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

    pub(super) fn tier_pending(&self) -> [u32; 3] {
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

    pub(super) fn tier_active(&self) -> [u32; 3] {
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

    pub(super) fn tier_inputs(
        &self,
        now: Micros,
        active: [u32; 3],
        pending: [u32; 3],
    ) -> [TierInput; 3] {
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

    pub(super) fn targets(&self, now: Micros, active: [u32; 3], pending: [u32; 3]) -> [u8; 3] {
        seats(self.cfg.workers, &self.tier_inputs(now, active, pending))
    }

    /// Floors first (a tier with work below its floor, as the allocation
    /// resolves floors: limited by the workers, back first; the back floor
    /// before any other), then the largest `target − active`; ties go to the
    /// worker's previous tier, then to the nearer tier. Only tiers with
    /// waiting work count.
    pub(super) fn choose_tier(
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
}

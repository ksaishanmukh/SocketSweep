//! Seat allocation between the proximity tiers (D'Hondt with floors and
//! caps). Pure.

use socketsweep_wire::{Tier, TierCfg};

/// One tier's inputs to [`seats`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierInput {
    /// D'Hondt weight (the open tier's already ramped).
    pub weight: f32,
    /// Seats given first, already resolved (no [`TierCfg::AUTO_FLOOR`]).
    pub floor: u8,
    /// Most seats; [`TierCfg::NO_CAP`] does not limit.
    pub cap: u8,
    /// Workers currently in the tier.
    pub active: u32,
    /// Runnable tasks waiting in the tier.
    pub pending: u32,
}

impl TierInput {
    /// How many seats the tier can use: its workers plus its waiting tasks,
    /// limited by its cap. A tier with nothing running and nothing waiting
    /// can use none, so its seats go to the others.
    pub fn capacity(&self) -> u32 {
        let cap = if self.cap == TierCfg::NO_CAP {
            u32::MAX
        } else {
            u32::from(self.cap)
        };
        (self.active.saturating_add(self.pending)).min(cap)
    }
}

/// Share `workers` seats between the three tiers (indexed by [`Tier`]).
///
/// 1. Floors first, limited by what each tier can use. If the floors alone
///    exceed the workers they are given back-tier first, since the back
///    floor is the one the scheduler guarantees.
/// 2. Each remaining seat goes to the tier with the highest
///    `weight / (seats + 1)` among those below their capacity; ties go to the
///    nearer tier.
/// 3. Seats no tier can use within its cap go to tiers that still have work
///    beyond it (by the same quotient), so no worker sits idle while any work
///    exists; seats nobody can use at all are left unassigned.
pub fn seats(workers: u8, tiers: &[TierInput; 3]) -> [u8; 3] {
    let mut seats = floor_seats(workers, tiers);
    let mut left = u32::from(workers) - seats.iter().sum::<u32>();
    for uncapped in [false, true] {
        while left > 0 {
            let mut best: Option<(usize, f32)> = None;
            for (i, t) in tiers.iter().enumerate() {
                let room = if uncapped {
                    t.active.saturating_add(t.pending)
                } else {
                    t.capacity()
                };
                if seats[i] >= room {
                    continue;
                }
                let q = t.weight / (seats[i] + 1) as f32;
                // Strictly greater: an equal quotient keeps the nearer tier.
                if best.is_none_or(|(_, b)| q > b) {
                    best = Some((i, q));
                }
            }
            let Some((i, _)) = best else { break };
            seats[i] += 1;
            left -= 1;
        }
    }
    seats.map(|s| s as u8)
}

/// The first step of [`seats`]: each tier's floor, limited by what it can use and
/// by the workers left, given back tier first.
pub fn floor_seats(workers: u8, tiers: &[TierInput; 3]) -> [u32; 3] {
    let mut seats = [0u32; 3];
    let mut left = u32::from(workers);
    for t in [Tier::Back, Tier::Near, Tier::Open] {
        let i = t.index();
        let f = u32::from(tiers[i].floor).min(tiers[i].capacity()).min(left);
        seats[i] = f;
        left -= f;
    }
    seats
}

/// The open tier's weight `dwell` microseconds after a focus change: it ramps
/// linearly from 1.0 to `full` over `ramp`.
pub fn ramped_weight(full: f32, dwell: u64, ramp: u64) -> f32 {
    if ramp == 0 || full <= 1.0 {
        return full;
    }
    let f = (dwell as f64 / ramp as f64).min(1.0) as f32;
    1.0 + (full - 1.0) * f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(cfg: TierCfg, w: u8, weight: f32, active: u32, pending: u32) -> TierInput {
        TierInput {
            weight,
            floor: cfg.floor_for(w),
            cap: cfg.cap,
            active,
            pending,
        }
    }

    fn defaults(w: u8, open_w: f32, pending: [u32; 3]) -> [TierInput; 3] {
        let d = TierCfg::DEFAULTS;
        [
            input(d[0], w, open_w, 0, pending[0]),
            input(d[1], w, d[1].weight, 0, pending[1]),
            input(d[2], w, d[2].weight, 0, pending[2]),
        ]
    }

    #[test]
    fn steady_state_matches_the_spec_table() {
        assert_eq!(seats(8, &defaults(8, 3.0, [100, 100, 100])), [3, 1, 4]);
    }

    #[test]
    fn first_click_gives_the_open_folder_one_worker() {
        assert_eq!(seats(8, &defaults(8, 1.0, [100, 100, 100])), [1, 1, 6]);
    }

    #[test]
    fn measured_focus_gives_its_seats_away() {
        // D'Hondt with Near capped at 2 and weight 1 against Back's 4 gives
        // Near only its floor.
        assert_eq!(seats(8, &defaults(8, 3.0, [0, 100, 100])), [0, 1, 7]);
    }

    #[test]
    fn no_focus_is_all_open() {
        assert_eq!(seats(8, &defaults(8, 3.0, [100, 0, 0])), [8, 0, 0]);
    }

    #[test]
    fn unusable_seats_move_to_tiers_with_work() {
        assert_eq!(seats(8, &defaults(8, 3.0, [100, 1, 1])), [6, 1, 1]);
        assert_eq!(seats(8, &defaults(8, 3.0, [2, 0, 0])), [2, 0, 0]);
        // Near's cap gives way when nothing else has work.
        assert_eq!(seats(8, &defaults(8, 3.0, [0, 100, 0])), [0, 8, 0]);
        assert_eq!(seats(8, &defaults(8, 3.0, [0, 100, 1])), [0, 7, 1]);
    }

    #[test]
    fn floors_beyond_the_workers_favour_back() {
        assert_eq!(seats(1, &defaults(1, 3.0, [9, 9, 9])), [0, 0, 1]);
        assert_eq!(seats(2, &defaults(2, 3.0, [9, 9, 9])), [0, 1, 1]);
    }

    #[test]
    fn ramp() {
        assert_eq!(ramped_weight(3.0, 0, 400), 1.0);
        assert_eq!(ramped_weight(3.0, 200, 400), 2.0);
        assert_eq!(ramped_weight(3.0, 4000, 400), 3.0);
        assert_eq!(ramped_weight(3.0, 0, 0), 3.0);
    }
}

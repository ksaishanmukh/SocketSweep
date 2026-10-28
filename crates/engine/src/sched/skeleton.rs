//! The skeleton phase: a breadth-first `Names` walk that ends when the
//! frontier is wide or deep enough.

use socketsweep_wire::{DirId, ScanEvent, Tier};

use super::{Micros, ScanState, St};

impl ScanState {
    /// Ends the phase when the frontier holds at least 4 × W directories and
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

    /// The tier that does the skeleton walk: Back, or everything when there
    /// is no focus.
    pub(super) fn skeleton_tier(&self) -> Tier {
        if self.focus_fp().is_none() {
            Tier::Open
        } else {
            Tier::Back
        }
    }

    /// The shallowest pending directory in `tier` from the skeleton buckets
    /// (FIFO within a depth).
    pub(super) fn bucket_pick(&mut self, tier: Tier) -> Option<DirId> {
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
}

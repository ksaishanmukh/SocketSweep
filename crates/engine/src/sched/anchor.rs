//! Where in a tier a worker looks for work: its anchor, and the descent
//! from it.

use socketsweep_wire::{DirId, Tier, ROOT_DIR};

use super::{ScanState, St, TaskKind, NONE};

/// Choice of where to look for a task inside a tier.
#[derive(Debug, Clone, Copy)]
pub(super) enum Anchor {
    /// Take this directory's own task only (its subtree straddles tiers).
    Own(DirId),
    /// Descend in this subtree, which lies wholly in the tier.
    Sub(DirId),
}

impl ScanState {
    /// Step 3 of `pick`: keep the previous anchor if it is still in the tier
    /// and has work; otherwise choose by the tier's rule. A tier root (F, P,
    /// or an ancestor of P) with its own task is taken first: its subtree
    /// straddles tiers, and its stats gate its children's completion.
    pub(super) fn choose_anchor(&self, w: usize, tier: Tier) -> Option<Anchor> {
        if let Some(a) = self.workers[w].anchor {
            let r = &self.dirs[a as usize];
            if r.state != St::Pruned && r.pending_desc > 0 && self.anchor_in_tier(a, tier) {
                return Some(Anchor::Sub(a));
            }
        }
        let own = |d: DirId| self.own_task(d).is_some();
        let mut best = None;
        match self.focus_fp() {
            // No focus: everything is Open, scheduled by the Back rule (a
            // plain parallel walk wants the biggest jobs first).
            None => {
                if own(ROOT_DIR) {
                    return Some(Anchor::Own(ROOT_DIR));
                }
                self.pick_child(ROOT_DIR, NONE, true, &mut best);
            }
            Some((f, p)) => match tier {
                Tier::Open => {
                    if own(f) {
                        return Some(Anchor::Own(f));
                    }
                    self.pick_child(f, NONE, false, &mut best);
                }
                Tier::Near => {
                    if own(p) {
                        return Some(Anchor::Own(p));
                    }
                    self.pick_child(p, f, true, &mut best);
                }
                Tier::Back => {
                    let mut cur = p;
                    while cur != ROOT_DIR {
                        let a = self.dirs[cur as usize].parent;
                        if own(a) {
                            return Some(Anchor::Own(a));
                        }
                        self.pick_child(a, cur, true, &mut best);
                        cur = a;
                    }
                }
            },
        }
        best.map(|b| Anchor::Sub(b.2))
    }

    /// Step 4 of `pick`: depth-first from `a`: a directory's own task first,
    /// else the highest-id child with work.
    pub(super) fn descend(&self, a: DirId) -> (DirId, TaskKind) {
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

    // (fewest workers, then most work) or (fewest workers, then least work)
    fn pick_child(
        &self,
        parent: DirId,
        skip: DirId,
        most_work: bool,
        best: &mut Option<(u16, i64, DirId)>,
    ) {
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
    }
}

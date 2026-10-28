//! The completion cascade: finishing a directory once it is ready, then its
//! parents.

use socketsweep_wire::{DirId, Outcome, ScanEvent, ROOT_DIR};

use crate::sched::{Micros, ScanState, St, StatSt, NONE};

impl ScanState {
    /// Listed, stat'ed, every child finished, and (G2) the parent's stats
    /// applied.
    pub(super) fn ready(&self, x: DirId) -> bool {
        let r = &self.dirs[x as usize];
        r.state == St::Listed
            && r.open_children == 0
            && (r.parent == NONE || self.dirs[r.parent as usize].stat == StatSt::Done)
    }

    /// The completion cascade, as a loop: finish `d` if ready, then its
    /// parent, and so on.
    pub(crate) fn try_complete(&mut self, d: DirId, now: Micros) {
        let mut x = d;
        while !self.is_finished() && self.ready(x) {
            self.finish_one(x, now);
            if x == ROOT_DIR {
                break;
            }
            x = self.dirs[x as usize].parent;
        }
    }

    /// Emit `SubtreeDone(x)` and hand its totals to its parent.
    pub(super) fn finish_one(&mut self, x: DirId, now: Micros) {
        let r = &mut self.dirs[x as usize];
        r.state = St::Done;
        let totals = r.totals;
        let parent = r.parent;
        self.out.push(ScanEvent::SubtreeDone { dir: x, totals });
        if let Some(f) = self.focus.as_mut() {
            if f.dir == x && !f.settled_sent {
                f.settled_sent = true;
                let seq = f.seq;
                self.out.push(ScanEvent::FocusState {
                    seq,
                    dir: x,
                    settled: true,
                });
            }
        }
        if x == ROOT_DIR {
            self.finish(Outcome::Complete, now);
        } else {
            let p = &mut self.dirs[parent as usize];
            p.totals.add(&totals);
            p.open_children -= 1;
        }
    }
}

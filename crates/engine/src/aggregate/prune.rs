//! Pruning a subtree deleted during the scan.

use socketsweep_wire::{DirId, ScanEvent, Totals, ROOT_DIR};

use crate::sched::{Micros, ScanState, St, NONE};

impl ScanState {
    /// A delete during the scan (`Delete.hint`): prune directory `dir` and
    /// everything below it. Returns `false` (and does nothing) for the root,
    /// an unknown or already pruned directory, or a finished scan.
    ///
    /// - The subtree is marked `Pruned`; its waiting tasks leave the
    ///   counters; results still in flight inside it are discarded.
    /// - If `dir` was finished, its totals plus its own entry are subtracted
    ///   from each ancestor up to and including the first one not finished.
    /// - If not, only its own entry is subtracted from the parent, whose
    ///   `open_children` goes down by one, which may let the parent finish.
    /// - Emits `Pruned { dir }` (before any `SubtreeDone` that this allows).
    pub fn prune(&mut self, dir: DirId, now: Micros) -> bool {
        if self.is_finished() || dir == ROOT_DIR {
            return false;
        }
        let Some(rec) = self.dirs.get(dir as usize) else {
            return false;
        };
        if rec.state == St::Pruned {
            return false;
        }
        let was_done = rec.state == St::Done;
        let parent = rec.parent;
        let own = rec.own;
        let sub_totals = rec.totals;
        let pending = rec.pending_desc;
        if self.focus.is_some_and(|f| self.within(f.dir, dir)) {
            self.focus = None;
        }
        self.add_pending(parent, -i64::from(pending));

        self.mark_subtree_pruned(dir);

        let entry = Totals {
            apparent: if own.counted { own.apparent } else { 0 },
            disk: if own.counted { own.disk } else { 0 },
            files: 0,
            dirs: 1,
            errors: u32::from(own.counted && own.err),
            flags: 0,
            newest_mtime: 0,
        };
        self.out.push(ScanEvent::Pruned { dir });
        if was_done {
            let mut gone = sub_totals;
            gone.add(&entry);
            self.subtract_from_ancestors(parent, &gone);
        } else {
            let p = &mut self.dirs[parent as usize];
            p.totals.subtract(&entry);
            p.open_children -= 1;
            self.try_complete(parent, now);
        }
        self.tick(now);
        true
    }

    /// Mark `dir` and everything below it `Pruned`, taking unlisted
    /// directories out of the counts and dropping owed stats.
    fn mark_subtree_pruned(&mut self, dir: DirId) {
        let mut stack = vec![dir];
        while let Some(x) = stack.pop() {
            let r = &mut self.dirs[x as usize];
            if matches!(r.state, St::Pending | St::InFlight) {
                let depth = usize::from(r.depth);
                self.unlisted_by_depth[depth] -= 1;
                self.unlisted -= 1;
            }
            r.state = St::Pruned;
            r.pending_desc = 0;
            stack.extend(r.children());
            self.deferred.remove(&x);
        }
    }

    /// Subtract `gone` from each ancestor from `parent` up to and including
    /// the first one not finished.
    fn subtract_from_ancestors(&mut self, parent: DirId, gone: &Totals) {
        let mut a = parent;
        loop {
            let r = &mut self.dirs[a as usize];
            r.totals.subtract(gone);
            if r.state != St::Done || r.parent == NONE {
                break;
            }
            a = r.parent;
        }
    }
}

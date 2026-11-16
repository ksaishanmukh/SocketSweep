//! Handing a worker its next task.
//!
//! Once [`choose_lane`](ScanState::choose_lane) has picked a side of the
//! focus, the task inside it is, in order:
//!
//! 1. **Big jobs.** A `StatDir` of at least [`BIG_JOB_ENTRIES`] entries,
//!    largest first. (Focused work takes its gating tasks, step 3, first.)
//! 2. **Skeleton** (background only, while the phase lasts): `Names` tasks,
//!    shallowest first from per-depth FIFO buckets, so no worker waits for a
//!    level to finish. A focused folder is read with `Full` even now: its
//!    sizes are what the user is waiting for.
//! 3. **Gating tasks.** Focused: the deferred stats of F's parent, then F's
//!    own read. Background: the own task of F's nearest ancestor above the
//!    parent that has one. These directories straddle both sides, so nobody
//!    descends into them; their tasks are taken directly.
//! 4. **Anchor and descent.** A worker keeps the subtree it last worked in
//!    while that has work on the same side. Otherwise it takes a new one: a
//!    child of F with the fewest workers, then the least work (small folders
//!    finish and their rows go final); in the background, a subtree beside
//!    the path from F to the root with the fewest workers, then the most
//!    work (big jobs start early, so the tail stays short). It then descends
//!    depth-first: a directory's own task, else the highest-id child with
//!    work.
//!
//! With no focus everything is background, scheduled from the root.

use socketsweep_wire::{DirId, ROOT_DIR};

use super::focus::Lane;
#[cfg(doc)]
use super::BIG_JOB_ENTRIES;
use super::{Micros, Pick, Running, ScanState, St, StatSt, Task, TaskKind, NONE};

/// How a candidate subtree is ranked after its worker count.
#[derive(Debug, Clone, Copy)]
enum Prefer {
    LeastWork,
    MostWork,
}

impl ScanState {
    /// Hand worker `w` its next task. The worker must have applied its
    /// previous result first. Congestion is the driver's business: it does
    /// not call `pick` while the sink is congested.
    pub fn pick(&mut self, w: usize, now: Micros) -> Pick {
        self.tick(now);
        if self.is_finished() {
            return Pick::Exit;
        }
        assert!(self.workers[w].task.is_none(), "worker {w} picked twice");
        let f = self.steering();
        let lane = match f {
            None => (self.dirs[ROOT_DIR as usize].pending_desc > 0).then_some(Lane::Background),
            Some(f) => self.choose_lane(f),
        };
        let Some(lane) = lane else {
            return Pick::Idle;
        };
        let found = match lane {
            Lane::Focused => self.find_focused(w, f.expect("a focused lane has a focus")),
            Lane::Background => self.find_background(w, f),
        };
        let Some((dir, kind)) = found else {
            debug_assert!(false, "{lane:?} has pending work but no task");
            return Pick::Idle;
        };
        Pick::Run(self.start_task(w, dir, kind))
    }

    fn find_focused(&mut self, w: usize, f: DirId) -> Option<(DirId, TaskKind)> {
        let p = self.dirs[f as usize].parent;
        for d in [p, f] {
            if let Some(k) = self.own_task(d) {
                return Some((d, k));
            }
        }
        if let Some(d) = self.take_big(Some(f), Lane::Focused) {
            return Some((d, TaskKind::StatDir));
        }
        let kept = self.workers[w].anchor.filter(|&a| self.within(a, f));
        let a = self
            .keep_anchor(kept)
            .or_else(|| self.best_child(f, NONE, Prefer::LeastWork))?;
        self.workers[w].anchor = Some(a);
        Some(self.descend(a))
    }

    fn find_background(&mut self, w: usize, f: Option<DirId>) -> Option<(DirId, TaskKind)> {
        if let Some(d) = self.take_big(f, Lane::Background) {
            return Some((d, TaskKind::StatDir));
        }
        if let Some(d) = self.bucket_pick(f) {
            return Some((d, TaskKind::Names));
        }
        if let Some(d) = self.gating_ancestor(f) {
            return Some((d, self.own_task(d)?));
        }
        let kept = self.workers[w].anchor.filter(|&a| match f {
            None => true,
            Some(f) => !self.within(a, f) && !self.within(f, a),
        });
        let a = self
            .keep_anchor(kept)
            .or_else(|| self.background_anchor(f))?;
        self.workers[w].anchor = Some(a);
        Some(self.descend(a))
    }

    /// The nearest directory on the path from F's grandparent (or, with no
    /// focus, the root) up to the root that has its own task.
    fn gating_ancestor(&self, f: Option<DirId>) -> Option<DirId> {
        let mut a = match f {
            None => ROOT_DIR,
            Some(f) => self.dirs[self.dirs[f as usize].parent as usize].parent,
        };
        while a != NONE {
            if self.own_task(a).is_some() {
                return Some(a);
            }
            a = self.dirs[a as usize].parent;
        }
        None
    }

    /// The previous anchor, if it still has work.
    fn keep_anchor(&self, a: Option<DirId>) -> Option<DirId> {
        a.filter(|&a| {
            let r = &self.dirs[a as usize];
            r.state != St::Pruned && r.pending_desc > 0
        })
    }

    /// The best subtree beside the path from F up to the root (with no
    /// focus, the best child of the root).
    fn background_anchor(&self, f: Option<DirId>) -> Option<DirId> {
        let Some(mut cur) = f else {
            return self.best_child(ROOT_DIR, NONE, Prefer::MostWork);
        };
        let mut best = None;
        while cur != ROOT_DIR {
            let a = self.dirs[cur as usize].parent;
            best = self.better(
                best,
                self.best_child(a, cur, Prefer::MostWork),
                Prefer::MostWork,
            );
            cur = a;
        }
        best
    }

    /// The child of `parent` other than `skip` with work, fewest workers
    /// first, then by `prefer`.
    fn best_child(&self, parent: DirId, skip: DirId, prefer: Prefer) -> Option<DirId> {
        self.dirs[parent as usize]
            .children()
            .filter(|&c| {
                let r = &self.dirs[c as usize];
                c != skip && r.pending_desc > 0 && r.state != St::Pruned
            })
            .fold(None, |best, c| self.better(best, Some(c), prefer))
    }

    /// The better of two candidate anchors; ties keep `a`.
    fn better(&self, a: Option<DirId>, b: Option<DirId>, prefer: Prefer) -> Option<DirId> {
        let key = |d: DirId| {
            let r = &self.dirs[d as usize];
            let work = i64::from(r.pending_desc);
            let work = match prefer {
                Prefer::LeastWork => work,
                Prefer::MostWork => -work,
            };
            (r.active_desc, work)
        };
        match (a, b) {
            (Some(x), Some(y)) if key(y) < key(x) => Some(y),
            (None, y) => y,
            (x, _) => x,
        }
    }

    /// Depth-first from `a`: a directory's own task first, else the
    /// highest-id child with work.
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
                    let r = &self.dirs[c as usize];
                    r.pending_desc > 0 && r.state != St::Pruned
                })
                .expect("pending_desc > 0 implies a task below");
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

    /// The largest waiting big job on `lane` of focus `f`.
    fn take_big(&mut self, f: Option<DirId>, lane: Lane) -> Option<DirId> {
        let dirs = &self.dirs;
        self.big.retain(|&(_, d)| {
            dirs[d as usize].state == St::NamesOnly && dirs[d as usize].stat == StatSt::Pending
        });
        self.big
            .iter()
            .filter(|&&(_, d)| self.lane_of(d, f) == lane)
            .max_by_key(|&&(n, _)| n)
            .map(|&(_, d)| d)
    }

    /// The shallowest pending background directory from the skeleton
    /// buckets (FIFO within a depth), while the skeleton phase lasts.
    fn bucket_pick(&mut self, f: Option<DirId>) -> Option<DirId> {
        let mut buckets = self.skeleton.take()?;
        let mut found = None;
        for b in &mut buckets {
            while b
                .front()
                .is_some_and(|&d| self.dirs[d as usize].state != St::Pending)
            {
                b.pop_front();
            }
            let at = b.iter().position(|&d| {
                self.dirs[d as usize].state == St::Pending && self.lane_of(d, f) == Lane::Background
            });
            found = at.and_then(|i| b.remove(i));
            if found.is_some() {
                break;
            }
        }
        self.skeleton = Some(buckets);
        found
    }

    fn start_task(&mut self, w: usize, dir: DirId, kind: TaskKind) -> Task {
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
        self.workers[w].task = Some(Running { dir, kind });
        Task {
            dir,
            kind,
            path: self.path_of(dir),
            names,
        }
    }
}

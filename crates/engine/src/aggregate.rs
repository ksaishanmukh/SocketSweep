//! `aggregate`: applying task results, running totals, open-child counts,
//! the completion cascade and pruning. Part of [`ScanState`], under the same
//! lock as the scheduler.
//!
//! # Totals
//!
//! A directory's [`Totals`] cover everything strictly below it: each entry
//! counts once (`files` or `dirs`), with its own `lstat` size (directories
//! included), plus the totals of each walked subdirectory. An entry whose
//! stat failed counts in `errors` and adds no size; a directory that could
//! not be read is `errors: 1` with [`Totals::UNREADABLE`].
//!
//! **Hardlinks.** A non-directory with `nlink > 1` adds its size only the
//! first time its `(dev, ino)` is seen in the scan; later links still count
//! in `files` but add no bytes. The wire [`Stat`](socketsweep_wire::Stat)
//! carries no inode, so this happens here, before totals are emitted. The
//! host's provisional sums (from `Stats` events) may count such a file
//! twice until `SubtreeDone` corrects them. A prune does not forget the
//! inodes it removes, so a file hardlinked both inside and outside a pruned
//! subtree stays uncounted.
//!
//! **Depth limit.** A subdirectory deeper than `max_depth` is not walked: it
//! is listed with `id: None`, counted as an entry, and its parent's totals
//! get [`Totals::DEPTH_LIMIT`].
//!
//! **Other file systems.** With `one_filesystem`, a directory whose own
//! device differs from the root's is listed as empty (one `Listing` part
//! with no entries) and finished with [`Totals::OTHER_FS`]. A `Names`
//! listing has no stats, so the check uses the device of the opened
//! directory ([`DirRead::dev`](crate::DirRead::dev)) rather than the
//! parent's stat of it.
//!
//! # `None` stats
//!
//! An entry whose `stat` is `None` in a `Listing` always gets a later
//! `Stats` slot: deferred stats for a `Names` listing, and for a `Full`
//! listing a `Stats` of `None`s covering each entry whose `lstat` failed
//! (sent straight after the listing). A `None` in `Stats` is an entry that
//! could not be stat'ed.
//!
//! # G2 and deferred stats
//!
//! G2 orders `Stats(D)` before every child's `SubtreeDone`. A child of a
//! directory listed by `Names` can be measured before its parent's deferred
//! stats arrive, so a child is only *finished* (its `SubtreeDone` emitted and
//! its totals added to the parent) once the parent's stats are applied;
//! until then it waits, complete, in state `Listed`. Applying the parent's
//! stats finishes every such waiting child, then the parent if it can.

use socketsweep_wire::{
    DirId, Entry, FsOp, Kind, Outcome, ScanEvent, Totals, MAX_LISTING_PART, ROOT_DIR,
};

use crate::fs::{errno, DirRead, StatResult};
use crate::sched::{
    DeferredEnt, DirRec, Micros, Own, ScanState, St, StatSt, TaskKind, TaskOutput, BIG_JOB_ENTRIES,
    NONE,
};

impl ScanState {
    /// Apply worker `w`'s result for the task it was given. Results for a
    /// pruned directory, or arriving after the scan finished, are discarded.
    /// An `Interrupted` result outside a cancel (the pool only interrupts on
    /// cancel) is treated as a failure with `EINTR`.
    pub fn apply(&mut self, w: usize, output: TaskOutput, now: Micros) {
        let Some(t) = self.workers[w].task.take() else {
            panic!("worker {w} applied a result without a task");
        };
        self.add_active(t.dir, -1);
        if self.is_finished() || self.dirs[t.dir as usize].state == St::Pruned {
            return;
        }
        let output = match output {
            TaskOutput::Interrupted => TaskOutput::Failed {
                op: if t.kind == TaskKind::StatDir {
                    FsOp::Stat
                } else {
                    FsOp::ReadDir
                },
                errno: errno::EINTR,
            },
            o => o,
        };
        match (t.kind, output) {
            (TaskKind::Names | TaskKind::Full, TaskOutput::Listing(read)) => {
                self.on_listing(t.dir, t.kind, read, now)
            }
            (TaskKind::Names | TaskKind::Full, TaskOutput::Failed { op, errno }) => {
                self.on_dir_failed(t.dir, op, errno, now)
            }
            (TaskKind::StatDir, TaskOutput::Stats(v)) => self.on_stats(t.dir, Ok(v), now),
            (TaskKind::StatDir, TaskOutput::Failed { errno, .. }) => {
                self.on_stats(t.dir, Err(errno), now)
            }
            (kind, out) => panic!("result {out:?} does not match a {kind:?} task"),
        }
        self.tick(now);
    }

    fn mark_listed(&mut self, d: DirId) {
        let depth = usize::from(self.dirs[d as usize].depth);
        self.unlisted_by_depth[depth] -= 1;
        self.unlisted -= 1;
        self.listed += 1;
    }

    fn on_listing(&mut self, d: DirId, kind: TaskKind, read: DirRead, now: Micros) {
        self.mark_listed(d);
        if d != ROOT_DIR && self.cfg.one_fs && read.dev != self.root_dev {
            self.out.push(ScanEvent::Listing {
                dir: d,
                part: 0,
                last: true,
                entries: Vec::new(),
            });
            let r = &mut self.dirs[d as usize];
            r.totals.flags |= Totals::OTHER_FS;
            r.state = St::Listed;
            r.stat = StatSt::Done;
            self.try_complete(d, now);
            return;
        }

        let depth = self.dirs[d as usize].depth;
        let child_depth = depth + 1;
        let walk = child_depth <= self.cfg.max_depth;
        let first = self.dirs.len() as DirId;
        let n = read.entries.len();
        let mut entries = Vec::with_capacity(n);
        let mut deferred = Vec::with_capacity(if kind == TaskKind::Names { n } else { 0 });
        let mut tot = Totals::default();
        let mut n_children: u32 = 0;
        let mut owed = 0usize;
        // Full listings: positions whose stat failed (sent as `None`).
        let mut failed_at: Vec<u32> = Vec::new();
        for e in read.entries {
            let is_dir = e.kind == Kind::Dir;
            let id = if is_dir && walk {
                let id = first + n_children;
                n_children += 1;
                self.dirs.push(DirRec::new(
                    d,
                    e.name.clone().into_boxed_slice(),
                    child_depth,
                ));
                Some(id)
            } else {
                if is_dir {
                    tot.flags |= Totals::DEPTH_LIMIT;
                }
                None
            };
            if is_dir {
                tot.dirs += 1;
            } else {
                tot.files += 1;
            }
            let wire_stat = match kind {
                TaskKind::Full => {
                    let r = e.stat.unwrap_or(Err(errno::EIO));
                    self.account(&mut tot, id.unwrap_or(NONE), &r);
                    if r.is_err() {
                        failed_at.push(entries.len() as u32);
                    }
                    r.ok().map(|s| s.stat)
                }
                _ => {
                    owed += usize::from(e.stat.is_none());
                    deferred.push(DeferredEnt {
                        name: e.name.clone(),
                        early: e.stat,
                        child: id.unwrap_or(NONE),
                    });
                    None
                }
            };
            entries.push(Entry {
                name: e.name,
                kind: e.kind,
                stat: wire_stat,
                id,
            });
        }
        self.emit_listing(d, entries);
        // A `None` stat in a listing always promises a `Stats` event: for a
        // full listing, failed stats are sent as runs of `None`.
        let mut i = 0;
        while i < failed_at.len() {
            let start = failed_at[i];
            let mut j = i + 1;
            while j < failed_at.len() && failed_at[j] == failed_at[j - 1] + 1 {
                j += 1;
            }
            self.out.push(ScanEvent::Stats {
                dir: d,
                start,
                stats: vec![None; j - i],
            });
            i = j;
        }

        let rec = &mut self.dirs[d as usize];
        rec.first_child = if n_children > 0 { first } else { NONE };
        rec.n_children = n_children;
        rec.open_children = n_children;
        rec.totals.add(&tot);

        if n_children > 0 {
            let cd = usize::from(child_depth);
            if self.unlisted_by_depth.len() <= cd {
                self.unlisted_by_depth.resize(cd + 1, 0);
            }
            self.unlisted_by_depth[cd] += n_children;
            self.unlisted += n_children;
            if let Some(buckets) = self.skeleton.as_mut() {
                if buckets.len() <= cd {
                    buckets.resize_with(cd + 1, Default::default);
                }
                buckets[cd].extend(first..first + n_children);
            }
        }

        let mut delta = i64::from(n_children);
        if kind == TaskKind::Names && owed > 0 {
            let r = &mut self.dirs[d as usize];
            r.state = St::NamesOnly;
            r.stat = StatSt::Pending;
            delta += 1;
            if owed as u32 >= BIG_JOB_ENTRIES {
                self.big.push((owed as u32, d));
            }
            self.deferred.insert(d, deferred);
            self.add_pending(d, delta);
        } else {
            self.add_pending(d, delta);
            if kind == TaskKind::Names {
                // Every entry was stat'ed early (DT_UNKNOWN) or there are none:
                // the deferred stats are already in hand.
                self.dirs[d as usize].state = St::NamesOnly;
                self.dirs[d as usize].stat = StatSt::InFlight;
                self.deferred.insert(d, deferred);
                self.on_stats(d, Ok(Vec::new()), now);
            } else {
                let r = &mut self.dirs[d as usize];
                r.state = St::Listed;
                r.stat = StatSt::Done;
                self.try_complete(d, now);
            }
        }
    }

    fn emit_listing(&mut self, d: DirId, entries: Vec<Entry>) {
        if entries.is_empty() {
            self.out.push(ScanEvent::Listing {
                dir: d,
                part: 0,
                last: true,
                entries,
            });
            return;
        }
        let parts = entries.len().div_ceil(MAX_LISTING_PART);
        let mut it = entries.into_iter();
        for part in 0..parts {
            let chunk: Vec<Entry> = it.by_ref().take(MAX_LISTING_PART).collect();
            self.out.push(ScanEvent::Listing {
                dir: d,
                part: part as u16,
                last: part + 1 == parts,
                entries: chunk,
            });
        }
    }

    /// Add one entry's stat to `tot` (and remember a child directory's own
    /// contribution for prunes).
    fn account(&mut self, tot: &mut Totals, child: DirId, r: &StatResult) {
        match r {
            Ok(st) => {
                let dup =
                    st.kind != Kind::Dir && st.nlink > 1 && !self.links.insert((st.dev, st.ino));
                let (apparent, disk) = if dup {
                    (0, 0)
                } else {
                    (st.stat.apparent, st.stat.disk())
                };
                tot.apparent = tot.apparent.saturating_add(apparent);
                tot.disk = tot.disk.saturating_add(disk);
                if st.kind != Kind::Dir {
                    self.files_seen += 1;
                }
                self.disk_seen = self.disk_seen.saturating_add(disk);
                if child != NONE {
                    self.dirs[child as usize].own = Own {
                        counted: true,
                        err: false,
                        apparent,
                        disk,
                    };
                }
            }
            Err(_) => {
                tot.errors += 1;
                if child != NONE {
                    self.dirs[child as usize].own = Own {
                        counted: true,
                        err: true,
                        apparent: 0,
                        disk: 0,
                    };
                }
            }
        }
    }

    fn on_dir_failed(&mut self, d: DirId, op: FsOp, errno: i32, now: Micros) {
        self.mark_listed(d);
        self.out.push(ScanEvent::DirFailed { dir: d, op, errno });
        let r = &mut self.dirs[d as usize];
        r.totals.errors += 1;
        r.totals.flags |= Totals::UNREADABLE;
        r.state = St::Listed;
        r.stat = StatSt::Done;
        self.try_complete(d, now);
    }

    /// Apply a directory's deferred stats. `Err(errno)`: the directory could
    /// not be reopened, so every owed entry is `None` (an error each) and the
    /// totals are flagged [`Totals::UNREADABLE`]; no `DirFailed` is sent,
    /// since the listing already succeeded. A pruned child's slot is applied
    /// as nothing and sent as `None` (the host has dropped that node).
    fn on_stats(&mut self, d: DirId, results: Result<Vec<StatResult>, i32>, now: Micros) {
        let ents = self.deferred.remove(&d).unwrap_or_default();
        let failed = results.as_ref().err().copied();
        let mut it = results.unwrap_or_default().into_iter();
        let mut tot = Totals::default();
        let mut wire = Vec::with_capacity(ents.len());
        for e in ents {
            let r = match e.early {
                Some(r) => r,
                None => match failed {
                    Some(errno) => Err(errno),
                    None => it.next().unwrap_or(Err(errno::EIO)),
                },
            };
            if e.child != NONE && self.dirs[e.child as usize].state == St::Pruned {
                wire.push(None);
                continue;
            }
            self.account(&mut tot, e.child, &r);
            wire.push(r.ok().map(|s| s.stat));
        }
        if failed.is_some() {
            tot.flags |= Totals::UNREADABLE;
        }
        let mut start = 0u32;
        let mut wire = wire.into_iter().peekable();
        while wire.peek().is_some() {
            let chunk: Vec<_> = wire.by_ref().take(MAX_LISTING_PART).collect();
            let len = chunk.len() as u32;
            self.out.push(ScanEvent::Stats {
                dir: d,
                start,
                stats: chunk,
            });
            start += len;
        }
        let r = &mut self.dirs[d as usize];
        r.totals.add(&tot);
        r.stat = StatSt::Done;
        r.state = St::Listed;
        // Children that finished while these stats were owed.
        for c in self.dirs[d as usize].children() {
            if self.ready(c) {
                self.finish_one(c, now);
            }
        }
        self.try_complete(d, now);
    }

    /// Listed, stat'ed, every child finished, and (G2) the parent's stats
    /// applied.
    fn ready(&self, x: DirId) -> bool {
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
    fn finish_one(&mut self, x: DirId, now: Micros) {
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

        let entry = Totals {
            apparent: if own.counted { own.apparent } else { 0 },
            disk: if own.counted { own.disk } else { 0 },
            files: 0,
            dirs: 1,
            errors: u32::from(own.counted && own.err),
            flags: 0,
        };
        self.out.push(ScanEvent::Pruned { dir });
        if was_done {
            let mut gone = sub_totals;
            gone.add(&entry);
            let mut a = parent;
            loop {
                let r = &mut self.dirs[a as usize];
                r.totals.subtract(&gone);
                if r.state != St::Done || r.parent == NONE {
                    break;
                }
                a = r.parent;
            }
        } else {
            let p = &mut self.dirs[parent as usize];
            p.totals.subtract(&entry);
            p.open_children -= 1;
            self.try_complete(parent, now);
        }
        self.tick(now);
        true
    }
}

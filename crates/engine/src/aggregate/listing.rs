//! Applying a `Names` or `Full` listing, or a directory that could not be
//! read.

use socketsweep_wire::{DirId, Entry, FsOp, Kind, ScanEvent, Totals, MAX_LISTING_PART, ROOT_DIR};

use crate::fs::{errno, DirRead, FsEntry};
use crate::sched::{
    DeferredEnt, DirRec, Micros, ScanState, St, StatSt, TaskKind, BIG_JOB_ENTRIES, NONE,
};

/// One listing on its way to wire entries, new child rows and totals.
struct ListingBuild {
    kind: TaskKind,
    walk: bool,
    child_depth: u8,
    first: DirId,
    entries: Vec<Entry>,
    deferred: Vec<DeferredEnt>,
    tot: Totals,
    n_children: u32,
    owed: usize,
    // Full listings: positions whose stat failed (sent as `None`).
    failed_at: Vec<u32>,
}

impl ScanState {
    pub(super) fn mark_listed(&mut self, d: DirId) {
        let depth = usize::from(self.dirs[d as usize].depth);
        self.unlisted_by_depth[depth] -= 1;
        self.unlisted -= 1;
        self.listed += 1;
    }

    pub(super) fn on_listing(&mut self, d: DirId, kind: TaskKind, read: DirRead, now: Micros) {
        self.mark_listed(d);
        if d != ROOT_DIR && self.cfg.one_fs && read.dev != self.root_dev {
            self.finish_other_fs(d, now);
            return;
        }

        let depth = self.dirs[d as usize].depth;
        let child_depth = depth + 1;
        let n = read.entries.len();
        let mut build = ListingBuild {
            kind,
            walk: child_depth <= self.cfg.max_depth,
            child_depth,
            first: self.dirs.len() as DirId,
            entries: Vec::with_capacity(n),
            deferred: Vec::with_capacity(if kind == TaskKind::Names { n } else { 0 }),
            tot: Totals::default(),
            n_children: 0,
            owed: 0,
            failed_at: Vec::new(),
        };
        for e in read.entries {
            self.add_entry(d, &mut build, e);
        }
        let ListingBuild {
            first,
            entries,
            deferred,
            tot,
            n_children,
            owed,
            failed_at,
            ..
        } = build;
        self.emit_listing(d, entries);
        self.emit_failed_stats(d, &failed_at);
        self.record_children(d, first, n_children, child_depth, &tot);
        self.settle_listing(d, kind, n_children, owed, deferred, now);
    }

    pub(super) fn on_dir_failed(&mut self, d: DirId, op: FsOp, errno: i32, now: Micros) {
        self.mark_listed(d);
        self.out.push(ScanEvent::DirFailed { dir: d, op, errno });
        let r = &mut self.dirs[d as usize];
        r.totals.errors += 1;
        r.totals.flags |= Totals::UNREADABLE;
        r.state = St::Listed;
        r.stat = StatSt::Done;
        self.try_complete(d, now);
    }

    /// `d` lies on another file system: list it as empty and finish it.
    fn finish_other_fs(&mut self, d: DirId, now: Micros) {
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
    }

    /// One entry of `d`'s listing: introduce it if it is a walked directory,
    /// count it, and account or defer its stat.
    fn add_entry(&mut self, d: DirId, build: &mut ListingBuild, e: FsEntry) {
        let is_dir = e.kind == Kind::Dir;
        let id = if is_dir && build.walk {
            let id = build.first + build.n_children;
            build.n_children += 1;
            self.dirs.push(DirRec::new(
                d,
                e.name.clone().into_boxed_slice(),
                build.child_depth,
            ));
            Some(id)
        } else {
            if is_dir {
                build.tot.flags |= Totals::DEPTH_LIMIT;
            }
            None
        };
        if is_dir {
            build.tot.dirs += 1;
        } else {
            build.tot.files += 1;
        }
        let wire_stat = match build.kind {
            TaskKind::Full => {
                let r = e.stat.unwrap_or(Err(errno::EIO));
                self.account(&mut build.tot, id.unwrap_or(NONE), &r);
                if r.is_err() {
                    build.failed_at.push(build.entries.len() as u32);
                }
                r.ok().map(|s| s.stat)
            }
            _ => {
                build.owed += usize::from(e.stat.is_none());
                build.deferred.push(DeferredEnt {
                    name: e.name.clone(),
                    early: e.stat,
                    child: id.unwrap_or(NONE),
                });
                None
            }
        };
        build.entries.push(Entry {
            name: e.name,
            kind: e.kind,
            stat: wire_stat,
            id,
        });
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

    fn emit_failed_stats(&mut self, d: DirId, failed_at: &[u32]) {
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
    }

    /// Link `d`'s new children into the table, its totals and the unlisted
    /// counts (and the skeleton buckets while they last).
    fn record_children(
        &mut self,
        d: DirId,
        first: DirId,
        n_children: u32,
        child_depth: u8,
        tot: &Totals,
    ) {
        let rec = &mut self.dirs[d as usize];
        rec.first_child = if n_children > 0 { first } else { NONE };
        rec.n_children = n_children;
        rec.open_children = n_children;
        rec.totals.add(tot);

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
    }

    /// The listing's effect on `d`'s state: owed stats become a `StatDir`
    /// task; otherwise the directory moves on at once.
    fn settle_listing(
        &mut self,
        d: DirId,
        kind: TaskKind,
        n_children: u32,
        owed: usize,
        deferred: Vec<DeferredEnt>,
        now: Micros,
    ) {
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
}

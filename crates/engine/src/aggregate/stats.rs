//! Applying stats: one entry's contribution to the totals, and a
//! directory's deferred stats.

use socketsweep_wire::{DirId, Kind, ScanEvent, Totals, MAX_LISTING_PART};

use crate::fs::{errno, StatResult};
use crate::sched::{Micros, Own, ScanState, St, StatSt, NONE};

impl ScanState {
    /// Add one entry's stat to `tot` (and remember a child directory's own
    /// contribution for prunes).
    pub(super) fn account(&mut self, tot: &mut Totals, child: DirId, r: &StatResult) {
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
                tot.newest_mtime = tot.newest_mtime.max(st.stat.mtime);
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

    /// Apply a directory's deferred stats. `Err(errno)`: the directory could
    /// not be reopened, so every owed entry is `None` (an error each) and the
    /// totals are flagged [`Totals::UNREADABLE`]; no `DirFailed` is sent,
    /// since the listing already succeeded. A pruned child's slot is applied
    /// as nothing and sent as `None` (the host has dropped that node).
    pub(super) fn on_stats(
        &mut self,
        d: DirId,
        results: Result<Vec<StatResult>, i32>,
        now: Micros,
    ) {
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
}

//! Applying task results: running totals, open-child counts, the completion
//! cascade and pruning. Part of [`ScanState`], under the same lock as the
//! scheduler.
//!
//! # Totals
//!
//! A directory's [`Totals`] cover everything strictly below it: each entry
//! counts once (`files` or `dirs`), with its own `lstat` size (directories
//! included), plus the totals of each walked subdirectory. An entry whose
//! stat failed counts in `errors` and adds no size; a directory that could
//! not be read is `errors: 1` with [`Totals::UNREADABLE`].
//!
//! **Newest mtime.** `newest_mtime` is the maximum `st_mtime` over every
//! entry counted, directories' own entries included (a directory's mtime
//! moves whenever something is added or removed directly in it, so it can
//! be newer than any file left inside), merged up with `max`. Entries whose
//! stat failed contribute nothing, and 0 means no mtime is known. Hardlink
//! duplicates still count here: the mtime is the same inode's, and only the
//! bytes are deduplicated. It is a high-water mark: pruning a subtree that
//! was deleted during the scan does not lower it, since the siblings would
//! have to be revisited to know the next-newest.
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
//! # Deferred stats
//!
//! The wire order puts `Stats(D)` before every child's `SubtreeDone`. A child of a
//! directory listed by `Names` can be measured before its parent's deferred
//! stats arrive, so a child is only *finished* (its `SubtreeDone` emitted and
//! its totals added to the parent) once the parent's stats are applied;
//! until then it waits, complete, in state `Listed`. Applying the parent's
//! stats finishes every such waiting child, then the parent if it can.

use socketsweep_wire::FsOp;
#[cfg(doc)]
use socketsweep_wire::Totals;

use crate::fs::errno;
use crate::sched::{Micros, ScanState, St, TaskKind, TaskOutput};

mod complete;
mod listing;
mod prune;
mod stats;

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
}

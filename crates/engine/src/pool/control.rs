//! What the session's reader thread tells a running scan.

use std::sync::atomic::Ordering;

use socketsweep_wire::DirId;

use super::Scan;
use crate::sink::EventSink;

impl<S: EventSink> Scan<S> {
    /// `ClientMsg::Focus`. Returns whether the focus changed (stale `seq`s
    /// and unknown or pruned directories are ignored).
    pub fn focus(&self, seq: u32, dir: DirId) -> bool {
        self.update(|s, now| s.set_focus(seq, dir, now))
    }

    /// `ClientMsg::ClearFocus`.
    pub fn clear_focus(&self, seq: u32) -> bool {
        self.update(|s, now| s.clear_focus(seq, now))
    }

    /// `ClientMsg::Cancel` or a lost connection: emits `Finished {
    /// Cancelled }`, interrupts the file system calls in flight, and makes
    /// [`Scan::run`] return.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.update(|s, now| s.cancel(now));
    }

    /// A `Delete` whose `hint` names directory `dir` of this scan, after the
    /// guard has confirmed the hint matches the resolved path. Call it
    /// **before** deleting anything: it emits `Pruned`, and the session then
    /// deletes and replies `Deleted`. Returns whether anything was pruned.
    pub fn prune(&self, dir: DirId) -> bool {
        self.update(|s, now| s.prune(dir, now))
    }
}

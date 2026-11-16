//! The focused folder, and the rule that steers workers towards it.
//!
//! With a focus F below the root (focus mode only), the waiting work splits
//! in two. *Focused* work is F's subtree plus the deferred stats of F's
//! parent: F cannot finish before those (G2), so they gate its answer.
//! *Background* work is everything else, F's siblings included.
//!
//! A free worker takes focused work, except that [`ScanState::bg_floor`]
//! workers always stay on the background so the rest of the scan keeps
//! moving. When one side has nothing ready, the other gets every worker, so
//! no worker idles while work exists. Running tasks are never interrupted: a
//! focus change only affects what workers pick next, and each worker's side
//! is recomputed from where its task is.
//!
//! The focus takes effect at once, with no ramp: a worker sent into a folder
//! the user only passes through still does useful work there, and the host
//! already combines rapid focus changes (150 ms).

use socketsweep_wire::{DirId, ScanEvent, ROOT_DIR};

use super::{Focus, Micros, ScanState, St};

/// Which side of the focus a task is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Focused,
    Background,
}

impl ScanState {
    /// `ClientMsg::Focus`. A `seq` not above the last one seen (for `Focus`
    /// or `ClearFocus`) is stale and ignored, as is an unknown or pruned
    /// directory (its `seq` is still used up). Returns whether the focus
    /// changed. Emits [`ScanEvent::FocusState`] at once, `settled` if the
    /// folder is already measured.
    pub fn set_focus(&mut self, seq: u32, dir: DirId, now: Micros) -> bool {
        if self.is_finished() || self.last_seq.is_some_and(|l| seq <= l) {
            return false;
        }
        self.last_seq = Some(seq);
        let Some(rec) = self.dirs.get(dir as usize) else {
            return false;
        };
        if rec.state == St::Pruned {
            return false;
        }
        let done = rec.state == St::Done;
        self.focus = Some(Focus {
            seq,
            dir,
            settled_sent: done,
        });
        self.out.push(ScanEvent::FocusState {
            seq,
            dir,
            settled: done,
        });
        self.tick(now);
        true
    }

    /// `ClientMsg::ClearFocus`: schedule as a plain parallel walk. Stale
    /// `seq`s are ignored as for [`ScanState::set_focus`].
    pub fn clear_focus(&mut self, seq: u32, now: Micros) -> bool {
        if self.is_finished() || self.last_seq.is_some_and(|l| seq <= l) {
            return false;
        }
        self.last_seq = Some(seq);
        self.focus = None;
        self.tick(now);
        true
    }

    /// The folder steering the workers. A focus at the root steers nothing,
    /// and in plain mode the focus is only acknowledged (`FocusState`).
    pub(super) fn steering(&self) -> Option<DirId> {
        let f = self.focus?.dir;
        (!self.plain && f != ROOT_DIR).then_some(f)
    }

    /// The side of focus `f` that directory `d`'s task is on.
    pub(super) fn lane_of(&self, d: DirId, f: Option<DirId>) -> Lane {
        match f {
            Some(f) if self.within(d, f) || d == self.dirs[f as usize].parent => Lane::Focused,
            _ => Lane::Background,
        }
    }

    /// Workers always kept on the background while it has work:
    /// `max(1, W/4)`, but never all of them. Every worker stays busy either
    /// way, so the split mostly decides order; the floor keeps the largest
    /// background jobs starting early, so they do not become the scan's tail
    /// after a long stay in one folder. Two of the default eight. A single
    /// worker serves the focus.
    pub(super) fn bg_floor(&self) -> u32 {
        let w = u32::from(self.workers_n);
        (w / 4).max(1).min(w - 1)
    }

    /// Which side a free worker's next task comes from, given focus `f`, or
    /// `None` if nothing is waiting.
    pub(super) fn choose_lane(&self, f: DirId) -> Option<Lane> {
        let p = self.dirs[f as usize].parent;
        let focused = self.dirs[f as usize].pending_desc + u32::from(self.own_task(p).is_some());
        let background = self.dirs[ROOT_DIR as usize].pending_desc - focused;
        let bg_active = self
            .workers
            .iter()
            .filter_map(|w| w.task)
            .filter(|t| self.lane_of(t.dir, Some(f)) == Lane::Background)
            .count() as u32;
        if background > 0 && (focused == 0 || bg_active < self.bg_floor()) {
            Some(Lane::Background)
        } else if focused > 0 {
            Some(Lane::Focused)
        } else {
            None
        }
    }
}

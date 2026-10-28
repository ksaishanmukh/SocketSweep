//! The focused folder: setting and clearing it.

use socketsweep_wire::{DirId, ScanEvent, ROOT_DIR};

use super::{Focus, Micros, ScanState, St};

impl ScanState {
    // ── Focus ───────────────────────────────────────────────────────────────

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
        // Re-opening the folder already open keeps its dwell.
        let since = match self.focus {
            Some(f) if f.dir == dir => f.since,
            _ => now,
        };
        self.focus = Some(Focus {
            seq,
            dir,
            since,
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

    /// The focused folder F and its parent P, when the focus is below the
    /// root. Focus at the root behaves as no focus: everything is Open. In
    /// plain mode the focus is only acknowledged (`FocusState`), never used
    /// for scheduling, so this is always `None` there.
    pub(crate) fn focus_fp(&self) -> Option<(DirId, DirId)> {
        if self.cfg.plain {
            return None;
        }
        let f = self.focus?;
        if f.dir == ROOT_DIR {
            None
        } else {
            Some((f.dir, self.dirs[f.dir as usize].parent))
        }
    }
}

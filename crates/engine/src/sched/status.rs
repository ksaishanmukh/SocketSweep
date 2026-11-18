//! Read-only views of a [`ScanState`].

use socketsweep_wire::{Outcome, ScanMode, Totals};

use super::{Micros, ScanState};

impl ScanState {
    /// Workers this scan uses.
    pub fn workers(&self) -> u8 {
        self.workers_n
    }

    /// How the scan ended, once [`ScanEvent::Finished`] has been emitted.
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// The root's totals so far (final once the scan is complete).
    pub fn root_totals(&self) -> Totals {
        self.dirs[0].totals
    }

    /// The mode this scan runs in.
    pub fn mode(&self) -> ScanMode {
        if self.plain {
            ScanMode::Plain
        } else {
            ScanMode::Focus
        }
    }

    /// When the skeleton phase ended, if it has (never in plain mode).
    pub fn skeleton_ready_at(&self) -> Option<Micros> {
        self.skeleton_ready_at
    }

    /// Directories known so far (the next new id).
    pub fn known(&self) -> u32 {
        self.dirs.len() as u32
    }

    /// Workers with a task in flight.
    pub fn busy(&self) -> usize {
        self.workers.iter().filter(|w| w.task.is_some()).count()
    }
}

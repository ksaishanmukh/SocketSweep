//! How a driver takes events out of a [`ScanState`] and sets its progress rate.

use socketsweep_wire::ScanEvent;

use super::{Micros, ScanState};

impl ScanState {
    /// Change how often [`ScanState::tick`] emits `Progress` (0 = never).
    pub fn set_progress_interval(&mut self, every: Micros) {
        self.progress_interval = every;
    }

    /// Remove and return the events produced so far, in order. The caller
    /// must hand them to the sink before releasing whatever lock guards this
    /// state.
    pub fn take_events(&mut self) -> Vec<ScanEvent> {
        std::mem::take(&mut self.out)
    }

    pub fn has_events(&self) -> bool {
        !self.out.is_empty()
    }
}

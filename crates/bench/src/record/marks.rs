//! The two events a scan is judged by, as the recorder keeps them.

use std::time::Duration;

use socketsweep_wire::{Outcome, Totals};

/// The `SkeletonReady` event as received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skeleton {
    pub at: Duration,
    /// The daemon's own figures.
    pub listed: u32,
    pub frontier: u32,
    pub elapsed_ms: u32,
    /// Levels completely listed when it arrived (the root is level 0, so 2
    /// means the root and every top-level folder).
    pub levels: u32,
}

/// The end of the scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    pub at: Duration,
    pub outcome: Outcome,
    pub totals: Totals,
    pub elapsed_ms: u32,
}

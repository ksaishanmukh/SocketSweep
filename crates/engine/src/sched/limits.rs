//! The scheduler's tuning constants.

use super::Micros;

/// A deferred stat job of at least this many entries is a "big job": it is
/// handed out before other work on its side of the focus, so one huge folder
/// does not become the scan's tail.
pub const BIG_JOB_ENTRIES: u32 = 2048;

/// How often [`ScanState::tick`](super::ScanState::tick) emits
/// [`ScanEvent::Progress`](socketsweep_wire::ScanEvent::Progress).
pub const PROGRESS_INTERVAL: Micros = 500_000;

/// Directories deeper than this below the root are not walked; they are
/// counted as entries and their parent gets
/// [`Totals::DEPTH_LIMIT`](socketsweep_wire::Totals::DEPTH_LIMIT). Bounds
/// the chain of parents every counter update walks.
pub const MAX_DEPTH: u8 = 64;

/// The skeleton phase ends after this long at the latest.
pub const SKELETON_BUDGET: Micros = 1_000_000;
/// The skeleton phase does not end on frontier size above this depth.
pub const SKELETON_MIN_DEPTH: usize = 2;
/// The skeleton phase ends once every level above this depth is listed.
pub const SKELETON_MAX_DEPTH: usize = 4;

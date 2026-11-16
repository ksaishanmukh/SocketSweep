//! How a scan is run, and the daemon's limits.

use serde::{Deserialize, Serialize};

/// How a scan is run. [`Default`] is what the host normally sends. Every
/// scheduling detail is the daemon's own: the host only picks the worker
/// count and the mode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOpts {
    /// Worker threads. `None` uses the daemon's default, `clamp(2 × cores,
    /// 4, 8)`.
    pub workers: Option<u8>,
    /// How the engine schedules the walk.
    pub mode: ScanMode,
}

/// How a scan is scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScanMode {
    /// Skeleton phase with deferred stats; free workers go to the focused
    /// folder first, with a floor of workers kept on the rest of the scan.
    #[default]
    Focus,
    /// The benchmark baseline: a straightforward parallel walk. No skeleton
    /// phase (so no `SkeletonReady`) and no deferred stats (every directory
    /// is read with `getdents` and `lstat` together), and the focus does not
    /// steer the workers; it is still acknowledged with `FocusState`, settled
    /// when the folder's `SubtreeDone` is sent. Workers spread over the root's
    /// subtrees (fewest workers, then most known work) and walk each depth
    /// first.
    Plain,
}

/// The daemon's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most workers [`ScanOpts::workers`] may ask for.
    pub max_workers: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_workers: 16 }
    }
}

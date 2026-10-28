//! How a scan is run: its options, the scheduling tiers and the daemon's
//! limits.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::{ScanEvent, ServerMsg, Totals};
use crate::{MAX_FRAME_BYTES, MAX_LISTING_PART, MAX_SCAN_BATCH_BYTES};

/// How a scan is run. [`Default`] is what the host normally sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanOpts {
    /// Worker threads. `None` uses [`Welcome::workers`](crate::Welcome::workers),
    /// which is `clamp(2 × cores, 4, 8)`.
    pub workers: Option<u8>,
    /// Directories deeper than this below the root are not walked; they
    /// complete at once with [`Totals::DEPTH_LIMIT`]. Clamped to
    /// [`Limits::max_depth`].
    pub max_depth: u16,
    /// The skeleton phase ends after this long at the latest.
    pub skeleton_budget_ms: u16,
    /// The skeleton phase does not end on frontier size before reaching this
    /// depth.
    pub skeleton_min_depth: u8,
    /// The skeleton phase ends on reaching this depth.
    pub skeleton_max_depth: u8,
    /// Do not cross into other file systems (bind mounts).
    pub one_filesystem: bool,
    /// Indexed by [`Tier`]: open, near, back.
    pub tiers: [TierCfg; 3],
    /// How long after a focus change the open tier's weight takes to ramp
    /// from 1 to its full weight.
    pub ramp_ms: u16,
    /// How the engine schedules the walk. Appended last, so every field
    /// before it keeps its encoding.
    pub mode: ScanMode,
}

/// How a scan is scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScanMode {
    /// The v4 scheduler: skeleton phase with deferred stats, then proximity
    /// allocation around the focused folder.
    #[default]
    Focus,
    /// The benchmark baseline (`docs/architecture.md`, section 6): what a
    /// straightforward parallel walk does. No skeleton phase (so no
    /// `SkeletonReady`) and no deferred stats (every directory is read with
    /// `getdents` and `lstat` together), and the focus does not steer the
    /// workers; it is still acknowledged with `FocusState`, settled when the
    /// folder's `SubtreeDone` is sent. Workers spread over the root's
    /// subtrees (fewest workers, then most known work) and walk each depth
    /// first.
    Plain,
}

impl Default for ScanOpts {
    fn default() -> Self {
        ScanOpts {
            workers: None,
            max_depth: Limits::default().max_depth,
            skeleton_budget_ms: 1000,
            skeleton_min_depth: 2,
            skeleton_max_depth: 4,
            one_filesystem: true,
            tiers: TierCfg::DEFAULTS,
            ramp_ms: 400,
            mode: ScanMode::Focus,
        }
    }
}

/// The three scheduling tiers, relative to the focused folder F and its parent
/// P. Not sent on its own; it indexes [`ScanOpts::tiers`] and
/// [`ScanEvent::Progress`]'s `alloc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// The subtree of F.
    Open = 0,
    /// The subtrees of F's siblings.
    Near = 1,
    /// Everything else.
    Back = 2,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Open, Tier::Near, Tier::Back];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// One tier's share of the workers. See `docs/architecture.md`, section 3,
/// "Proximity allocation".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TierCfg {
    /// D'Hondt weight. The open tier's ramps from 1.0 to this.
    pub weight: f32,
    /// Seats given before any are shared out by weight, or
    /// [`TierCfg::AUTO_FLOOR`].
    pub floor: u8,
    /// Most seats, or [`TierCfg::NO_CAP`].
    pub cap: u8,
}

impl TierCfg {
    /// A `floor` the daemon computes as `max(1, W / 4)`.
    pub const AUTO_FLOOR: u8 = u8::MAX;
    /// A `cap` that does not limit.
    pub const NO_CAP: u8 = u8::MAX;

    /// Open `{ 3.0, 0, none }`, near `{ 1.0, 1, 2 }`, back
    /// `{ 4.0, max(1, W/4), none }`.
    pub const DEFAULTS: [TierCfg; 3] = [
        TierCfg {
            weight: 3.0,
            floor: 0,
            cap: TierCfg::NO_CAP,
        },
        TierCfg {
            weight: 1.0,
            floor: 1,
            cap: 2,
        },
        TierCfg {
            weight: 4.0,
            floor: TierCfg::AUTO_FLOOR,
            cap: TierCfg::NO_CAP,
        },
    ];

    /// The floor in seats for a scan with `workers` workers.
    pub fn floor_for(&self, workers: u8) -> u8 {
        if self.floor == TierCfg::AUTO_FLOOR {
            (workers / 4).max(1)
        } else {
            self.floor
        }
    }
}

/// The daemon's limits, sent in [`Welcome`](crate::Welcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_frame_bytes: u32,
    /// Most entries in one listing part.
    pub max_listing_part: u32,
    /// Target bound on one [`ServerMsg::Scan`] batch, in encoded bytes.
    pub max_batch_bytes: u32,
    /// Deepest [`ScanOpts::max_depth`] honoured. The daemon's counters walk
    /// the chain of parents on every change, so it is bounded.
    pub max_depth: u16,
    /// Most workers [`ScanOpts::workers`] may ask for.
    pub max_workers: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_frame_bytes: MAX_FRAME_BYTES,
            max_listing_part: MAX_LISTING_PART as u32,
            max_batch_bytes: MAX_SCAN_BATCH_BYTES as u32,
            max_depth: 64,
            max_workers: 16,
        }
    }
}

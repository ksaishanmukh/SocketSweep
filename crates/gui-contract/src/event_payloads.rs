//! What the host's events carry.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

use crate::{NodeRef, SessionId, Totals, View};

// ── Events ──────────────────────────────────────────────────────────────────

/// Emitted as `scan-progress` about ten times a second while a scan runs,
/// and once more when it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub session: SessionId,
    pub phase: ScanPhase,
    pub totals: Totals,
    /// Folders listed so far, of `known` discovered.
    pub listed: u32,
    pub known: u32,
    pub elapsed_ms: u32,
    /// Set once the skeleton is ready: the top levels of the tree are listed
    /// and every folder in them can be opened.
    pub skeleton: Option<SkeletonReady>,
    /// The folder last passed to `focus`, and whether it is fully measured.
    pub focus: Option<FocusState>,
    /// Set on the last event of a scan.
    pub finished: Option<ScanFinished>,
    /// The focused folder, refreshed. Only that one, so the payload does not
    /// grow with the tree. Its sort and row cap are the `by` and `limit` of
    /// the last `focus` (defaults: disk, 500 rows); `getView` never changes
    /// them, so a GUI that wants more rows or another sort calls `focus` again.
    pub view: Option<View>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    /// Listing the top levels, breadth first.
    Skeleton,
    /// Measuring everything, nearest the focused folder first.
    Measuring,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonReady {
    /// Folders listed when the skeleton phase ended.
    pub listed: u32,
    /// Folders known but not yet listed.
    pub frontier: u32,
    pub elapsed_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    pub node: NodeRef,
    /// The folder's subtree is fully measured; the GUI clears its spinner.
    pub settled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinished {
    pub outcome: ScanOutcome,
    pub elapsed_ms: u32,
    /// Why, when the scan failed.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ScanOutcome {
    Complete,
    Cancelled,
    Failed,
}

/// Emitted as `copy-progress` while `copyToDesktop` runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyProgress {
    /// What is being copied.
    pub node: NodeRef,
    pub destination: String,
    pub files_done: u32,
    pub files_total: u32,
    #[specta(type = Number)]
    pub bytes_done: u64,
    #[specta(type = Number)]
    pub bytes_total: u64,
    /// False while the phone is still listing what to copy, so the totals
    /// are still growing.
    pub totals_known: bool,
    /// The file being copied, relative to what was asked for.
    pub current: Option<String>,
}

/// Emitted as `connection-lost` when the phone stops answering or is
/// unplugged. The session is over; nodes from it are stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionLost {
    pub serial: String,
    pub message: String,
}

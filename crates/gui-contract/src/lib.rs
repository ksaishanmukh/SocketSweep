//! The contract between the React GUI and the desktop host.
//!
//! Every command the GUI can invoke, every event the host emits, and every
//! value either carries is defined here as a plain Rust type, with no Tauri
//! dependency. `src/bindings.ts` is generated from this crate (see
//! [`bindings`]); the GUI imports nothing else from the host, and CI fails if
//! the checked-in file is stale. See `docs/architecture.md`, section 1.
//!
//! # Nodes, not paths
//!
//! The host holds the scanned tree and the byte-exact Android paths. The GUI
//! refers to a file or folder only by [`NodeRef`]: a path round-tripped
//! through a JavaScript string can come back subtly different, and it comes
//! back as a delete target.
//!
//! A [`NodeRef`] carries the [`SessionId`] of the tree it came from. Every
//! scan starts a new session, so a node held across a rescan or a reconnect
//! cannot name whatever has its id in the new tree: the host refuses it with
//! [`ErrorCode::Stale`].
//!
//! # Sizes
//!
//! Every size is a [`Size`]: apparent bytes and bytes used on disk. Disk usage
//! is the default display; which one the GUI sorts and draws by is the
//! [`SizeMetric`] it passes to each query.
//!
//! A scan fills sizes in progressively, so every node also carries a
//! [`Measure`]: `unknown` until its folder is listed (or, for a file, until
//! its size arrives), `partial` while the number is a lower bound that is
//! still climbing, `final` once its whole subtree is measured.
//!
//! # 64-bit numbers
//!
//! Byte counts are `u64` in Rust and `number` in TypeScript, which is exact up
//! to 2^53 bytes (8 PiB), far beyond any phone. Each such field is marked
//! `#[specta(type = specta_typescript::Number)]`, which is specta's explicit
//! opt-in for this; a `u64` without it fails the bindings export rather than
//! silently losing precision. Counts of files and folders are `u32`.
//!
//! # Errors
//!
//! A failed command rejects its promise with a [`CommandError`], never a bare
//! string.

pub mod bindings;
mod commands;
mod events;

pub use commands::{command_defs, CommandDef, Commands, COMMAND_NAMES};
pub use events::{event_defs, Event, EventDef, EVENT_NAMES};

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

/// Identifies one tree: a scan of one device. Assigned by the host, unique
/// for the life of the process, never 0.
pub type SessionId = u32;

/// A node's index within its session's tree. The scan root is 0.
pub type NodeId = u32;

/// A file or folder of one session's tree. See [nodes, not
/// paths](crate#nodes-not-paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeRef {
    pub session: SessionId,
    pub id: NodeId,
}

/// A size in bytes, both ways of counting it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Size {
    /// The sum of file lengths, what a file manager shows.
    #[specta(type = Number)]
    pub apparent: u64,
    /// Space allocated on the phone's storage, what deleting frees.
    #[specta(type = Number)]
    pub disk: u64,
}

/// How far a node's size is known. See [sizes](crate#sizes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Measure {
    /// Not listed yet (a folder) or not stat'ed yet (a file). The GUI shows
    /// "measuring…", not 0.
    Unknown,
    /// A lower bound that climbs as the scan goes on.
    Partial,
    /// Everything below is measured.
    Final,
}

/// Which [`Size`] a query sorts and sizes by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SizeMetric {
    #[default]
    Disk,
    Apparent,
}

/// What a node is. Symbolic links are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    /// Sockets, pipes, devices.
    Other,
}

// ── Devices and sessions ────────────────────────────────────────────────────

/// An attached phone, usable or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// Human-readable model for the picker; falls back to the serial.
    pub model: String,
    pub state: DeviceState,
    /// True only when `state` is `ready`.
    pub usable: bool,
    /// What to do about a device that cannot be used yet, when the host knows.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceState {
    Ready,
    /// Waiting for the user to accept the debugging prompt on the phone.
    Unauthorized,
    Offline,
    /// The computer will not let adb open it (Linux without a udev rule).
    NoPermission,
    Other,
}

/// The answer to `connect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub serial: String,
    pub model: String,
    /// The root that scans default to, as the user knows it (`/sdcard`).
    pub root: String,
    /// Where `copyToDesktop` copies when no destination is given, as a path on
    /// this computer (the desktop), so the GUI can name the action up front.
    /// `null` when the host knows no default; a copy must then name one.
    pub copy_destination: Option<String>,
    /// The last finished scan of this device and root, if it has been seen
    /// before, for showing what changed.
    pub previous: Option<ScanRecord>,
}

/// A remembered scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub size: Size,
    pub files: u32,
    pub dirs: u32,
    /// Unix seconds. The GUI formats it.
    #[specta(type = Number)]
    pub at: u64,
}

/// The answer to `startScan`: the scan is under way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanStarted {
    /// The new tree. Every node from an earlier session is now stale.
    pub session: SessionId,
    pub root: NodeRef,
    /// The root as the user knows it, for the first breadcrumb.
    pub path: String,
}

/// Sums over a tree or subtree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub size: Size,
    pub files: u32,
    pub dirs: u32,
    /// Folders that could not be read and files that could not be measured.
    pub errors: u32,
}

// ── Tree queries ────────────────────────────────────────────────────────────

/// One file or folder in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub node: NodeRef,
    /// Lossy UTF-8, for display only. Actions travel by `node`.
    pub name: String,
    pub kind: EntryKind,
    pub size: Size,
    pub measure: Measure,
    /// Files in the subtree; 1 for a file.
    pub files: u32,
    /// Some folder in the subtree could not be read, so the size is low.
    pub unreadable: bool,
    /// The containing folder's path, on results that span the tree (search
    /// hits, largest files), where the name alone does not say where the thing
    /// lives. `null` inside a single folder's listing.
    pub parent: Option<String>,
    /// That folder, for "go to".
    pub parent_node: Option<NodeRef>,
}

/// One folder's contents, largest first by the requested [`SizeMetric`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub node: NodeRef,
    /// Display path.
    pub path: String,
    pub size: Size,
    pub measure: Measure,
    pub files: u32,
    pub dirs: u32,
    pub unreadable: bool,
    pub rows: Vec<Row>,
    /// Rows beyond the requested limit, so the GUI can say "and 412 more".
    pub hidden: u32,
}

/// One step of the path from the root to a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub node: NodeRef,
    pub name: String,
}

/// A node plus a bounded slice of its descendants, for the nested treemap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TreemapNode {
    pub node: NodeRef,
    pub name: String,
    pub kind: EntryKind,
    pub size: Size,
    pub measure: Measure,
    /// Largest first; empty below the requested depth.
    pub children: Vec<TreemapNode>,
}

/// Storage owned by one app, summed across its data, obb and media folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    /// Android package name, e.g. `com.example.game`. Not a display label:
    /// adb's shell has no access to app labels.
    pub package: String,
    pub size: Size,
    pub files: u32,
    /// The largest folder the package owns, for navigating to it.
    pub node: NodeRef,
}

/// Total size of one broad file category (images, video, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TypeGroup {
    pub label: String,
    pub size: Size,
    pub files: u32,
}

// ── Actions ─────────────────────────────────────────────────────────────────

/// The answer to `delete`, once the phone confirms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    /// Files and folders removed.
    #[specta(type = Number)]
    pub items: u64,
    /// The whole tree's totals afterwards.
    pub totals: Totals,
    /// The focused folder, refreshed; its nearest surviving ancestor if it
    /// was inside what was deleted.
    pub view: View,
}

/// The answer to `copyToDesktop`, once every file is copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    /// Where the copy is, on this computer.
    pub destination: String,
    pub files: u32,
    #[specta(type = Number)]
    pub bytes: u64,
    /// Symbolic links and special files, which are not copied.
    pub skipped: u32,
    /// Files whose Android name is not valid here, copied under a changed
    /// name.
    pub renamed: u32,
}

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

// ── Errors ──────────────────────────────────────────────────────────────────

/// How every command fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: ErrorCode,
    /// For the user, already worded.
    pub message: String,
}

impl CommandError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        CommandError {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// No device is connected.
    NotConnected,
    /// Connected, but no scan has started.
    NoScan,
    /// The node is from an earlier session. The GUI refreshes.
    Stale,
    /// The node has since been deleted.
    Removed,
    /// Another operation that cannot overlap this one is running.
    Busy,
    /// adb or the phone refused or failed.
    Device,
    /// The daemon on the phone refused or failed.
    Daemon,
    /// The user cancelled it.
    Cancelled,
    /// The arguments make no sense, e.g. deleting the scan root.
    Invalid,
    /// A file on this computer could not be written.
    Io,
}

// ── Defaults ────────────────────────────────────────────────────────────────

/// Rows per view when `limit` is not given. Enough to fill any screen; the
/// GUI virtualises and asks for more if it needs them.
pub const DEFAULT_VIEW_LIMIT: u32 = 500;
/// Levels below the requested node in a treemap when `depth` is not given.
pub const DEFAULT_TREEMAP_DEPTH: u32 = 2;
/// Results from `largestFiles` when `limit` is not given.
pub const DEFAULT_LARGEST_LIMIT: u32 = 100;
/// Results from `appBreakdown` when `limit` is not given.
pub const DEFAULT_APPS_LIMIT: u32 = 50;
/// Results from `search` when `limit` is not given.
pub const DEFAULT_SEARCH_LIMIT: u32 = 200;
/// How often `scan-progress` is emitted while a scan runs.
pub const SCAN_PROGRESS_INTERVAL_MS: u32 = 100;

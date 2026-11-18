//! The contract between the React GUI and the desktop host.
//!
//! The values the GUI's commands take and return, and the events the host
//! emits, as plain Rust types with no Tauri dependency (the optional `tauri`
//! feature only registers the event payloads). The commands themselves are
//! declared once, in `src-tauri`, which generates `src/bindings.ts` from them
//! and these types; the GUI imports nothing else from the host, and a test
//! fails if the checked-in file is stale.
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

mod actions;
mod devices;
mod errors;
mod event_payloads;
mod filters;
mod nodes;
mod overview;
mod queries;

pub use actions::{CopyResult, DeleteFailure, DeletedMany};
pub use devices::{Connected, Device, DeviceState, ScanRecord, ScanStarted, Totals};
pub use errors::{CommandError, ErrorCode};
pub use event_payloads::{
    AppsUpdated, ConnectionLost, CopyProgress, FocusState, ScanFinished, ScanOutcome, ScanPhase,
    ScanProgress, SkeletonReady,
};
pub use filters::{ChangedBucket, KindFilter, SearchFilter, SortKey, ViewFilter, ViewSort};
pub use nodes::{
    AppTag, Category, EntryKind, Measure, NodeId, NodeRef, SessionId, Size, SizeMetric,
};
pub use overview::{
    AppsStatus, CategoryUsage, Leftover, Leftovers, Overview, SinceLastScan, UnreadableFolder,
};
pub use queries::{AppUsage, Crumb, Row, SearchRequest, TreemapNode, View, ViewRequest};

/// Rows per view when `limit` is not given. Enough to fill any screen; the
/// GUI virtualises and asks for more if it needs them.
pub const DEFAULT_VIEW_LIMIT: u32 = 500;
/// Levels below the requested node in a treemap when `depth` is not given.
pub const DEFAULT_TREEMAP_DEPTH: u32 = 2;
/// Results from `search` when `limit` is not given.
pub const DEFAULT_SEARCH_LIMIT: u32 = 200;
/// Files in `overview`'s `biggestFiles`.
pub const OVERVIEW_BIGGEST_FILES: u32 = 10;
/// Apps in `overview`'s `apps`.
pub const OVERVIEW_APPS: u32 = 8;
/// How often `scan-progress` is emitted while a scan runs.
pub const SCAN_PROGRESS_INTERVAL_MS: u32 = 100;

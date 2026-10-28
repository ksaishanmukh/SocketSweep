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

//! # Layout
//!
//! The value types live in `nodes`, `devices`, `queries`, `actions`,
//! `event_payloads` and `errors`, all re-exported here; [`Commands`] and
//! [`Event`] list the commands and events, and [`bindings`] renders them.

mod actions;
pub mod bindings;
mod commands;
mod devices;
mod errors;
mod event_payloads;
mod events;
mod nodes;
mod queries;

pub use actions::{CopyResult, Deleted};
pub use commands::{command_defs, CommandDef, Commands, COMMAND_NAMES};
pub use devices::{Connected, Device, DeviceState, ScanRecord, ScanStarted, Totals};
pub use errors::{CommandError, ErrorCode};
pub use event_payloads::{
    ConnectionLost, CopyProgress, FocusState, ScanFinished, ScanOutcome, ScanPhase, ScanProgress,
    SkeletonReady,
};
pub use events::{event_defs, Event, EventDef, EVENT_NAMES};
pub use nodes::{EntryKind, Measure, NodeId, NodeRef, SessionId, Size, SizeMetric};
pub use queries::{AppUsage, Crumb, Row, TreemapNode, TypeGroup, View};

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

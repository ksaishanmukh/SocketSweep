//! Everything after the handshake. See [requests and replies](crate#requests-and-replies).
//!
//! Field and variant order is the wire format: add new variants at the end and
//! bump [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) with any change.

use serde::{Deserialize, Serialize};

use crate::{DirId, ReqId, BLOCK_SIZE, MAX_FRAME_BYTES, MAX_LISTING_PART, MAX_SCAN_BATCH_BYTES};

// ── Host → daemon ───────────────────────────────────────────────────────────

/// One request or hint from the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientFrame {
    /// Chosen by the host, unique among its requests still in progress, and
    /// never 0. Hints (`Focus`, `ClearFocus`, `Cancel`) take an id too, which
    /// nothing answers.
    pub id: ReqId,
    pub msg: ClientMsg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Answered with [`ServerMsg::Pong`] carrying the same nonce. Sent after
    /// [`HOST_PING_INTERVAL`](crate::HOST_PING_INTERVAL) idle.
    Ping { nonce: u32 },
    /// Walk `root`, streaming [`ServerMsg::Scan`] batches until
    /// [`ScanEvent::Finished`]. The id of this frame is the scan's id in
    /// `Focus`, `ClearFocus`, `Cancel` and `Delete.hint`.
    StartScan { root: Vec<u8>, opts: ScanOpts },
    /// The user opened directory `dir` of scan `scan`. A hint, never answered
    /// directly; the scheduler moves workers towards it and reports
    /// [`ScanEvent::FocusState`]. A `seq` lower than one already seen for the
    /// scan is stale and ignored.
    Focus { scan: ReqId, dir: DirId, seq: u32 },
    /// The user is no longer in any directory of the scan (or is at its root):
    /// schedule as a plain parallel walk. Shares `seq` numbering with `Focus`.
    ClearFocus { scan: ReqId, seq: u32 },
    /// Stop request `target`. A hint: a scan still ends with
    /// [`ScanEvent::Finished`] (outcome [`Outcome::Cancelled`]), a delete or
    /// copy with [`ServerMsg::Failed`] ([`ErrCode::Cancelled`]) unless it had
    /// already finished.
    Cancel { target: ReqId },
    /// Recursively delete `path`, answered with [`ServerMsg::Deleted`].
    ///
    /// The path is what is acted on; the daemon resolves it through its guard
    /// (canonicalise the parent, never follow the final component) and
    /// refuses anything outside the allowed roots. `hint` names the same
    /// thing as `(scan, dir)` so a running scan can prune it; the daemon uses
    /// it only if it matches the path the guard resolved.
    Delete {
        path: Vec<u8>,
        hint: Option<(ReqId, DirId)>,
    },
    /// List everything under `path` for copying, answered with
    /// [`ServerMsg::CopyManifest`] parts. Requires [`Caps::COPY_MANIFEST`](crate::Caps::COPY_MANIFEST).
    PrepareCopy { path: Vec<u8> },
    /// Stop the daemon. Not answered; the connection closes.
    Shutdown,
}

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

// ── Daemon → host ───────────────────────────────────────────────────────────

/// One reply or event from the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerFrame {
    /// The request this answers, or [`SESSION_LEVEL`](crate::SESSION_LEVEL).
    pub id: ReqId,
    pub msg: ServerMsg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    /// Answers [`ClientMsg::Ping`].
    Pong { nonce: u32 },
    /// Session-level: nothing else has been sent for
    /// [`HEARTBEAT_INTERVAL`](crate::HEARTBEAT_INTERVAL). Readers only note
    /// that the daemon is alive.
    Heartbeat,
    /// Scan events in order, batched to at most
    /// [`MAX_SCAN_BATCH_BYTES`](crate::MAX_SCAN_BATCH_BYTES) encoded.
    Scan(Vec<ScanEvent>),
    /// A delete is under way: `items` removed so far.
    DeleteProgress { items: u64 },
    /// Ends a delete: `items` removed in all, and the totals of what was
    /// removed, so the host can subtract them.
    Deleted { items: u64, totals: Totals },
    /// Part of the answer to [`ClientMsg::PrepareCopy`]. `root` is the
    /// resolved absolute path of what was asked for, the same in every part;
    /// each item's `rel` is relative to it. The part with `last` ends the
    /// request.
    CopyManifest {
        root: Vec<u8>,
        files: Vec<CopyItem>,
        last: bool,
    },
    /// Ends the request it answers, which did not succeed.
    Failed {
        code: ErrCode,
        /// The `errno` behind it, when there was one.
        errno: Option<i32>,
        /// For logs and the user. Not parsed.
        message: String,
    },
}

/// One step of a scan. See [ordering guarantees](crate#ordering-guarantees).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanEvent {
    /// The first event of every scan. `root` is the path the daemon resolved
    /// (`/sdcard` is a symlink chain to `/storage/emulated/0`), and it is
    /// directory [`ROOT_DIR`](crate::ROOT_DIR). `dev` is its file system, for
    /// `one_filesystem`; `workers` how many workers this scan uses.
    Started {
        root: Vec<u8>,
        dev: u64,
        workers: u8,
    },
    /// Part `part` (from 0) of directory `dir`'s entries, at most
    /// [`MAX_LISTING_PART`](crate::MAX_LISTING_PART) of them. `last` marks
    /// the final part. Introduces every `id` it carries.
    Listing {
        dir: DirId,
        part: u16,
        last: bool,
        entries: Vec<Entry>,
    },
    /// Deferred stats for `dir`'s entries `start..start + stats.len()`,
    /// numbered by position across all its listing parts. `None` is an entry
    /// that could not be stat'ed (it vanished, or permission was denied); it
    /// counts in [`Totals::errors`].
    Stats {
        dir: DirId,
        start: u32,
        stats: Vec<Option<Stat>>,
    },
    /// `dir` could not be read. It is still finished with a `SubtreeDone`,
    /// flagged [`Totals::UNREADABLE`].
    DirFailed { dir: DirId, op: FsOp, errno: i32 },
    /// Everything below `dir` is measured. The daemon's `totals` are final
    /// and win over whatever the host had added up.
    SubtreeDone { dir: DirId, totals: Totals },
    /// The skeleton phase ended: `listed` directories are listed, `frontier`
    /// more are known but not yet listed.
    SkeletonReady {
        listed: u32,
        frontier: u32,
        elapsed_ms: u32,
    },
    /// Where the scheduler is with focus `seq`: `settled` once `dir`'s
    /// subtree is fully measured.
    FocusState { seq: u32, dir: DirId, settled: bool },
    /// `dir` was deleted during the scan. It and everything below it are gone
    /// and will not be mentioned again; its totals are already removed from
    /// its ancestors.
    Pruned { dir: DirId },
    /// Periodic. Directories `listed` of `known`; `files` and `disk` counted
    /// so far; `pending` tasks waiting; `alloc` workers per [`Tier`]; and the
    /// directories being read for longer than
    /// [`STALL_THRESHOLD`](crate::STALL_THRESHOLD), with milliseconds so far.
    Progress {
        listed: u32,
        known: u32,
        files: u64,
        disk: u64,
        pending: u32,
        alloc: [u8; 3],
        stalled: Vec<(DirId, u32)>,
    },
    /// The last event of every scan that started.
    Finished {
        outcome: Outcome,
        totals: Totals,
        elapsed_ms: u32,
    },
}

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The name's exact bytes.
    pub name: Vec<u8>,
    pub kind: Kind,
    /// `None` in a skeleton listing; the stats follow in [`ScanEvent::Stats`].
    pub stat: Option<Stat>,
    /// `Some` exactly when this is a directory that will be walked.
    pub id: Option<DirId>,
}

/// The type of an entry, from `d_type` (or `lstat` where `d_type` is
/// `DT_UNKNOWN`). Symbolic links are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    File,
    Dir,
    Symlink,
    /// Sockets, pipes, devices.
    Other,
}

/// What `lstat` said about an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    /// `st_size`.
    pub apparent: u64,
    /// `st_blocks`, in [`BLOCK_SIZE`] units. See [`Caps::DISK_USAGE`](crate::Caps::DISK_USAGE).
    pub blocks: u64,
    /// `st_mtime`, Unix seconds.
    pub mtime: i64,
}

impl Stat {
    /// Space used on disk, in bytes.
    pub fn disk(&self) -> u64 {
        self.blocks.saturating_mul(BLOCK_SIZE)
    }
}

/// Sums over a subtree: everything strictly below a directory, its
/// subdirectories' own sizes included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub apparent: u64,
    pub disk: u64,
    /// Entries that are not directories.
    pub files: u32,
    pub dirs: u32,
    /// Directories that could not be read plus entries that could not be
    /// stat'ed.
    pub errors: u32,
    /// [`Totals::UNREADABLE`] and the like, or'ed together up the tree.
    pub flags: u8,
}

impl Totals {
    /// Some directory in the subtree could not be read, so the sums are low.
    pub const UNREADABLE: u8 = 1 << 0;
    /// Some directory lay beyond [`ScanOpts::max_depth`] and was not walked.
    pub const DEPTH_LIMIT: u8 = 1 << 1;
    /// Some mount point was not crossed because of
    /// [`ScanOpts::one_filesystem`].
    pub const OTHER_FS: u8 = 1 << 2;
    /// Some disk figure is estimated; see [`Caps::DISK_USAGE`](crate::Caps::DISK_USAGE).
    pub const DISK_ESTIMATED: u8 = 1 << 3;

    /// Add a child's totals into these.
    pub fn add(&mut self, other: &Totals) {
        self.apparent = self.apparent.saturating_add(other.apparent);
        self.disk = self.disk.saturating_add(other.disk);
        self.files = self.files.saturating_add(other.files);
        self.dirs = self.dirs.saturating_add(other.dirs);
        self.errors = self.errors.saturating_add(other.errors);
        self.flags |= other.flags;
    }

    /// Remove a deleted child's totals from these. Flags stay: they say what
    /// happened during the scan, not what is left.
    pub fn subtract(&mut self, other: &Totals) {
        self.apparent = self.apparent.saturating_sub(other.apparent);
        self.disk = self.disk.saturating_sub(other.disk);
        self.files = self.files.saturating_sub(other.files);
        self.dirs = self.dirs.saturating_sub(other.dirs);
        self.errors = self.errors.saturating_sub(other.errors);
    }
}

/// How a scan ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// Every directory was finished or pruned.
    Complete,
    /// By [`ClientMsg::Cancel`], or a lost connection.
    Cancelled,
    /// The daemon could not go on; the totals cover what was done.
    Aborted { code: ErrCode },
}

/// The file system call that failed in [`ScanEvent::DirFailed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsOp {
    Open,
    ReadDir,
    Stat,
}

/// Why a request failed, in [`ServerMsg::Failed`] or [`Outcome::Aborted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrCode {
    /// Not a valid request: id 0, an unknown scan in a hint, a malformed path.
    BadRequest,
    /// The id is already in use by a request still in progress.
    DuplicateId,
    /// Another request of this kind is running, and only one may.
    Busy,
    /// The capability the request needs was not negotiated.
    Unsupported,
    /// `ENOENT`.
    NotFound,
    /// `EACCES` or `EPERM`.
    PermissionDenied,
    /// The guard resolved the path to somewhere outside the allowed roots.
    OutsideRoot,
    /// The path is an allowed root itself, which is never deleted.
    ProtectedRoot,
    /// Stopped by [`ClientMsg::Cancel`].
    Cancelled,
    /// Any other file system error; see the `errno`.
    Io,
    /// A bug in the daemon.
    Internal,
}

/// One item of a [`ServerMsg::CopyManifest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyItem {
    /// Relative to the manifest's `root`; empty for the root itself.
    pub rel: Vec<u8>,
    /// Files are pulled and directories created; symbolic links and other
    /// kinds are listed so the host can report them as skipped.
    pub kind: Kind,
    /// `st_size`; 0 for anything but a file.
    pub size: u64,
    /// `st_mode` permission bits.
    pub mode: u32,
    /// `st_mtime`, Unix seconds.
    pub mtime: i64,
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

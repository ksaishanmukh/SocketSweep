//! Daemon → host: replies, scan events and how requests end.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::{ClientMsg, Tier};
use super::{CopyItem, Entry, Stat, Totals};
use crate::{DirId, ReqId};

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

/// One step of a scan. Events obey the crate's ordering guarantees.
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

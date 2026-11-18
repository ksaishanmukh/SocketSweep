//! The file system contract the engine walks through ([`Fs`]).
//!
//! The phone daemon implements it with rustix
//! (`openat(O_DIRECTORY | O_NOFOLLOW)`, `getdents64`,
//! `statat(AT_SYMLINK_NOFOLLOW)`); [`StdFs`](crate::StdFs) is a portable
//! `std::fs` version for tests and host tools.
//!
//! Paths are bytes end to end. The engine builds a directory's path by joining
//! the scan root and each name with `/`.

mod interrupt;

pub mod errno;

pub use interrupt::Interrupt;

use socketsweep_wire::{FsOp, Kind, Stat};

/// What `lstat` said about one entry, with the identity the engine needs for
/// hardlink de-duplication. The wire [`Stat`] carries no inode, so the engine
/// de-duplicates before it sums totals ([`ScanState`](crate::ScanState)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsStat {
    /// The entry's type from `st_mode`.
    pub kind: Kind,
    pub stat: Stat,
    /// `st_dev`.
    pub dev: u64,
    /// `st_ino`.
    pub ino: u64,
    /// `st_nlink`. Files with more than one link are counted once per
    /// `(dev, ino)` in a scan.
    pub nlink: u64,
}

/// One entry's stat, or the `errno` that stopped it.
pub type StatResult = Result<FsStat, i32>;

/// One directory entry as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    /// The name's exact bytes, never `.` or `..`.
    pub name: Vec<u8>,
    /// From `d_type`. Where `d_type` is `DT_UNKNOWN` the implementation must
    /// `lstat` the entry at once and take the kind from `st_mode` (or report
    /// [`Kind::Other`] when that `lstat` fails).
    pub kind: Kind,
    /// [`Fs::list_full`]: always `Some`. [`Fs::read_names`]: `Some` only for
    /// entries that had to be stat'ed because of `DT_UNKNOWN`; the engine
    /// never stats those again.
    pub stat: Option<StatResult>,
}

/// A directory's entries, plus the device the directory itself is on (one
/// `fstat` of the open handle), so the scan can stay on the root's file
/// system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirRead {
    pub dev: u64,
    pub entries: Vec<FsEntry>,
}

/// Why an [`Fs`] call did not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// The call failed; for a directory read this is reported as
    /// [`ScanEvent::DirFailed`](socketsweep_wire::ScanEvent::DirFailed).
    Failed { op: FsOp, errno: i32 },
    /// The [`Interrupt`] was raised part-way.
    Interrupted,
}

/// The resolved scan root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootInfo {
    /// The resolved path, sent in [`ScanEvent::Started`](socketsweep_wire::ScanEvent::Started).
    pub path: Vec<u8>,
    /// `st_dev` of the root.
    pub dev: u64,
}

/// Reads directories and stats entries.
///
/// Implementations must be safe to call from many worker threads at once,
/// never follow symbolic links (neither the final component nor, for the
/// real daemon, any directory being walked: `O_NOFOLLOW`), never return `.`
/// or `..`, and poll the [`Interrupt`] at least every
/// [`CANCEL_CHECK_ENTRIES`](socketsweep_wire::CANCEL_CHECK_ENTRIES) entries ([`Interrupt::poll`]).
pub trait Fs: Send + Sync {
    /// Resolve the scan root and check it is a directory. A failure here
    /// fails the `StartScan` request before `Started`.
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError>;

    /// `getdents` only: names and `d_type`, no stats (except for
    /// `DT_UNKNOWN`, see [`FsEntry::kind`]). Used for skeleton listings.
    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError>;

    /// `lstat` each of `names` inside `dir` (one handle on `dir`, then
    /// `statat` per name), in order. An error opening `dir` fails the whole
    /// call; an error on one name is that name's `Err`.
    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError>;

    /// `getdents` and `lstat` together: every entry's `stat` is `Some`.
    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError>;
}

impl<T: Fs + ?Sized> Fs for &T {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        (**self).root(path)
    }
    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        (**self).read_names(dir, intr)
    }
    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError> {
        (**self).stat_all(dir, names, intr)
    }
    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        (**self).list_full(dir, intr)
    }
}

impl<T: Fs + ?Sized> Fs for std::sync::Arc<T> {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        (**self).root(path)
    }
    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        (**self).read_names(dir, intr)
    }
    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError> {
        (**self).stat_all(dir, names, intr)
    }
    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        (**self).list_full(dir, intr)
    }
}

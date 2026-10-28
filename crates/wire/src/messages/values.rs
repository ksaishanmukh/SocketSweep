//! The values messages carry: directory entries, stats, totals and copy
//! manifest items.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::{ScanEvent, ScanOpts, ServerMsg};
use crate::{DirId, BLOCK_SIZE};

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
    /// Newest `st_mtime` (Unix seconds) among the entries counted here,
    /// directories included; 0 when nothing below has a known mtime. A high-water
    /// mark: deleting entries never lowers it.
    pub newest_mtime: i64,
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
        self.newest_mtime = self.newest_mtime.max(other.newest_mtime);
    }

    /// Remove a deleted child's totals from these. Flags and `newest_mtime`
    /// stay: flags say what happened during the scan, and a maximum cannot be
    /// undone without rescanning the siblings.
    pub fn subtract(&mut self, other: &Totals) {
        self.apparent = self.apparent.saturating_sub(other.apparent);
        self.disk = self.disk.saturating_sub(other.disk);
        self.files = self.files.saturating_sub(other.files);
        self.dirs = self.dirs.saturating_sub(other.dirs);
        self.errors = self.errors.saturating_sub(other.errors);
    }
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

//! The node and directory records a [`Tree`](super::Tree) is built from.

use socketsweep_gui_contract::{EntryKind, Measure, NodeId, Size};
use socketsweep_wire::{DirId, Kind, Stat, Totals};

/// "No node" in a link. The root is 0, so 0 cannot mean absent.
pub(super) const NONE: u32 = u32::MAX;

#[derive(Debug)]
pub(super) struct Node {
    pub(super) name: Box<[u8]>,
    pub(super) parent: NodeId,
    pub(super) first_child: NodeId,
    pub(super) last_child: NodeId,
    pub(super) next_sibling: NodeId,
    pub(super) kind: Kind,
    /// The walked directory's id, or [`NONE`].
    pub(super) dir: DirId,
    /// This entry's own `lstat` sizes.
    pub(super) own: Size,
    /// Strictly below; zero for anything but a directory.
    pub(super) sub: Totals,
    pub(super) measure: Measure,
    /// Its own `lstat` has arrived (or failed).
    pub(super) stat_known: bool,
    /// Its `lstat` failed, which counts as one error in its parent.
    pub(super) stat_failed: bool,
    pub(super) removed: bool,
}

impl Node {
    pub(super) fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    /// What the GUI shows: own plus everything below.
    pub(super) fn size(&self) -> Size {
        Size {
            apparent: self.own.apparent.saturating_add(self.sub.apparent),
            disk: self.own.disk.saturating_add(self.sub.disk),
        }
    }

    /// Everything this node contributes to its parent's sub totals.
    pub(super) fn contribution(&self) -> Totals {
        let mut t = self.sub;
        t.apparent = t.apparent.saturating_add(self.own.apparent);
        t.disk = t.disk.saturating_add(self.own.disk);
        if self.is_dir() {
            t.dirs = t.dirs.saturating_add(1);
        } else {
            t.files = t.files.saturating_add(1);
        }
        if self.stat_failed {
            t.errors = t.errors.saturating_add(1);
        }
        t
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirState {
    /// Introduced by its parent's listing (or `Started`), no listing yet.
    Introduced,
    /// Some listing parts have arrived.
    Listing,
    /// Every listing part has arrived.
    Listed,
    /// `SubtreeDone` arrived.
    Done,
    /// Deleted or pruned: later events about it are dropped.
    Gone,
}

#[derive(Debug)]
pub(super) struct DirRec {
    pub(super) node: NodeId,
    pub(super) state: DirState,
    pub(super) next_part: u16,
    /// Child nodes in listing order, for `Stats` positions. Dropped once done.
    pub(super) entries: Vec<NodeId>,
    /// Walked child directories not yet done or gone, to check that a directory
    /// finishes only after its children.
    pub(super) open: u32,
}

pub(super) fn size_of(stat: &Stat) -> Size {
    Size {
        apparent: stat.apparent,
        disk: stat.disk(),
    }
}

pub(super) fn entry_kind(kind: Kind) -> EntryKind {
    match kind {
        Kind::File => EntryKind::File,
        Kind::Dir => EntryKind::Dir,
        Kind::Symlink => EntryKind::Symlink,
        Kind::Other => EntryKind::Other,
    }
}

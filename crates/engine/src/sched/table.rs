//! The directory table: one row per known directory, and the counters kept
//! along each chain of parents.

use socketsweep_wire::{DirId, Tier, Totals, ROOT_DIR};

use super::{Micros, ScanState, TaskKind, NONE};
use crate::fs::StatResult;

/// Where a directory is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum St {
    /// Known (introduced by its parent's listing), not yet read.
    Pending,
    /// A `Names` or `Full` task is reading it.
    InFlight,
    /// Listed by `Names`; its stats are owed.
    NamesOnly,
    /// Listed and stat'ed; waiting for its children (or its parent's stats).
    Listed,
    /// `SubtreeDone` sent.
    Done,
    /// Deleted during the scan (this or an ancestor).
    Pruned,
}

/// The deferred-stat job of a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatSt {
    Pending,
    InFlight,
    /// Done, or never needed (`Full` listing).
    Done,
}

/// What a directory's own entry contributed to its parent's totals, so a
/// prune can take it out again.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Own {
    /// The parent has applied this entry's stat.
    pub counted: bool,
    /// ... and it failed.
    pub err: bool,
    pub apparent: u64,
    pub disk: u64,
}

/// One row of the directory table. Children of a directory get consecutive
/// ids when it is listed, so they are `first_child .. first_child +
/// n_children` (the spec's sibling list, without the links).
#[derive(Debug, Clone)]
pub(crate) struct DirRec {
    pub parent: DirId,
    pub name: Box<[u8]>,
    pub depth: u8,
    pub first_child: DirId,
    pub n_children: u32,
    pub state: St,
    pub stat: StatSt,
    /// Runnable tasks in this subtree, including this directory's own.
    pub pending_desc: u32,
    /// Workers currently running a task in this subtree.
    pub active_desc: u16,
    /// Child directories not yet `Done` (pruned ones are subtracted).
    pub open_children: u32,
    /// Sums over everything strictly below this directory.
    pub totals: Totals,
    pub own: Own,
}

impl DirRec {
    pub(crate) fn new(parent: DirId, name: Box<[u8]>, depth: u8) -> Self {
        DirRec {
            parent,
            name,
            depth,
            first_child: NONE,
            n_children: 0,
            state: St::Pending,
            stat: StatSt::Done,
            pending_desc: 1,
            active_desc: 0,
            open_children: 0,
            totals: Totals::default(),
            own: Own::default(),
        }
    }

    pub(crate) fn children(&self) -> std::ops::Range<DirId> {
        if self.n_children == 0 {
            0..0
        } else {
            self.first_child..self.first_child + self.n_children
        }
    }
}

/// One entry of a `Names` listing whose stat is owed.
#[derive(Debug, Clone)]
pub(crate) struct DeferredEnt {
    /// Moved into the `StatDir` task when it is handed out.
    pub name: Vec<u8>,
    /// Already stat'ed because of `DT_UNKNOWN`.
    pub early: Option<StatResult>,
    /// The child's id when this entry is a directory that is walked.
    pub child: DirId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Running {
    pub dir: DirId,
    pub kind: TaskKind,
    pub since: Micros,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkerSlot {
    pub task: Option<Running>,
    pub prev_tier: Option<Tier>,
    /// The subtree the worker last descended in, or [`NONE`].
    pub anchor: Option<DirId>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Focus {
    pub seq: u32,
    pub dir: DirId,
    pub since: Micros,
    pub settled_sent: bool,
}

impl ScanState {
    /// The full path of a known directory.
    pub fn path_of(&self, dir: DirId) -> Vec<u8> {
        let mut parts: Vec<&[u8]> = Vec::new();
        let mut x = dir;
        while x != ROOT_DIR {
            let r = &self.dirs[x as usize];
            parts.push(&r.name);
            x = r.parent;
        }
        let mut p = self.root_path.clone();
        for name in parts.iter().rev() {
            if p.last() != Some(&b'/') {
                p.push(b'/');
            }
            p.extend_from_slice(name);
        }
        p
    }

    /// Whether `d` is `a` or below it.
    pub(crate) fn within(&self, mut d: DirId, a: DirId) -> bool {
        let ad = self.dirs[a as usize].depth;
        while self.dirs[d as usize].depth > ad {
            d = self.dirs[d as usize].parent;
        }
        d == a
    }

    // ── Counters ────────────────────────────────────────────────────────────

    /// Add `delta` to `pending_desc` of `d` and every ancestor.
    pub(crate) fn add_pending(&mut self, d: DirId, delta: i64) {
        let mut x = d;
        while x != NONE {
            let r = &mut self.dirs[x as usize];
            r.pending_desc = (i64::from(r.pending_desc) + delta) as u32;
            x = r.parent;
        }
    }

    /// Add `delta` to `active_desc` of `d` and every ancestor.
    pub(crate) fn add_active(&mut self, d: DirId, delta: i32) {
        let mut x = d;
        while x != NONE {
            let r = &mut self.dirs[x as usize];
            r.active_desc = (i32::from(r.active_desc) + delta) as u16;
            x = r.parent;
        }
    }
}

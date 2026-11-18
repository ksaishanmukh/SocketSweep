//! The scanned tree, built straight from the daemon's event stream.
//!
//! # Shape
//!
//! A flat `Vec<Node>` with child links held as indices: nodes never
//! move, a [`NodeId`] is a position, and walking to a parent is an index. Files
//! are nodes too. A second dense vector maps each wire [`DirId`] to its node,
//! so an event costs one index, not a path lookup.
//!
//! # Sizes
//!
//! Every node has its **own** size (its `lstat`, from its parent's listing or
//! a later `Stats`), and a directory also has **sub** totals: everything
//! strictly below it, subdirectories' own sizes included, which is exactly the
//! wire [`Totals`] convention. What the GUI sees is `own + sub`, so a folder's
//! rows always add up to the folder.
//!
//! Sums climb as `Listing` and `Stats` arrive, each delta applied to every
//! ancestor (`Partial`). `SubtreeDone` replaces a directory's sub totals with
//! the daemon's and applies the difference up the chain (`Final`): the
//! daemon's figure wins.
//!
//! Each walked directory also keeps the bytes below it per content category,
//! which the daemon does not send, so they are summed here from the same
//! listings and stats and taken out again on deletes; a folder's category is
//! whichever holds the most.
//!
//! # What else rows carry
//!
//! Change times are each node's newest `mtime`, bucketed by age from when the
//! scan started. A folder directly under `Android/data`, `Android/media` or
//! `Android/obb` named like a package is an app folder, tagged from the
//! [`AppCatalog`] the app helper filled in. Folders that reported `DirFailed`
//! are remembered for the overview.
//!
//! # Validation
//!
//! The daemon guarantees that a directory is introduced before anything refers
//! to it, that each directory's events come in a fixed order (listing parts,
//! `Stats`, each child's `SubtreeDone`, its own `SubtreeDone`), and that each
//! directory is finished once or pruned. These are checked as events arrive. A
//! violation is a [`TreeError::Protocol`], and the caller fails the scan: a
//! tree built from a stream it does not understand would show wrong numbers as
//! if they were right.
//!
//! # Deletes
//!
//! A subtree can disappear two ways: a `Pruned` event in the scan stream, or
//! [`Tree::remove`] once the phone confirms a delete. Whichever comes first
//! removes it; the other finds it gone and does nothing. The daemon's control
//! frames overtake its bulk queue, so events about directories inside a removed
//! subtree can still arrive afterwards; those directories are remembered as
//! gone and their events are dropped rather than treated as violations.

mod apps;
mod breakdowns;
mod category;
mod error;
mod identity;
mod ingest;
mod nodes;
mod order;
mod paths;
mod queries;
mod rows;
mod search;
mod sizes;

use std::time::{SystemTime, UNIX_EPOCH};

use socketsweep_gui_contract::{Measure, NodeId, SessionId, Size, SizeMetric};
use socketsweep_wire::{Kind, Outcome, Totals, ROOT_DIR};

#[cfg(doc)]
use socketsweep_wire::DirId;

use crate::apps::AppCatalog;
use nodes::{DirRec, DirState, Node, NONE};

pub use category::category_of;
pub use error::TreeError;
pub use order::{bucket_of, ViewQuery};
pub use search::SearchQuery;

/// The scan root's node.
pub const ROOT: NodeId = 0;

/// Facts from the stream the tree does not keep in nodes, returned by
/// [`Tree::ingest`] for the caller to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingested {
    /// The event only changed the tree.
    Tree,
    SkeletonReady {
        listed: u32,
        frontier: u32,
        elapsed_ms: u32,
    },
    /// `node` is `None` when the directory has since been removed.
    FocusState {
        seq: u32,
        node: Option<NodeId>,
        settled: bool,
    },
    Progress {
        listed: u32,
        known: u32,
    },
    Finished {
        outcome: Outcome,
        totals: Totals,
        elapsed_ms: u32,
    },
}

/// One scan's tree.
pub struct Tree {
    session: SessionId,
    nodes: Vec<Node>,
    /// Indexed by [`DirId`]; `None` for an id not introduced yet.
    dirs: Vec<Option<DirRec>>,
    /// The root the daemon resolved: what wire paths are built from.
    root_path: Vec<u8>,
    /// The root as the user knows it (`/sdcard`): what display paths are
    /// built from.
    root_label: String,
    finished: Option<Outcome>,
    /// Folders that reported `DirFailed`, in the order they did.
    unreadable: Vec<NodeId>,
    /// Unix seconds when the scan started: what "changed 3 months ago" counts
    /// back from.
    scan_time: i64,
    /// What the phone has said about installed apps.
    apps: AppCatalog,
}

impl Tree {
    /// A tree for a scan whose `Started` named `resolved_root`. `label` is the
    /// root as the user asked for it.
    pub fn new(session: SessionId, resolved_root: &[u8], label: &str) -> Self {
        let root = Node {
            name: basename(label.as_bytes()).to_vec().into_boxed_slice(),
            parent: NONE,
            first_child: NONE,
            last_child: NONE,
            next_sibling: NONE,
            kind: Kind::Dir,
            dir: ROOT_DIR,
            own: Size::default(),
            mtime: 0,
            sub: Totals::default(),
            measure: Measure::Unknown,
            stat_known: true,
            stat_failed: false,
            removed: false,
        };
        Tree {
            session,
            nodes: vec![root],
            dirs: vec![Some(DirRec::new(ROOT, DirState::Introduced))],
            root_path: resolved_root.to_vec(),
            root_label: label.to_string(),
            finished: None,
            unreadable: Vec::new(),
            scan_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            apps: AppCatalog::default(),
        }
    }

    /// Unix seconds when the scan started.
    pub fn scan_time(&self) -> i64 {
        self.scan_time
    }

    /// Count change times back from `unix_seconds` instead of the moment the
    /// tree was made.
    pub fn set_scan_time(&mut self, unix_seconds: i64) {
        self.scan_time = unix_seconds;
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The scan has ended, and how.
    pub fn finished(&self) -> Option<Outcome> {
        self.finished
    }

    /// The root as the user knows it.
    pub fn root_label(&self) -> &str {
        &self.root_label
    }
}

/// The figure a [`SizeMetric`] selects.
pub fn metric(size: Size, by: SizeMetric) -> u64 {
    match by {
        SizeMetric::Disk => size.disk,
        SizeMetric::Apparent => size.apparent,
    }
}

/// The last component of `path`, ignoring trailing slashes.
pub(crate) fn basename(path: &[u8]) -> &[u8] {
    let trimmed = match path.iter().rposition(|b| *b != b'/') {
        Some(end) => &path[..=end],
        None => return path,
    };
    match trimmed.iter().rposition(|b| *b == b'/') {
        Some(slash) => &trimmed[slash + 1..],
        None => trimmed,
    }
}

fn protocol(m: impl Into<String>) -> TreeError {
    TreeError::Protocol(m.into())
}

#[cfg(test)]
mod tests;

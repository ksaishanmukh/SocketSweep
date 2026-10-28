//! The scanned tree, built straight from the daemon's event stream.
//!
//! # Shape
//!
//! A flat `Vec<Node>` with child links held as indices, as in v3: nodes never
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
//! daemon's figure wins. See `docs/architecture.md`, section 4.
//!
//! # Validation
//!
//! The ordering guarantees G1–G3 are checked as events arrive. A violation is
//! a [`TreeError::Protocol`], and the caller fails the scan: a tree built from
//! a stream it does not understand would show wrong numbers as if they were
//! right.
//!
//! # Deletes
//!
//! A subtree can disappear two ways: a `Pruned` event in the scan stream, or
//! [`Tree::remove`] once the phone confirms a delete. Whichever comes first
//! removes it; the other finds it gone and does nothing. The daemon's control
//! frames overtake its bulk queue, so events about directories inside a removed
//! subtree can still arrive afterwards; those directories are remembered as
//! gone and their events are dropped rather than treated as violations.
//!
//! # Layout
//!
//! `nodes` holds the node and directory records, `ingest` applies the event
//! stream, `sizes` keeps sums in step up the ancestor chain, `identity` maps
//! nodes to references and paths, and `queries` and `breakdowns` answer the
//! GUI's questions.

mod breakdowns;
mod identity;
mod ingest;
mod nodes;
mod queries;
mod sizes;

use socketsweep_gui_contract::{Measure, NodeId, SessionId, Size, SizeMetric};
use socketsweep_wire::{Kind, Outcome, Totals, ROOT_DIR};

#[cfg(doc)]
use socketsweep_wire::DirId;

use nodes::{DirRec, DirState, Node, NONE};

/// The scan root's node.
pub const ROOT: NodeId = 0;

/// Why a tree operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// A node from another session's tree.
    Stale,
    /// The node existed but has been deleted.
    Removed,
    /// No such node in this tree.
    UnknownNode(NodeId),
    /// The scan root cannot be deleted.
    RootNotDeletable,
    /// The event stream broke an ordering guarantee or named something that
    /// does not exist. The scan must be failed.
    Protocol(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::Stale => write!(f, "These results are out of date. Rescan."),
            TreeError::Removed => write!(f, "That item has already been deleted."),
            TreeError::UnknownNode(id) => write!(f, "There is no item {id} in this scan."),
            TreeError::RootNotDeletable => write!(f, "The scan root cannot be deleted."),
            TreeError::Protocol(m) => write!(f, "The phone sent an invalid scan: {m}"),
        }
    }
}

impl std::error::Error for TreeError {}

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

/// One scan's tree. See the [module documentation](self).
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
            sub: Totals::default(),
            measure: Measure::Unknown,
            stat_known: true,
            stat_failed: false,
            removed: false,
        };
        Tree {
            session,
            nodes: vec![root],
            dirs: vec![Some(DirRec {
                node: ROOT,
                state: DirState::Introduced,
                next_part: 0,
                entries: Vec::new(),
                open: 0,
            })],
            root_path: resolved_root.to_vec(),
            root_label: label.to_string(),
            finished: None,
        }
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

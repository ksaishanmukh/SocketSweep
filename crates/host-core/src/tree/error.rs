//! Why a tree operation failed.

use socketsweep_gui_contract::NodeId;

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

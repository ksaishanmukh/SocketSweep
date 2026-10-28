//! Nodes and sizes: what every query and event refers to.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

/// Identifies one tree: a scan of one device. Assigned by the host, unique
/// for the life of the process, never 0.
pub type SessionId = u32;

/// A node's index within its session's tree. The scan root is 0.
pub type NodeId = u32;

/// A file or folder of one session's tree. See [nodes, not
/// paths](crate#nodes-not-paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeRef {
    pub session: SessionId,
    pub id: NodeId,
}

/// A size in bytes, both ways of counting it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Size {
    /// The sum of file lengths, what a file manager shows.
    #[specta(type = Number)]
    pub apparent: u64,
    /// Space allocated on the phone's storage, what deleting frees.
    #[specta(type = Number)]
    pub disk: u64,
}

/// How far a node's size is known. See [sizes](crate#sizes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Measure {
    /// Not listed yet (a folder) or not stat'ed yet (a file). The GUI shows
    /// "measuring…", not 0.
    Unknown,
    /// A lower bound that climbs as the scan goes on.
    Partial,
    /// Everything below is measured.
    Final,
}

/// Which [`Size`] a query sorts and sizes by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SizeMetric {
    #[default]
    Disk,
    Apparent,
}

/// What a node is. Symbolic links are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    /// Sockets, pipes, devices.
    Other,
}

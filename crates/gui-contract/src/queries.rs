//! What the tree queries answer.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{EntryKind, Measure, NodeRef, Size};

#[cfg(doc)]
use crate::SizeMetric;

/// One file or folder in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub node: NodeRef,
    /// Lossy UTF-8, for display only. Actions travel by `node`.
    pub name: String,
    pub kind: EntryKind,
    pub size: Size,
    pub measure: Measure,
    /// Files in the subtree; 1 for a file.
    pub files: u32,
    /// Some folder in the subtree could not be read, so the size is low.
    pub unreadable: bool,
    /// The containing folder's path, on results that span the tree (search
    /// hits, largest files), where the name alone does not say where the thing
    /// lives. `null` inside a single folder's listing.
    pub parent: Option<String>,
    /// That folder, for "go to".
    pub parent_node: Option<NodeRef>,
}

/// One folder's contents, largest first by the requested [`SizeMetric`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub node: NodeRef,
    /// Display path.
    pub path: String,
    pub size: Size,
    pub measure: Measure,
    pub files: u32,
    pub dirs: u32,
    pub unreadable: bool,
    pub rows: Vec<Row>,
    /// Rows beyond the requested limit, so the GUI can say "and 412 more".
    pub hidden: u32,
}

/// One step of the path from the root to a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub node: NodeRef,
    pub name: String,
}

/// A node plus a bounded slice of its descendants, for the nested treemap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TreemapNode {
    pub node: NodeRef,
    pub name: String,
    pub kind: EntryKind,
    pub size: Size,
    pub measure: Measure,
    /// Largest first; empty below the requested depth.
    pub children: Vec<TreemapNode>,
}

/// Storage owned by one app, summed across its data, obb and media folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    /// Android package name, e.g. `com.example.game`. Not a display label:
    /// adb's shell has no access to app labels.
    pub package: String,
    pub size: Size,
    pub files: u32,
    /// The largest folder the package owns, for navigating to it.
    pub node: NodeRef,
}

/// Total size of one broad file category (images, video, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TypeGroup {
    pub label: String,
    pub size: Size,
    pub files: u32,
}

//! What the tree queries answer.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

use crate::{
    AppTag, Category, EntryKind, Measure, NodeRef, SearchFilter, Size, SizeMetric, ViewFilter,
    ViewSort,
};

/// Which folder `focus` and `getView` mean, and how to show it. Every field
/// may be left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewRequest {
    /// The folder; the root when `null`.
    #[serde(default)]
    pub node: Option<NodeRef>,
    /// Rows to return at most (default 500).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Which size to measure and sort by (default disk).
    #[serde(default)]
    pub by: Option<SizeMetric>,
    /// Row order (default largest first).
    #[serde(default)]
    pub sort: Option<ViewSort>,
    /// Which rows to keep (default all).
    #[serde(default)]
    pub filter: Option<ViewFilter>,
}

/// What `search` looks for and how it shows the hits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// Matched against names, case-insensitively.
    pub query: String,
    /// Results to return at most (default 200).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Which size to measure and sort by (default disk).
    #[serde(default)]
    pub by: Option<SizeMetric>,
    /// Which hits to keep (default all).
    #[serde(default)]
    pub filter: Option<SearchFilter>,
    /// Result order (default largest first).
    #[serde(default)]
    pub sort: Option<ViewSort>,
    /// Only look inside this folder; everywhere when `null`.
    #[serde(default)]
    pub within: Option<NodeRef>,
}

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
    /// The newest change, in Unix seconds: a file's modification time, a
    /// folder's newest anywhere inside (it climbs as the scan goes on, and
    /// deletes never lower it). `null` while unknown.
    #[specta(type = Option<Number>)]
    pub changed: Option<i64>,
    pub category: Category,
    /// Set on a folder that belongs to an app.
    pub app: Option<AppTag>,
    /// The containing folder's path, on results that span the tree (search
    /// hits, largest files), where the name alone does not say where the thing
    /// lives. `null` inside a single folder's listing.
    pub parent: Option<String>,
    /// That folder, for "go to".
    pub parent_node: Option<NodeRef>,
    /// Staged for deletion, itself or through a folder above it.
    pub staged: bool,
    /// What `size` would be with everything staged removed: less than `size`
    /// when staged nodes lie inside, zero when the row is staged itself. Same
    /// `measure` as `size`.
    pub projected: Size,
}

/// One folder's contents, in the requested [`ViewSort`] (largest first by
/// the requested [`SizeMetric`] unless asked otherwise), narrowed by the
/// requested [`ViewFilter`].
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
    /// As on [`Row`].
    #[specta(type = Option<Number>)]
    pub changed: Option<i64>,
    pub category: Category,
    pub app: Option<AppTag>,
    /// As on [`Row`].
    pub staged: bool,
    pub projected: Size,
    pub rows: Vec<Row>,
    /// Matching rows beyond the requested limit, so the GUI can say "and
    /// 412 more".
    pub hidden: u32,
    /// Children that pass the filter, shown or not.
    pub total_matching: u32,
    /// Children before filtering, so the GUI can say "7 of 20 shown".
    pub total_children: u32,
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
    /// As on [`Row`].
    #[specta(type = Option<Number>)]
    pub changed: Option<i64>,
    pub category: Category,
    pub app: Option<AppTag>,
    /// As on [`Row`].
    pub staged: bool,
    pub projected: Size,
    /// Largest first; empty below the requested depth.
    pub children: Vec<TreemapNode>,
}

/// Storage owned by one app, summed across its data, obb and media folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    /// Android package name, e.g. `com.example.game`.
    pub package: String,
    /// The name the launcher shows, once the phone has reported it.
    pub label: Option<String>,
    /// As on [`AppTag`].
    pub installed: bool,
    pub has_icon: bool,
    pub size: Size,
    pub files: u32,
    /// The largest folder the package owns, for navigating to it.
    pub node: NodeRef,
}

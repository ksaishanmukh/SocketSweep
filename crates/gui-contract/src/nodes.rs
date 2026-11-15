//! Nodes and sizes: what every query and event refers to.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

/// Identifies one tree: a scan of one device. Assigned by the host, unique
/// for the life of the process, never 0.
pub type SessionId = u32;

/// A node's index within its session's tree. The scan root is 0.
pub type NodeId = u32;

/// A file or folder of one session's tree. The GUI refers to files and
/// folders only by node, never by path.
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

/// How far a node's size is known.
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

/// A broad kind of content, for colouring and filtering. A file's comes from
/// its extension; a folder's is the one holding the most bytes inside it, by
/// the [`SizeMetric`] of the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Audio,
    Video,
    Photo,
    /// Archives and app packages (`.zip`, `.apk`, `.obb`, …).
    Archive,
    Doc,
    /// Everything else: app data, caches, unknown extensions.
    Other,
}

impl Category {
    /// Every category, in the order [`Category::index`] numbers them.
    pub const ALL: [Category; 6] = [
        Category::Audio,
        Category::Video,
        Category::Photo,
        Category::Archive,
        Category::Doc,
        Category::Other,
    ];

    /// Position in [`Category::ALL`], for arrays with one slot per category.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The app a folder belongs to: a folder directly under `Android/data`,
/// `Android/media` or `Android/obb` named like a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppTag {
    /// Android package name, e.g. `com.whatsapp`.
    pub package: String,
    /// The name the launcher shows, once the phone has reported it.
    pub label: Option<String>,
    /// False only once the phone has reported its apps and this package is
    /// not among them: the folder is left over from an uninstalled app.
    pub installed: bool,
    /// `appIcon` has an icon for this package.
    pub has_icon: bool,
}

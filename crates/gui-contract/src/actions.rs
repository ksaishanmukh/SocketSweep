//! What `deleteMany` and `copyMany` answer.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

use crate::{CommandError, NodeRef, Size, Totals, View};

/// The answer to `deleteMany`, once the phone has answered for every node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeletedMany {
    /// Nodes deleted, of those asked for (a node inside another one asked
    /// for counts with it, not on its own).
    pub deleted: u32,
    /// Files and folders removed, everything inside included.
    #[specta(type = Number)]
    pub items: u64,
    /// What the phone freed.
    pub size: Size,
    /// Nodes the phone could not delete; the rest are gone.
    pub failures: Vec<DeleteFailure>,
    /// The whole tree's totals afterwards.
    pub totals: Totals,
    /// The focused folder, refreshed; its nearest surviving ancestor if it
    /// was inside what was deleted.
    pub view: View,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFailure {
    pub node: NodeRef,
    /// Display path.
    pub path: String,
    pub error: CommandError,
}

/// The answer to `copyMany`, once every file is copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    /// Where the copy is, on this computer: the copied file or folder for one
    /// node, the destination folder for several.
    pub destination: String,
    pub files: u32,
    #[specta(type = Number)]
    pub bytes: u64,
    /// Symbolic links and special files, which are not copied.
    pub skipped: u32,
    /// Files whose Android name is not valid here, copied under a changed
    /// name.
    pub renamed: u32,
}

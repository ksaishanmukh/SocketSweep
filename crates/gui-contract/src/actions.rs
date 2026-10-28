//! What `delete` and `copyToDesktop` answer.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

use crate::{Totals, View};

// ── Actions ─────────────────────────────────────────────────────────────────

/// The answer to `delete`, once the phone confirms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    /// Files and folders removed.
    #[specta(type = Number)]
    pub items: u64,
    /// The whole tree's totals afterwards.
    pub totals: Totals,
    /// The focused folder, refreshed; its nearest surviving ancestor if it
    /// was inside what was deleted.
    pub view: View,
}

/// The answer to `copyToDesktop`, once every file is copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    /// Where the copy is, on this computer.
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

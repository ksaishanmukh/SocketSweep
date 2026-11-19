//! What `overview` answers: the summary screen of a scan.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

use crate::{AppUsage, Category, NodeRef, Row, ScanRecord, Size, StagedTotals, Totals};

/// Whether the phone has said which apps are installed. App names, icons and
/// leftovers depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AppsStatus {
    /// Still asking; package names only for now.
    Pending,
    /// The phone answered.
    Ready,
    /// The phone could not say (the helper failed or is not bundled). Package
    /// names only, and no leftovers, for this scan.
    Unavailable,
}

/// The whole scan at a glance. Every figure is as far as the scan has got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub totals: Totals,
    /// What is staged for deletion; `items` is 0 when nothing is.
    pub staged: StagedTotals,
    /// `totals.size` with everything staged removed.
    pub projected: Size,
    /// Bytes per category over the whole tree, largest first, empty ones left
    /// out.
    pub categories: Vec<CategoryUsage>,
    /// The ten largest files, with their folders.
    pub biggest_files: Vec<Row>,
    /// The eight apps using the most storage under `Android/data`,
    /// `Android/media` and `Android/obb`.
    pub apps: Vec<AppUsage>,
    pub apps_status: AppsStatus,
    /// Folders of apps that are no longer installed. Empty until
    /// `appsStatus` is `ready`.
    pub leftovers: Leftovers,
    /// Folders the phone would not let the scan read, so their sizes are
    /// missing.
    pub unreadable: Vec<UnreadableFolder>,
    /// Compared with the last finished scan of this phone and root.
    pub since_last_scan: Option<SinceLastScan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategoryUsage {
    pub category: Category,
    pub size: Size,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Leftovers {
    pub total: Size,
    /// Largest first.
    pub apps: Vec<Leftover>,
}

/// One uninstalled package's folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub package: String,
    pub size: Size,
    /// Each of its folders, largest first, for selecting or deleting them.
    pub folders: Vec<NodeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableFolder {
    pub node: NodeRef,
    /// The full display path, e.g. `/sdcard/Android/data/com.example`.
    pub path: String,
}

/// The difference from the previous scan: positive means more now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SinceLastScan {
    pub previous: ScanRecord,
    #[specta(type = Number)]
    pub apparent_delta: i64,
    #[specta(type = Number)]
    pub disk_delta: i64,
    #[specta(type = Number)]
    pub files_delta: i64,
}

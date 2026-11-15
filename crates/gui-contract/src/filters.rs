//! How a folder view or a search is sorted and narrowed.
//!
//! The host applies both over every candidate before the row limit, so a
//! filtered folder of 50,000 files still shows its true matches, and
//! `totalMatching` can say how many there are.
//!
//! Within one group of options (several categories, say) any option matches;
//! across groups all must match. An empty group does not narrow anything.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Category;

#[cfg(doc)]
use crate::SizeMetric;

/// What rows are ordered by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    /// Case-insensitively.
    Name,
    /// By the query's [`SizeMetric`].
    #[default]
    Size,
    /// Files inside a folder. Files count as fewer than any folder.
    Count,
    /// The newest change: a file's own, a folder's anywhere inside. Unknown
    /// counts as oldest.
    Changed,
}

/// A sort order. Equal rows fall back to their names, so they do not swap
/// places between refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewSort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for ViewSort {
    /// Largest first.
    fn default() -> Self {
        ViewSort {
            key: SortKey::Size,
            descending: true,
        }
    }
}

/// Folders or files. Symbolic links and special files count as files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum KindFilter {
    Folder,
    File,
}

/// How long ago something last changed, counted back from when the scan
/// started rather than from now, so results do not drift while the app
/// stays open. Something whose change time is unknown is in no bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChangedBucket {
    /// Within 30 days (or dated after the scan started).
    Last30d,
    /// More than 30 days, up to 182 days.
    Months1To6,
    /// More than 182 days, up to 365 days.
    Months6To12,
    /// More than 365 days.
    OverYear,
}

/// What a folder view keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewFilter {
    /// Names containing this, case-insensitively. `null` or empty keeps all.
    pub query: Option<String>,
    pub categories: Vec<Category>,
    pub kinds: Vec<KindFilter>,
    pub changed: Vec<ChangedBucket>,
}

/// What a search keeps besides its query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilter {
    pub categories: Vec<Category>,
    pub kinds: Vec<KindFilter>,
    pub changed: Vec<ChangedBucket>,
}

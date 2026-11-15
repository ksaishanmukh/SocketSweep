//! Sorting and filtering rows, the same way for folder views and search.

use std::cmp::Ordering;

use socketsweep_gui_contract::{
    Category, ChangedBucket, KindFilter, NodeId, SearchFilter, SizeMetric, SortKey, ViewFilter,
    ViewSort, DEFAULT_VIEW_LIMIT,
};

use super::{metric, Tree};

const DAY: i64 = 24 * 60 * 60;

/// How a folder view is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewQuery {
    pub limit: u32,
    pub by: SizeMetric,
    pub sort: ViewSort,
    pub filter: ViewFilter,
}

impl Default for ViewQuery {
    /// Largest first by disk usage, unfiltered, the default row cap.
    fn default() -> Self {
        ViewQuery {
            limit: DEFAULT_VIEW_LIMIT,
            by: SizeMetric::Disk,
            sort: ViewSort::default(),
            filter: ViewFilter::default(),
        }
    }
}

/// Which bucket something changed at `changed` falls in, counted back from
/// `scan_time` (both Unix seconds).
pub fn bucket_of(scan_time: i64, changed: i64) -> ChangedBucket {
    let age = scan_time.saturating_sub(changed);
    if age <= 30 * DAY {
        ChangedBucket::Last30d
    } else if age <= 182 * DAY {
        ChangedBucket::Months1To6
    } else if age <= 365 * DAY {
        ChangedBucket::Months6To12
    } else {
        ChangedBucket::OverYear
    }
}

/// A filter ready to test nodes with.
pub(super) struct Matcher<'a> {
    /// Lowercase; `None` keeps every name.
    needle: Option<String>,
    categories: &'a [Category],
    kinds: &'a [KindFilter],
    changed: &'a [ChangedBucket],
    by: SizeMetric,
}

impl<'a> Matcher<'a> {
    pub(super) fn view(filter: &'a ViewFilter, by: SizeMetric) -> Self {
        Matcher {
            needle: filter
                .query
                .as_deref()
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(str::to_lowercase),
            categories: &filter.categories,
            kinds: &filter.kinds,
            changed: &filter.changed,
            by,
        }
    }

    pub(super) fn search(query: &str, filter: &'a SearchFilter, by: SizeMetric) -> Self {
        let query = query.trim();
        Matcher {
            needle: (!query.is_empty()).then(|| query.to_lowercase()),
            categories: &filter.categories,
            kinds: &filter.kinds,
            changed: &filter.changed,
            by,
        }
    }

    /// Keeps everything.
    pub(super) fn is_empty(&self) -> bool {
        self.needle.is_none()
            && self.categories.is_empty()
            && self.kinds.is_empty()
            && self.changed.is_empty()
    }
}

impl Tree {
    /// Whether `id` passes `m`. Within a group any option matches; every
    /// group must match.
    pub(super) fn matches(&self, id: NodeId, m: &Matcher<'_>) -> bool {
        let n = &self.nodes[id as usize];
        if let Some(needle) = &m.needle {
            if !String::from_utf8_lossy(&n.name)
                .to_lowercase()
                .contains(needle.as_str())
            {
                return false;
            }
        }
        if !m.kinds.is_empty() {
            let kind = if n.is_dir() {
                KindFilter::Folder
            } else {
                KindFilter::File
            };
            if !m.kinds.contains(&kind) {
                return false;
            }
        }
        if !m.categories.is_empty() && !m.categories.contains(&self.category(id, m.by)) {
            return false;
        }
        if !m.changed.is_empty() {
            let Some(changed) = self.newest_mtime(id) else {
                return false;
            };
            if !m.changed.contains(&bucket_of(self.scan_time, changed)) {
                return false;
            }
        }
        true
    }

    /// Put `ids` in `sort` order, sizes by `by`. Ties go by name, then by
    /// node, ascending whatever the direction, so equal rows keep their
    /// places between refreshes.
    pub(super) fn order(&self, ids: &mut [NodeId], sort: ViewSort, by: SizeMetric) {
        if sort.key == SortKey::Name {
            ids.sort_by_cached_key(|id| {
                let name = &self.nodes[*id as usize].name;
                (String::from_utf8_lossy(name).to_lowercase(), *id)
            });
            if sort.descending {
                ids.reverse();
            }
            return;
        }
        ids.sort_unstable_by(|a, b| {
            let primary = self
                .sort_value(*a, sort.key, by)
                .cmp(&self.sort_value(*b, sort.key, by));
            let primary = if sort.descending {
                primary.reverse()
            } else {
                primary
            };
            primary.then_with(|| self.tie_break(*a, *b))
        });
    }

    fn sort_value(&self, id: NodeId, key: SortKey, by: SizeMetric) -> i128 {
        let n = &self.nodes[id as usize];
        match key {
            SortKey::Size | SortKey::Name => i128::from(metric(n.size(), by)),
            SortKey::Count if n.is_dir() => i128::from(n.sub.files),
            SortKey::Count => -1,
            SortKey::Changed => i128::from(n.newest_mtime()),
        }
    }

    fn tie_break(&self, a: NodeId, b: NodeId) -> Ordering {
        let (x, y) = (&self.nodes[a as usize], &self.nodes[b as usize]);
        x.name.cmp(&y.name).then_with(|| a.cmp(&b))
    }
}

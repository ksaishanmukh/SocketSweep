//! The summary screen: assembled from the tree, the app catalog and the
//! previous scan.

use socketsweep_gui_contract::{
    Overview, SinceLastScan, SizeMetric, OVERVIEW_APPS, OVERVIEW_BIGGEST_FILES,
};

use super::errors::CmdResult;
use super::state::Inner;
use crate::lock;

pub(super) fn overview(inner: &Inner, by: SizeMetric) -> CmdResult<Overview> {
    let scan = inner.scan()?;
    let tree = lock(&scan.tree);
    let totals = tree.totals();
    let since_last_scan = scan.previous.map(|previous| SinceLastScan {
        previous,
        apparent_delta: delta(totals.size.apparent, previous.size.apparent),
        disk_delta: delta(totals.size.disk, previous.size.disk),
        files_delta: i64::from(totals.files) - i64::from(previous.files),
    });
    Ok(Overview {
        totals,
        staged: tree.staged_totals(),
        projected: tree.projected_total(),
        categories: tree.categories(by),
        biggest_files: tree.largest_files(OVERVIEW_BIGGEST_FILES, by),
        apps: tree.app_breakdown(OVERVIEW_APPS, by),
        apps_status: tree.apps().status(),
        leftovers: tree.leftovers(by),
        unreadable: tree.unreadable_folders(),
        since_last_scan,
    })
}

/// `now - before`, saturating at the ends of `i64`.
fn delta(now: u64, before: u64) -> i64 {
    if now >= before {
        i64::try_from(now - before).unwrap_or(i64::MAX)
    } else {
        i64::try_from(before - now).map_or(i64::MIN, |d| -d)
    }
}

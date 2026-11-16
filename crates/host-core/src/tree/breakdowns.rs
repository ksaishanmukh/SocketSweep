//! Storage broken down by content category and by app package.

use std::collections::HashMap;

use socketsweep_gui_contract::{AppUsage, Category, CategoryUsage, NodeId, Size, SizeMetric};
use socketsweep_wire::ROOT_DIR;

use super::apps::add_size;
use super::{metric, Tree};

impl Tree {
    /// Bytes per category over the whole tree, largest first by `by`, empty
    /// ones left out. Folders' own sizes count as [`Category::Other`].
    pub fn categories(&self, by: SizeMetric) -> Vec<CategoryUsage> {
        let Some(Some(root)) = self.dirs.get(ROOT_DIR as usize) else {
            return Vec::new();
        };
        let mut out: Vec<CategoryUsage> = Category::ALL
            .iter()
            .map(|c| CategoryUsage {
                category: *c,
                size: root.cats[c.index()],
            })
            .filter(|u| metric(u.size, by) > 0)
            .collect();
        out.sort_by(|a, b| {
            metric(b.size, by)
                .cmp(&metric(a.size, by))
                .then_with(|| a.category.index().cmp(&b.category.index()))
        });
        out
    }

    /// Storage per app package, summed over its app folders, largest first.
    pub fn app_breakdown(&self, limit: u32, by: SizeMetric) -> Vec<AppUsage> {
        // package -> (size, files, largest folder, its size)
        let mut totals: HashMap<&str, (Size, u32, NodeId, u64)> = HashMap::new();
        for (package, id) in self.app_folders() {
            let n = &self.nodes[id as usize];
            let size = n.size();
            let slot = totals.entry(package).or_insert((Size::default(), 0, id, 0));
            slot.0 = add_size(slot.0, size);
            slot.1 += n.sub.files;
            if metric(size, by) > slot.3 {
                slot.2 = id;
                slot.3 = metric(size, by);
            }
        }
        let mut rows: Vec<AppUsage> = totals
            .into_iter()
            .filter(|(_, (size, ..))| metric(*size, by) > 0)
            .map(|(package, (size, files, node, _))| {
                let tag = self.apps.tag(package);
                AppUsage {
                    package: tag.package,
                    label: tag.label,
                    installed: tag.installed,
                    has_icon: tag.has_icon,
                    size,
                    files,
                    node: self.node_ref(node),
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            metric(b.size, by)
                .cmp(&metric(a.size, by))
                .then_with(|| a.package.cmp(&b.package))
        });
        rows.truncate(limit as usize);
        rows
    }
}

//! Storage broken down by file type and by app package.

use std::collections::HashMap;

use socketsweep_gui_contract::{AppUsage, NodeId, Size, SizeMetric, TypeGroup};

use super::{metric, Tree, NONE, ROOT};

impl Tree {
    /// Total size per broad file category, largest first.
    pub fn type_breakdown(&self, by: SizeMetric) -> Vec<TypeGroup> {
        let mut sizes = [Size::default(); CATEGORIES.len()];
        let mut counts = [0u32; CATEGORIES.len()];
        for n in self.nodes.iter().filter(|n| !n.is_dir() && !n.removed) {
            let c = category_of(&n.name);
            sizes[c].apparent += n.own.apparent;
            sizes[c].disk += n.own.disk;
            counts[c] += 1;
        }
        let mut groups: Vec<TypeGroup> = CATEGORIES
            .iter()
            .enumerate()
            .filter(|(i, _)| counts[*i] > 0)
            .map(|(i, label)| TypeGroup {
                label: (*label).to_string(),
                size: sizes[i],
                files: counts[i],
            })
            .collect();
        groups.sort_by(|a, b| {
            metric(b.size, by)
                .cmp(&metric(a.size, by))
                .then_with(|| a.label.cmp(&b.label))
        });
        groups
    }

    fn child_named(&self, parent: NodeId, name: &[u8]) -> Option<NodeId> {
        let mut cur = self.nodes.get(parent as usize)?.first_child;
        while cur != NONE {
            let n = &self.nodes[cur as usize];
            if !n.removed && &*n.name == name {
                return Some(cur);
            }
            cur = n.next_sibling;
        }
        None
    }

    /// Storage per app package, from `Android/{data,obb,media}/<package>`.
    /// Location identifies the owner where no extension could.
    pub fn app_breakdown(&self, limit: u32, by: SizeMetric) -> Vec<AppUsage> {
        const AREAS: [&[u8]; 3] = [b"data", b"obb", b"media"];
        let Some(android) = self.child_named(ROOT, b"Android") else {
            return Vec::new();
        };
        // package -> (size, files, largest folder, its size)
        let mut totals: HashMap<&[u8], (Size, u32, NodeId, u64)> = HashMap::new();
        for area in AREAS {
            let Some(area_id) = self.child_named(android, area) else {
                continue;
            };
            for pkg in self.children_of(area_id) {
                let n = &self.nodes[pkg as usize];
                if !n.is_dir() {
                    continue;
                }
                let size = n.size();
                let slot = totals
                    .entry(&n.name)
                    .or_insert((Size::default(), 0, pkg, 0));
                slot.0.apparent += size.apparent;
                slot.0.disk += size.disk;
                slot.1 += n.sub.files;
                if metric(size, by) > slot.3 {
                    slot.2 = pkg;
                    slot.3 = metric(size, by);
                }
            }
        }
        let mut rows: Vec<AppUsage> = totals
            .into_iter()
            .filter(|(_, (size, ..))| metric(*size, by) > 0)
            .map(|(name, (size, files, node, _))| AppUsage {
                package: String::from_utf8_lossy(name).into_owned(),
                size,
                files,
                node: self.node_ref(node),
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

/// Broad categories, deliberately coarse: "24 GB of video" is actionable.
const CATEGORIES: [&str; 7] = [
    "Photos",
    "Video",
    "Audio",
    "Apps",
    "Documents",
    "Archives",
    "Other",
];

const PHOTOS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "bmp", "tiff", "tif", "dng", "raw",
    "avif", "svg",
];
const VIDEO: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "3gp", "webm", "flv", "m4v", "wmv", "ts", "mpg", "mpeg",
];
const AUDIO: &[&str] = &[
    "mp3", "aac", "flac", "wav", "ogg", "m4a", "opus", "wma", "amr", "mid",
];
const APPS: &[&str] = &["apk", "obb", "xapk", "apks", "aab"];
const DOCS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "epub", "mobi", "csv", "rtf",
    "odt",
];
const ARCHIVES: &[&str] = &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img"];

fn category_of(name: &[u8]) -> usize {
    let other = CATEGORIES.len() - 1;
    let Some(dot) = name.iter().rposition(|b| *b == b'.') else {
        return other;
    };
    if dot == 0 || dot + 1 >= name.len() {
        return other;
    }
    let ext = String::from_utf8_lossy(&name[dot + 1..]).to_lowercase();
    [PHOTOS, VIDEO, AUDIO, APPS, DOCS, ARCHIVES]
        .iter()
        .position(|list| list.contains(&ext.as_str()))
        .unwrap_or(other)
}

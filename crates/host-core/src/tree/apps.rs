//! App folders: folders directly under `Android/data`, `Android/media` and
//! `Android/obb` named like a package. Their location identifies the owning
//! app where no extension could.

use std::collections::HashMap;

use socketsweep_gui_contract::{AppTag, AppsStatus, Leftover, Leftovers, NodeId, Size, SizeMetric};

use super::{metric, Tree, NONE, ROOT};
use crate::apps::{is_package_name, AppCatalog};

/// The folders under `Android/` that hold per-app folders.
const AREAS: [&[u8]; 3] = [b"data", b"media", b"obb"];

impl Tree {
    pub fn apps(&self) -> &AppCatalog {
        &self.apps
    }

    /// Replace what the tree knows about installed apps.
    pub fn set_apps(&mut self, catalog: AppCatalog) {
        self.apps = catalog;
    }

    /// Record icons for `packages`.
    pub fn add_icons<'a>(&mut self, packages: impl IntoIterator<Item = &'a String>) {
        self.apps.add_icons(packages);
    }

    /// The package `id` is the folder of, if it is an app folder.
    pub(super) fn package_of(&self, id: NodeId) -> Option<&str> {
        let n = self.nodes.get(id as usize)?;
        if !n.is_dir() || n.parent == NONE || !self.is_app_area(n.parent) {
            return None;
        }
        std::str::from_utf8(&n.name)
            .ok()
            .filter(|name| is_package_name(name))
    }

    fn is_app_area(&self, area: NodeId) -> bool {
        let a = &self.nodes[area as usize];
        if !AREAS.contains(&&*a.name) || a.parent == NONE {
            return false;
        }
        let android = &self.nodes[a.parent as usize];
        &*android.name == b"Android" && android.parent == ROOT
    }

    pub(super) fn app_tag(&self, id: NodeId) -> Option<AppTag> {
        self.package_of(id).map(|p| self.apps.tag(p))
    }

    /// Every live app folder, with its package.
    pub fn app_folders(&self) -> Vec<(&str, NodeId)> {
        let Some(android) = self.child_named(ROOT, b"Android") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for area in AREAS {
            let Some(area_id) = self.child_named(android, area) else {
                continue;
            };
            for id in self.children_of(area_id) {
                if let Some(package) = self.package_of(id) {
                    out.push((package, id));
                }
            }
        }
        out
    }

    /// Packages that own folders, largest first by `by`.
    pub fn packages_by_size(&self, by: SizeMetric) -> Vec<String> {
        let mut sizes: HashMap<&str, u64> = HashMap::new();
        for (package, id) in self.app_folders() {
            *sizes.entry(package).or_default() += metric(self.nodes[id as usize].size(), by);
        }
        let mut packages: Vec<(&str, u64)> = sizes.into_iter().collect();
        packages.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        packages.into_iter().map(|(p, _)| p.to_string()).collect()
    }

    /// Folders of packages the phone says are not installed, largest first.
    /// Empty until the phone has listed its apps.
    pub fn leftovers(&self, by: SizeMetric) -> Leftovers {
        if self.apps.status() != AppsStatus::Ready {
            return Leftovers::default();
        }
        let mut groups: HashMap<&str, Vec<NodeId>> = HashMap::new();
        for (package, id) in self.app_folders() {
            if !self.apps.installed(package) {
                groups.entry(package).or_default().push(id);
            }
        }
        let mut total = Size::default();
        let mut apps: Vec<Leftover> = groups
            .into_iter()
            .map(|(package, mut folders)| {
                self.order(&mut folders, Default::default(), by);
                let size = folders.iter().fold(Size::default(), |acc, f| {
                    add_size(acc, self.nodes[*f as usize].size())
                });
                total = add_size(total, size);
                Leftover {
                    package: package.to_string(),
                    size,
                    folders: folders.iter().map(|f| self.node_ref(*f)).collect(),
                }
            })
            .collect();
        apps.sort_by(|a, b| {
            metric(b.size, by)
                .cmp(&metric(a.size, by))
                .then_with(|| a.package.cmp(&b.package))
        });
        Leftovers { total, apps }
    }

    pub(super) fn child_named(&self, parent: NodeId, name: &[u8]) -> Option<NodeId> {
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
}

pub(super) fn add_size(a: Size, b: Size) -> Size {
    Size {
        apparent: a.apparent.saturating_add(b.apparent),
        disk: a.disk.saturating_add(b.disk),
    }
}

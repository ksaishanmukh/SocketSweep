//! Installed apps: names and icons from the on-phone helper.
//!
//! The adb shell cannot read app labels or icons, so a small Java program
//! (`android-helper/`, bundled as a `.dex`) runs on the phone through
//! `app_process` and prints them, one JSON line per app (parsed by
//! [`socketsweep_wire::apps::parse_apps`]). [`helper`] pushes and runs it;
//! [`AppCatalog`] is what the tree knows from it.
//!
//! Everything here is best effort. Without the helper the GUI still has every
//! package name, from the folders under `Android/data`, `Android/media` and
//! `Android/obb`; it only lacks labels, icons and leftover detection.

pub mod helper;

use std::collections::{HashMap, HashSet};

use socketsweep_gui_contract::{AppTag, AppsStatus};

/// Where the helper is pushed.
pub const DEVICE_HELPER: &str = "/data/local/tmp/socketsweep-helper.dex";

/// What the phone has said about installed apps.
#[derive(Debug, Clone)]
pub struct AppCatalog {
    status: AppsStatus,
    /// Installed package → launcher label.
    labels: HashMap<String, String>,
    /// Packages whose icon the host holds.
    icons: HashSet<String>,
}

impl Default for AppCatalog {
    /// Nothing known yet.
    fn default() -> Self {
        AppCatalog {
            status: AppsStatus::Pending,
            labels: HashMap::new(),
            icons: HashSet::new(),
        }
    }
}

impl AppCatalog {
    /// The phone listed these installed packages, with their labels.
    pub fn ready(labels: HashMap<String, String>) -> Self {
        AppCatalog {
            status: AppsStatus::Ready,
            labels,
            icons: HashSet::new(),
        }
    }

    /// The phone could not say.
    pub fn unavailable() -> Self {
        AppCatalog {
            status: AppsStatus::Unavailable,
            ..AppCatalog::default()
        }
    }

    pub fn status(&self) -> AppsStatus {
        self.status
    }

    /// False only when the phone has listed its apps and `package` is not
    /// among them.
    pub fn installed(&self, package: &str) -> bool {
        self.status != AppsStatus::Ready || self.labels.contains_key(package)
    }

    pub fn label(&self, package: &str) -> Option<&str> {
        self.labels.get(package).map(String::as_str)
    }

    pub fn has_icon(&self, package: &str) -> bool {
        self.icons.contains(package)
    }

    pub fn icon_count(&self) -> u32 {
        u32::try_from(self.icons.len()).unwrap_or(u32::MAX)
    }

    /// Record that the host holds an icon for each of `packages`.
    pub fn add_icons<'a>(&mut self, packages: impl IntoIterator<Item = &'a String>) {
        self.icons.extend(packages.into_iter().cloned());
    }

    /// What the GUI is told about the folder of `package`.
    pub fn tag(&self, package: &str) -> AppTag {
        AppTag {
            package: package.to_string(),
            label: self.label(package).map(str::to_string),
            installed: self.installed(package),
            has_icon: self.has_icon(package),
        }
    }
}

/// Whether a folder name looks like an Android package: at least two
/// dot-separated parts of ASCII letters, digits and `_`, each starting with a
/// letter. Rules out `.nomedia`, `cache` and the like, and keeps anything that
/// reaches a shell command to characters that need no quoting.
pub fn is_package_name(name: &str) -> bool {
    let mut parts = 0;
    for part in name.split('.') {
        let mut chars = part.chars();
        let starts_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
        if !starts_ok || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        parts += 1;
    }
    parts >= 2
}

#[cfg(test)]
pub(crate) mod tests;

//! What the host knows about installed apps from the phone's helper.

use std::collections::{HashMap, HashSet};

use socketsweep_gui_contract::{AppTag, AppsStatus};

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

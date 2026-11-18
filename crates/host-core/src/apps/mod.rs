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

mod catalog;
pub mod helper;

pub use catalog::AppCatalog;

/// Where the helper is pushed.
pub const DEVICE_HELPER: &str = "/data/local/tmp/socketsweep-helper.dex";

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

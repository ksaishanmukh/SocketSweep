//! Remembers the last finished scan of each device and root.
//!
//! Storage is a filing cabinet people return to, so "what changed since last
//! time" is usually more useful than the absolute total. Keyed by serial, since
//! a desktop may see several phones, and then by root, since the internal
//! storage and an SD card of the same phone are different totals.
//!
//! # File format
//!
//! `scan-history.json` in the app data directory, a map of serial to a map of
//! root to [`ScanRecord`]:
//!
//! ```json
//! { "R58M12345": { "/sdcard": { "size": 1, "files": 2, "dirs": 3, "at": 1700000000 } } }
//! ```
//!
//! Earlier versions stored one record per serial, `{ "<serial>": ScanRecord }`.
//! Those versions only ever scanned the default root, so such an entry is read
//! as that device's [`DEFAULT_ROOT`] record and rewritten in the new shape the
//! next time that file is saved.
//!
//! The file is replaced atomically — written beside the original, then renamed
//! over it — so a crash mid-write leaves the previous history rather than a
//! truncated one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::arena::Stats;
use crate::session::DEFAULT_ROOT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub size: u64,
    pub files: u32,
    pub dirs: u32,
    /// Unix seconds. The frontend formats it; the host does not guess a locale.
    pub at: u64,
}

impl ScanRecord {
    fn from_stats(stats: &Stats) -> Self {
        ScanRecord {
            size: stats.size,
            files: stats.files,
            dirs: stats.dirs,
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

/// One device's entry, in either the current or the earlier shape.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    ByRoot(HashMap<String, ScanRecord>),
    Legacy(ScanRecord),
}

/// Serial → root → last finished scan.
type History = HashMap<String, HashMap<String, ScanRecord>>;

fn path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("scan-history.json"))
}

/// The history in `file`. Missing, unreadable or malformed reads as empty.
fn load(file: &Path) -> History {
    let Some(raw) = std::fs::read_to_string(file).ok() else {
        return History::new();
    };
    let Ok(stored) = serde_json::from_str::<HashMap<String, Stored>>(&raw) else {
        return History::new();
    };
    stored
        .into_iter()
        .map(|(serial, entry)| {
            let roots = match entry {
                Stored::ByRoot(roots) => roots,
                Stored::Legacy(record) => HashMap::from([(DEFAULT_ROOT.to_string(), record)]),
            };
            (serial, roots)
        })
        .collect()
}

/// Replace `file` with `history`, atomically.
fn save(file: &Path, history: &History) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(history).map_err(std::io::Error::other)?;

    let mut temp = file.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);

    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

fn previous_in(file: &Path, serial: &str, root: &str) -> Option<ScanRecord> {
    load(file).get(serial)?.get(root).copied()
}

fn record_in(file: &Path, serial: &str, root: &str, stats: &Stats) -> std::io::Result<()> {
    // Only a walk the daemon reported finished is a fair baseline; a partial
    // total would make the next scan look like growth.
    if stats.scanning {
        return Ok(());
    }
    let mut history = load(file);
    history
        .entry(serial.to_string())
        .or_default()
        .insert(root.to_string(), ScanRecord::from_stats(stats));
    save(file, &history)
}

/// The stored record for a device and root, if there is one.
///
/// Every failure here is silent and returns `None`: a missing or unreadable
/// history file is a nicety not working, and is not worth interrupting a scan
/// the user actually asked for.
pub fn previous(app: &tauri::AppHandle, serial: &str, root: &str) -> Option<ScanRecord> {
    previous_in(&path(app)?, serial, root)
}

/// Record a finished scan, replacing any earlier entry for the same device and
/// root. A scan still in progress is not recorded. Failures are silent, as for
/// [`previous`].
pub fn record(app: &tauri::AppHandle, serial: &str, root: &str, stats: &Stats) {
    if let Some(file) = path(app) {
        let _ = record_in(&file, serial, root, stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory per test, so parallel tests do not share a file.
    /// Removed again when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "socketsweep-history-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            Scratch(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("scan-history.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn finished(size: u64) -> Stats {
        Stats {
            size,
            files: 2,
            dirs: 3,
            scanning: false,
        }
    }

    #[test]
    fn records_are_kept_per_device_and_root() {
        let scratch = Scratch::new("keyed");
        let file = scratch.file();
        record_in(&file, "A", "/sdcard", &finished(10)).unwrap();
        record_in(&file, "A", "/storage/1A2B-3C4D", &finished(20)).unwrap();
        record_in(&file, "B", "/sdcard", &finished(30)).unwrap();

        assert_eq!(previous_in(&file, "A", "/sdcard").unwrap().size, 10);
        assert_eq!(
            previous_in(&file, "A", "/storage/1A2B-3C4D").unwrap().size,
            20
        );
        assert_eq!(previous_in(&file, "B", "/sdcard").unwrap().size, 30);
        assert!(previous_in(&file, "B", "/storage/1A2B-3C4D").is_none());

        record_in(&file, "A", "/sdcard", &finished(11)).unwrap();
        assert_eq!(previous_in(&file, "A", "/sdcard").unwrap().size, 11);
        assert_eq!(
            previous_in(&file, "A", "/storage/1A2B-3C4D").unwrap().size,
            20,
            "replacing one root leaves the other alone"
        );
    }

    #[test]
    fn an_unfinished_scan_is_not_recorded() {
        let scratch = Scratch::new("unfinished");
        let file = scratch.file();
        let mut stats = finished(10);
        stats.scanning = true;
        record_in(&file, "A", "/sdcard", &stats).unwrap();
        assert!(previous_in(&file, "A", "/sdcard").is_none());
    }

    #[test]
    fn a_legacy_per_serial_entry_reads_as_the_default_root() {
        let scratch = Scratch::new("legacy");
        let file = scratch.file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{ "A": { "size": 5, "files": 1, "dirs": 1, "at": 99 } }"#,
        )
        .unwrap();

        let old = previous_in(&file, "A", DEFAULT_ROOT).unwrap();
        assert_eq!((old.size, old.at), (5, 99));
        assert!(previous_in(&file, "A", "/storage/emulated/0").is_none());

        // Recording another root keeps the migrated entry.
        record_in(&file, "A", "/storage/1A2B-3C4D", &finished(7)).unwrap();
        assert_eq!(previous_in(&file, "A", DEFAULT_ROOT).unwrap().size, 5);
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let scratch = Scratch::new("atomic");
        let file = scratch.file();
        record_in(&file, "A", "/sdcard", &finished(1)).unwrap();
        let entries: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from("scan-history.json")]);
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_and_is_replaced() {
        let scratch = Scratch::new("corrupt");
        let file = scratch.file();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{ not json").unwrap();
        assert!(previous_in(&file, "A", "/sdcard").is_none());

        record_in(&file, "A", "/sdcard", &finished(3)).unwrap();
        assert_eq!(previous_in(&file, "A", "/sdcard").unwrap().size, 3);
    }
}

//! The last finished scan of each device and root, for "what changed since
//! last time".
//!
//! # File format
//!
//! JSON, serial → root → record:
//!
//! ```json
//! { "R58M12345": { "/sdcard": { "size": { "apparent": 1, "disk": 2 }, "files": 2, "dirs": 3, "at": 1700000000 } } }
//! ```
//!
//! Two older shapes are read too: a bare number for `size` (apparent bytes,
//! read as both figures) and one record per serial (read as that device's
//! `/sdcard`). Both are rewritten in the current shape on the next save.
//!
//! The file is replaced atomically, written beside the original and renamed
//! over it, so a crash mid-write leaves the previous history. Every failure is
//! silent: history is a nicety, not worth interrupting a scan for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use socketsweep_gui_contract::{ScanRecord, Size, Totals};

use crate::daemon::DEFAULT_ROOT;

/// A size as stored: current, or a bare number from older files.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredSize {
    Both(Size),
    Legacy(u64),
}

#[derive(Deserialize)]
struct StoredRecord {
    size: StoredSize,
    files: u32,
    dirs: u32,
    at: u64,
}

impl From<StoredRecord> for ScanRecord {
    fn from(r: StoredRecord) -> Self {
        let size = match r.size {
            StoredSize::Both(size) => size,
            StoredSize::Legacy(n) => Size {
                apparent: n,
                disk: n,
            },
        };
        ScanRecord {
            size,
            files: r.files,
            dirs: r.dirs,
            at: r.at,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDevice {
    ByRoot(HashMap<String, StoredRecord>),
    Legacy(StoredRecord),
}

type Map = HashMap<String, HashMap<String, ScanRecord>>;

/// The history file.
#[derive(Debug, Clone)]
pub struct History {
    file: PathBuf,
}

impl History {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        History { file: file.into() }
    }

    /// The record for a device and root, if there is one.
    pub fn previous(&self, serial: &str, root: &str) -> Option<ScanRecord> {
        load(&self.file).get(serial)?.get(root).copied()
    }

    /// Remember a finished scan's totals, replacing any earlier record for
    /// the same device and root.
    pub fn record(&self, serial: &str, root: &str, totals: &Totals) {
        let record = ScanRecord {
            size: totals.size,
            files: totals.files,
            dirs: totals.dirs,
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        let mut map = load(&self.file);
        map.entry(serial.to_string())
            .or_default()
            .insert(root.to_string(), record);
        let _ = save(&self.file, &map);
    }
}

fn load(file: &Path) -> Map {
    let Ok(raw) = std::fs::read_to_string(file) else {
        return Map::new();
    };
    let Ok(stored) = serde_json::from_str::<HashMap<String, StoredDevice>>(&raw) else {
        return Map::new();
    };
    stored
        .into_iter()
        .map(|(serial, device)| {
            let roots = match device {
                StoredDevice::ByRoot(roots) => {
                    roots.into_iter().map(|(k, v)| (k, v.into())).collect()
                }
                StoredDevice::Legacy(r) => HashMap::from([(DEFAULT_ROOT.to_string(), r.into())]),
            };
            (serial, roots)
        })
        .collect()
}

fn save(file: &Path, map: &Map) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(map).map_err(std::io::Error::other)?;
    let mut temp = file.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            Scratch(std::env::temp_dir().join(format!(
                "socketsweep-history-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
        }

        fn history(&self) -> History {
            History::new(self.0.join("scan-history.json"))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn totals(disk: u64) -> Totals {
        Totals {
            size: Size {
                apparent: disk / 2,
                disk,
            },
            files: 2,
            dirs: 3,
            errors: 0,
        }
    }

    #[test]
    fn records_are_kept_per_device_and_root() {
        let s = Scratch::new("keyed");
        let h = s.history();
        h.record("A", "/sdcard", &totals(10));
        h.record("A", "/storage/1A2B-3C4D", &totals(20));
        h.record("B", "/sdcard", &totals(30));
        assert_eq!(h.previous("A", "/sdcard").unwrap().size.disk, 10);
        assert_eq!(h.previous("A", "/storage/1A2B-3C4D").unwrap().size.disk, 20);
        assert_eq!(h.previous("B", "/sdcard").unwrap().size.apparent, 15);
        assert!(h.previous("B", "/storage/1A2B-3C4D").is_none());
        h.record("A", "/sdcard", &totals(11));
        assert_eq!(h.previous("A", "/sdcard").unwrap().size.disk, 11);
        assert_eq!(h.previous("A", "/storage/1A2B-3C4D").unwrap().size.disk, 20);
    }

    #[test]
    fn v3_files_are_read() {
        let s = Scratch::new("legacy");
        let h = s.history();
        std::fs::create_dir_all(&s.0).unwrap();
        std::fs::write(
            &h.file,
            r#"{ "A": { "size": 5, "files": 1, "dirs": 1, "at": 99 },
                 "B": { "/sdcard": { "size": 7, "files": 1, "dirs": 1, "at": 98 } } }"#,
        )
        .unwrap();
        let a = h.previous("A", DEFAULT_ROOT).unwrap();
        assert_eq!((a.size.apparent, a.size.disk, a.at), (5, 5, 99));
        assert_eq!(h.previous("B", "/sdcard").unwrap().size.disk, 7);
        h.record("A", "/storage/1A2B-3C4D", &totals(8));
        assert_eq!(h.previous("A", DEFAULT_ROOT).unwrap().size.disk, 5);
    }

    #[test]
    fn saving_is_atomic_and_a_corrupt_file_reads_as_empty() {
        let s = Scratch::new("atomic");
        let h = s.history();
        std::fs::create_dir_all(&s.0).unwrap();
        std::fs::write(&h.file, "{ not json").unwrap();
        assert!(h.previous("A", "/sdcard").is_none());
        h.record("A", "/sdcard", &totals(3));
        assert_eq!(h.previous("A", "/sdcard").unwrap().size.disk, 3);
        let names: Vec<_> = std::fs::read_dir(&s.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("scan-history.json")]);
    }
}

//! Remembers the last scan of each device.
//!
//! Storage is a filing cabinet people return to, so "what changed since last
//! time" is usually more useful than the absolute total. Keyed by serial, since
//! a desktop may see several phones.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::arena::Stats;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub size: u64,
    pub files: u32,
    pub dirs: u32,
    /// Unix seconds. The frontend formats it; the host does not guess a locale.
    pub at: u64,
}

impl ScanRecord {
    pub fn from_stats(stats: &Stats) -> Self {
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

type History = HashMap<String, ScanRecord>;

fn path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("scan-history.json"))
}

/// The stored record for a device, if there is one.
///
/// Every failure here is silent and returns `None`: a missing or unreadable
/// history file is a nicety not working, and is not worth interrupting a scan
/// the user actually asked for.
pub fn previous(app: &tauri::AppHandle, serial: &str) -> Option<ScanRecord> {
    let file = path(app)?;
    let raw = std::fs::read_to_string(file).ok()?;
    let history: History = serde_json::from_str(&raw).ok()?;
    history.get(serial).copied()
}

/// Record a completed scan, replacing any earlier entry for the same device.
pub fn record(app: &tauri::AppHandle, serial: &str, stats: &Stats) {
    let Some(file) = path(app) else { return };

    if let Some(dir) = file.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }

    let mut history: History = std::fs::read_to_string(&file)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    history.insert(serial.to_string(), ScanRecord::from_stats(stats));

    if let Ok(json) = serde_json::to_string_pretty(&history) {
        let _ = std::fs::write(file, json);
    }
}

//! Devices and sessions: the phone picker, `connect` and `startScan`.

use serde::{Deserialize, Serialize};
use specta::Type;
use specta_typescript::Number;

use crate::{NodeRef, SessionId, Size};

/// An attached phone, usable or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// Human-readable model for the picker; falls back to the serial.
    pub model: String,
    pub state: DeviceState,
    /// True only when `state` is `ready`.
    pub usable: bool,
    /// What to do about a device that cannot be used yet, when the host knows.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DeviceState {
    Ready,
    /// Waiting for the user to accept the debugging prompt on the phone.
    Unauthorized,
    Offline,
    /// The computer will not let adb open it (Linux without a udev rule).
    NoPermission,
    Other,
}

/// The answer to `connect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub serial: String,
    pub model: String,
    /// The root that scans default to, as the user knows it (`/sdcard`).
    pub root: String,
    /// Where `copyToDesktop` copies when no destination is given, as a path on
    /// this computer (the desktop), so the GUI can name the action up front.
    /// `null` when the host knows no default; a copy must then name one.
    pub copy_destination: Option<String>,
    /// The last finished scan of this device and root, if it has been seen
    /// before, for showing what changed.
    pub previous: Option<ScanRecord>,
}

/// A remembered scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub size: Size,
    pub files: u32,
    pub dirs: u32,
    /// Unix seconds. The GUI formats it.
    #[specta(type = Number)]
    pub at: u64,
}

/// The answer to `startScan`: the scan is under way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanStarted {
    /// The new tree. Every node from an earlier session is stale.
    pub session: SessionId,
    pub root: NodeRef,
    /// The root as the user knows it, for the first breadcrumb.
    pub path: String,
}

/// Sums over a tree or subtree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub size: Size,
    pub files: u32,
    pub dirs: u32,
    /// Folders that could not be read and files that could not be measured.
    pub errors: u32,
}

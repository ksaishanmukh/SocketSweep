//! How every command fails.

use serde::{Deserialize, Serialize};
use specta::Type;

// ── Errors ──────────────────────────────────────────────────────────────────

/// How every command fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: ErrorCode,
    /// For the user, already worded.
    pub message: String,
}

impl CommandError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        CommandError {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// No device is connected.
    NotConnected,
    /// Connected, but no scan has started.
    NoScan,
    /// The node is from an earlier session. The GUI refreshes.
    Stale,
    /// The node has since been deleted.
    Removed,
    /// Another operation that cannot overlap this one is running.
    Busy,
    /// adb or the phone refused or failed.
    Device,
    /// The daemon on the phone refused or failed.
    Daemon,
    /// The user cancelled it.
    Cancelled,
    /// The arguments make no sense, e.g. deleting the scan root.
    Invalid,
    /// A file on this computer could not be written.
    Io,
}

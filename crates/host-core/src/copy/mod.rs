//! Copy to this computer: map a [`CopyManifest`](socketsweep_wire::ServerMsg::CopyManifest)
//! onto local paths, then pull every file with [`Transport::pull`].
//!
//! # Names
//!
//! Android names are arbitrary bytes; Windows (and to a lesser degree macOS)
//! refuse many of them. Every component goes through [`host_safe_name`], the
//! same mapping on every platform so a copy looks the same everywhere:
//!
//! - invalid UTF-8 becomes U+FFFD;
//! - `< > : " / \ | ? *` and control characters become `_`;
//! - trailing dots and spaces, which Windows silently drops, become `_`;
//! - reserved device names (`CON`, `nul.txt`, `COM1`, …) get a leading `_`;
//! - an empty name becomes `_`.
//!
//! Two names that map to the same result, or differ only in case (Windows and
//! macOS compare names case-insensitively), are told apart with ` (2)`,
//! ` (3)`, … before the extension. Each changed name counts as renamed.
//!
//! # Pulling
//!
//! Directories are created first, then files are pulled by [`COPY_WORKERS`]
//! threads, as the spec's 2–3 parallel sync connections. Cancelling stops
//! workers taking new files; a file part-way through finishes (the transport
//! cannot be interrupted), and files already copied stay. A failed pull's
//! partial file is removed.
//!
//! # Layout
//!
//! `names` makes names host-safe, `plan` maps a manifest onto local paths, and
//! `pull` creates and pulls them.

mod names;
mod plan;
mod pull;

use std::path::PathBuf;

use socketsweep_transport::TransportError;
use socketsweep_wire::Kind;

#[cfg(doc)]
use socketsweep_transport::Transport;

pub use names::host_safe_name;
pub use plan::plan;
pub use pull::execute;

/// Files pulled at once.
pub const COPY_WORKERS: usize = 3;

/// One thing to create or pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedItem {
    /// The phone path.
    pub remote: Vec<u8>,
    pub local: PathBuf,
    pub kind: Kind,
    pub size: u64,
    pub mtime: i64,
    /// Relative to what was asked for, for display.
    pub rel: String,
}

/// A manifest mapped onto this computer.
#[derive(Debug, Clone, Default)]
pub struct CopyPlan {
    /// Where the copy lands: the copied folder or file itself.
    pub top: PathBuf,
    /// Directories, parents before children.
    pub dirs: Vec<PlannedItem>,
    pub files: Vec<PlannedItem>,
    /// Symbolic links and special files, not copied.
    pub skipped: u32,
    /// Items stored under a changed name.
    pub renamed: u32,
    pub bytes: u64,
}

/// Progress of a running copy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyTick {
    pub files_done: u32,
    pub bytes_done: u64,
    pub current: Option<String>,
}

/// Why a copy stopped.
#[derive(Debug)]
pub enum CopyError {
    Cancelled,
    /// A local file or folder could not be written.
    Local {
        path: PathBuf,
        source: std::io::Error,
    },
    Transport(TransportError),
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyError::Cancelled => write!(f, "The copy was cancelled."),
            CopyError::Local { path, source } => {
                write!(f, "Cannot write {}: {source}", path.display())
            }
            CopyError::Transport(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CopyError {}

#[cfg(test)]
mod tests;

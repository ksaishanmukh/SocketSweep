//! The daemon's lifecycle on the phone, over the [`Transport`].
//!
//! Check the phone can run it, push the matching build, launch it with the
//! session token in its environment, open the tunnel, and complete the
//! handshake, retrying while it binds its socket. On any failure after the
//! push, and on disconnect, everything is torn down again: the daemon is
//! killed and its binary removed.
//!
//! The socket name is fresh per session, so a stale daemon cannot be mistaken
//! for the current one; the token, not the name, is the security boundary.

mod command;
mod lifecycle;
mod random;

use std::path::{Path, PathBuf};
use std::time::Duration;

use socketsweep_transport::{DeviceInfo, TransportError};

#[cfg(doc)]
use socketsweep_transport::Transport;

use crate::client::{Client, ClientConfig, ClientError};

pub use command::{check_root, launch_command, parse_launch_command, shell_quote, LaunchCommand};
pub use lifecycle::tear_down;
pub use random::{random_session, random_token};

/// The root scanned when none is named.
pub const DEFAULT_ROOT: &str = "/sdcard";

/// Where the daemon is pushed.
pub const DEVICE_BIN: &str = "/data/local/tmp/socketsweep-daemon";

/// The daemon's stderr, quoted in the error when it never answers.
pub const DEVICE_LOG: &str = "/data/local/tmp/socketsweep.log";

const LOG_EXCERPT_BYTES: usize = 2048;

/// Android 11.
pub const MIN_SDK: u32 = 30;

/// Kill any running daemon. The bracket stops the pattern matching the shell
/// that runs it, which would kill the shell instead.
const KILL_DAEMON: &str = "pkill -f '[s]ocketsweep-daemon'";

/// Lets the shell user read all of shared storage under Scoped Storage.
const GRANT_STORAGE: &str = "appops set com.android.shell MANAGE_EXTERNAL_STORAGE allow";

/// The daemon builds the caller has, by ABI. Paths on this computer.
#[derive(Debug, Clone, Default)]
pub struct DaemonBinaries {
    /// For phones (`arm64-v8a`).
    pub arm64: Option<PathBuf>,
    /// For the emulator (`x86_64`).
    pub x86_64: Option<PathBuf>,
}

impl DaemonBinaries {
    /// The build for a phone reporting `info`, or why there is none.
    pub fn pick(&self, info: &DeviceInfo) -> Result<&Path, LaunchError> {
        if info.sdk < MIN_SDK {
            return Err(LaunchError::Unsupported(format!(
                "SocketSweep needs Android 11 or newer (this phone runs API {}).",
                info.sdk
            )));
        }
        let path = match info.abi.as_str() {
            "arm64-v8a" => self.arm64.as_deref(),
            "x86_64" => self.x86_64.as_deref(),
            other => {
                return Err(LaunchError::Unsupported(format!(
                    "SocketSweep supports 64-bit ARM phones and x86_64 emulators only (this one is {other})."
                )))
            }
        };
        path.ok_or_else(|| {
            LaunchError::Missing(format!(
                "This build of SocketSweep has no daemon for {}. Run `npm run setup` if you are running from source.",
                info.abi
            ))
        })
    }
}

/// How long to keep trying.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// How long the daemon gets to start answering.
    pub startup_deadline: Duration,
    /// Pause between handshake attempts.
    pub retry_gap: Duration,
    pub client: ClientConfig,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            startup_deadline: Duration::from_secs(10),
            retry_gap: Duration::from_millis(50),
            client: ClientConfig::default(),
        }
    }
}

/// Why the daemon could not be started.
#[derive(Debug)]
pub enum LaunchError {
    /// The root is not one the app scans.
    BadRoot(String),
    /// The phone cannot run the daemon.
    Unsupported(String),
    /// The daemon build for this phone is not available.
    Missing(String),
    Transport(TransportError),
    /// The daemon refused us or speaks another version: retrying will not help.
    Handshake(ClientError),
    /// It never answered. Carries the last error and the daemon's log.
    NeverAnswered {
        last: String,
        log: Option<String>,
    },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::BadRoot(m) | LaunchError::Unsupported(m) | LaunchError::Missing(m) => {
                f.write_str(m)
            }
            LaunchError::Transport(e) => write!(f, "{e}"),
            LaunchError::Handshake(e) => write!(f, "{e}"),
            LaunchError::NeverAnswered { last, log } => {
                write!(
                    f,
                    "The daemon was started on the phone but never answered. Last error: {last}"
                )?;
                if let Some(log) = log {
                    write!(f, "\n\nDaemon log:\n{log}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LaunchError {}

impl From<TransportError> for LaunchError {
    fn from(e: TransportError) -> Self {
        LaunchError::Transport(e)
    }
}

/// A running daemon and the connection to it.
pub struct Daemon {
    pub serial: String,
    pub socket: String,
    pub root: String,
    pub info: DeviceInfo,
    pub client: Client,
}

#[cfg(test)]
mod tests;

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

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rand::Rng;
use socketsweep_transport::{DeviceInfo, Transport, TransportError};
use socketsweep_wire::{token_to_hex, Caps, SessionId, Token, TOKEN_ENV, TOKEN_LEN};

use crate::client::{Client, ClientConfig, ClientError};

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
/// that runs it (see v3 `session.rs`), so it stays a command of its own.
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

/// Accept `root` only if it is a storage root a phone exposes: `/sdcard`,
/// `/storage/emulated/<user>`, or an SD card at `/storage/XXXX-XXXX`. It goes
/// on the daemon's command line, so anything else is refused rather than
/// interpreted.
pub fn check_root(root: &str) -> Result<(), LaunchError> {
    fn is_user_storage(rest: &str) -> bool {
        !rest.is_empty() && rest.len() <= 9 && rest.bytes().all(|b| b.is_ascii_digit())
    }
    fn is_sd_card(rest: &str) -> bool {
        let b = rest.as_bytes();
        b.len() == 9
            && b[4] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || c.is_ascii_hexdigit())
    }
    let ok = root == DEFAULT_ROOT
        || root
            .strip_prefix("/storage/emulated/")
            .is_some_and(is_user_storage)
        || root.strip_prefix("/storage/").is_some_and(is_sd_card);
    if ok {
        Ok(())
    } else {
        Err(LaunchError::BadRoot(format!(
            "Cannot scan {root:?}. Choose /sdcard, /storage/emulated/<user>, or an SD card at /storage/XXXX-XXXX."
        )))
    }
}

/// Quote `s` as one word for `sh`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The command that starts the daemon in the background. The token travels
/// in the environment, never argv, where other apps could read it.
pub fn launch_command(token: &Token, socket: &str, root: &str) -> String {
    format!(
        "{TOKEN_ENV}={} nohup {DEVICE_BIN} --socket {} --root {} </dev/null >/dev/null 2>{DEVICE_LOG} &",
        token_to_hex(token),
        shell_quote(socket),
        shell_quote(root),
    )
}

/// What [`launch_command`] carries, for fake daemons that play the phone's
/// shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub token_hex: String,
    pub socket: String,
    pub root: String,
}

/// Read back a [`launch_command`]; `None` for any other command.
pub fn parse_launch_command(command: &str) -> Option<LaunchCommand> {
    let rest = command.strip_prefix(TOKEN_ENV)?.strip_prefix('=')?;
    let (token_hex, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix("nohup ")?.strip_prefix(DEVICE_BIN)?;
    let rest = rest.strip_prefix(" --socket ")?;
    let (socket, rest) = take_quoted(rest)?;
    let rest = rest.strip_prefix(" --root ")?;
    let (root, _) = take_quoted(rest)?;
    Some(LaunchCommand {
        token_hex: token_hex.to_string(),
        socket,
        root,
    })
}

/// One word quoted by [`shell_quote`], and what follows it.
fn take_quoted(s: &str) -> Option<(String, &str)> {
    let mut rest = s.strip_prefix('\'')?;
    let mut out = String::new();
    loop {
        let end = rest.find('\'')?;
        out.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix(r"\''") {
            Some(after) => {
                out.push('\'');
                rest = after;
            }
            None => return Some((out, rest)),
        }
    }
}

fn random_socket_name() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    let suffix: String = (0..16)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect();
    format!("socketsweep-{suffix}")
}

/// A fresh token from the operating system's CSPRNG.
pub fn random_token() -> Token {
    let mut token = [0u8; TOKEN_LEN];
    rand::rng().fill(&mut token);
    token
}

pub fn random_session() -> SessionId {
    let mut s = [0u8; 16];
    rand::rng().fill(&mut s);
    s
}

/// The opening of the daemon's log, for an error message.
fn daemon_log(transport: &dyn Transport, serial: &str) -> Option<String> {
    let out = transport.shell(serial, &format!("cat {DEVICE_LOG}")).ok()?;
    let log = out.stdout_text();
    let log = log.trim();
    if log.is_empty() {
        return None;
    }
    let mut end = log.len().min(LOG_EXCERPT_BYTES);
    while !log.is_char_boundary(end) {
        end -= 1;
    }
    let ellipsis = if end < log.len() { "…" } else { "" };
    Some(format!("{}{ellipsis}", &log[..end]))
}

/// Kill the daemon and remove its binary. Best effort: the phone may be gone.
pub fn tear_down(transport: &dyn Transport, serial: &str) {
    let _ = transport.shell(serial, KILL_DAEMON);
    let _ = transport.shell(serial, &format!("rm -f {DEVICE_BIN}"));
}

/// A running daemon and the connection to it.
pub struct Daemon {
    pub serial: String,
    pub socket: String,
    pub root: String,
    pub info: DeviceInfo,
    pub client: Client,
}

impl Daemon {
    /// Start the daemon on `serial` for `root` and connect to it. Blocks for
    /// as long as that takes, up to the startup deadline.
    pub fn start(
        transport: &dyn Transport,
        serial: &str,
        root: &str,
        binaries: &DaemonBinaries,
        options: &LaunchOptions,
    ) -> Result<Daemon, LaunchError> {
        check_root(root)?;
        let info = transport.device_info(serial)?;
        let binary = binaries.pick(&info)?;

        // A daemon left over from a crashed run holds the old binary.
        let _ = transport.shell(serial, KILL_DAEMON);
        let _ = transport.shell(serial, GRANT_STORAGE);
        transport.push(serial, binary, DEVICE_BIN, 0o755)?;

        let result = Self::launch(transport, serial, root, info, options);
        if result.is_err() {
            tear_down(transport, serial);
        }
        result
    }

    fn launch(
        transport: &dyn Transport,
        serial: &str,
        root: &str,
        info: DeviceInfo,
        options: &LaunchOptions,
    ) -> Result<Daemon, LaunchError> {
        let socket = random_socket_name();
        let token = random_token();
        let session = random_session();
        transport.shell(serial, &launch_command(&token, &socket, root))?;

        let deadline = Instant::now() + options.startup_deadline;
        let last = loop {
            let attempt = match transport.open_tunnel(serial, &socket) {
                Err(e) => Err(e.to_string()),
                Ok(stream) => {
                    match Client::connect(
                        stream,
                        token,
                        session,
                        Caps::all(),
                        options.client.clone(),
                    ) {
                        Ok(client) => Ok(client),
                        Err(e) if e.retryable() => Err(e.to_string()),
                        Err(e) => return Err(LaunchError::Handshake(e)),
                    }
                }
            };
            match attempt {
                Ok(client) => {
                    return Ok(Daemon {
                        serial: serial.to_string(),
                        socket,
                        root: root.to_string(),
                        info,
                        client,
                    })
                }
                Err(e) if Instant::now() >= deadline => break e,
                Err(_) => std::thread::sleep(options.retry_gap),
            }
        };
        Err(LaunchError::NeverAnswered {
            last,
            log: daemon_log(transport, serial),
        })
    }

    /// Ask the daemon to exit, close the connection, and clean up the phone.
    pub fn stop(&self, transport: &dyn Transport) {
        self.client.shutdown();
        tear_down(transport, &self.serial);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supported_roots_are_accepted() {
        for root in [
            "/sdcard",
            "/storage/emulated/0",
            "/storage/emulated/10",
            "/storage/1A2B-3C4D",
        ] {
            assert!(check_root(root).is_ok(), "{root}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for root in [
            "",
            "/",
            "/sdcard/",
            "/sdcard/DCIM",
            "/sdcard; rm -rf /",
            "/sdcard'",
            "/storage/emulated/",
            "/storage/emulated/0/",
            "/storage/1A2B-3C4G",
            "/data",
            "/storage/emulated/../../data",
        ] {
            assert!(check_root(root).is_err(), "{root:?} should be refused");
        }
    }

    #[test]
    fn shell_quoting_survives_single_quotes_and_spaces() {
        assert_eq!(shell_quote("/sdcard"), "'/sdcard'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn the_launch_command_round_trips_and_keeps_the_token_off_argv() {
        let token = [0xAB; TOKEN_LEN];
        let cmd = launch_command(&token, "socketsweep-x", "/storage/emulated/0");
        assert!(cmd.starts_with(&format!("{TOKEN_ENV}={}", token_to_hex(&token))));
        let after_bin = cmd.split(DEVICE_BIN).nth(1).unwrap();
        assert!(!after_bin.contains(&token_to_hex(&token)));
        let parsed = parse_launch_command(&cmd).unwrap();
        assert_eq!(parsed.token_hex, token_to_hex(&token));
        assert_eq!(parsed.socket, "socketsweep-x");
        assert_eq!(parsed.root, "/storage/emulated/0");
        assert_eq!(
            take_quoted(r"'it'\''s' rest"),
            Some(("it's".into(), " rest"))
        );
        assert!(parse_launch_command("pkill -f x").is_none());
    }

    #[test]
    fn the_binary_is_picked_by_abi_and_old_phones_are_refused() {
        let bins = DaemonBinaries {
            arm64: Some("arm".into()),
            x86_64: None,
        };
        let info = |sdk, abi: &str| DeviceInfo {
            sdk,
            abi: abi.into(),
            model: "m".into(),
        };
        assert_eq!(bins.pick(&info(34, "arm64-v8a")).unwrap(), Path::new("arm"));
        assert!(matches!(
            bins.pick(&info(34, "x86_64")),
            Err(LaunchError::Missing(_))
        ));
        assert!(matches!(
            bins.pick(&info(29, "arm64-v8a")),
            Err(LaunchError::Unsupported(_))
        ));
        assert!(matches!(
            bins.pick(&info(34, "armeabi-v7a")),
            Err(LaunchError::Unsupported(_))
        ));
    }
}

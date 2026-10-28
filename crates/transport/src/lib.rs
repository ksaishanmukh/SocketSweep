//! How the host reaches a phone: the [`Transport`] trait.
//!
//! host-core depends on this trait only, never on adb, so everything above it
//! can be tested against [`FakeTransport`](fake::FakeTransport), an in-memory
//! phone. The real implementation is `crates/transport-adb`, which speaks to
//! the adb server's smart sockets directly; see `docs/architecture.md`,
//! section 1.
//!
//! # What a transport does
//!
//! - lists attached [`Device`]s, including ones that are not usable yet, so the
//!   GUI can say what to do about them;
//! - runs shell commands and reports their [`Output`];
//! - pushes the daemon to the phone and pulls files back for copying;
//! - opens a byte tunnel ([`Duplex`]) to the daemon's abstract socket, hiding
//!   how: adb forwards a host port today, but no caller ever sees the port.
//!
//! Every method names the device by serial, so two attached phones never get
//! each other's commands, and every method may block: call them off any thread
//! that must stay responsive.
//!
//! # Paths
//!
//! Remote paths the host chose itself (where the daemon is pushed) are `&str`.
//! Remote paths that came from the phone's file system (what is pulled) are
//! `&[u8]`: Android file names are arbitrary bytes, and adb's sync protocol
//! carries them as such.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

/// A bidirectional byte stream to the daemon, such as a forwarded TCP socket.
///
/// Shaped like [`TcpStream`]: methods take `&self`, and timeouts and shutdown
/// apply to the connection, so every handle from [`Duplex::try_clone`] sees
/// them. The session client reads on one thread through one handle and writes
/// through another.
///
/// Dropping the last handle closes the stream and releases whatever the
/// transport set up for it, such as an adb port forward.
pub trait Duplex: Read + Write + Send {
    /// Close both directions. Pending and later reads on every handle return
    /// end of stream, which is how a blocked reader thread is woken.
    fn shutdown(&self) -> io::Result<()>;

    /// `None` blocks forever. A read that times out fails with
    /// [`io::ErrorKind::TimedOut`] or [`io::ErrorKind::WouldBlock`] (platforms
    /// differ for sockets); callers treat both alike.
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;

    /// As [`Duplex::set_read_timeout`], for writes.
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;

    /// Another handle to the same stream.
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>>;
}

impl Duplex for TcpStream {
    fn shutdown(&self) -> io::Result<()> {
        match TcpStream::shutdown(self, Shutdown::Both) {
            // Already closed by the peer: the goal is met.
            Err(e) if e.kind() == io::ErrorKind::NotConnected => Ok(()),
            other => other,
        }
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_write_timeout(self, timeout)
    }

    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(TcpStream::try_clone(self)?))
    }
}

/// The host's way to a phone. See the [crate documentation](crate).
pub trait Transport: Send + Sync {
    /// Every attached device, including ones that are not usable yet, so the
    /// GUI can tell "nothing plugged in" from "tap Allow on the phone". One
    /// device in an odd state never hides the others.
    fn devices(&self) -> Result<Vec<Device>>;

    /// Facts about the phone that decide which daemon build it runs.
    fn device_info(&self, serial: &str) -> Result<DeviceInfo>;

    /// Run `command` in the phone's `sh`.
    ///
    /// The command is handed over verbatim: anything interpolated into it
    /// must already be quoted for the shell. A command that runs and exits
    /// non-zero is `Ok`, with the code in [`Output::exit`]; `Err` means it
    /// could not be run at all.
    fn shell(&self, serial: &str, command: &str) -> Result<Output>;

    /// Copy the local file `local` to `remote` on the phone, with permission
    /// bits `mode` (e.g. `0o755` for the daemon), replacing any file there.
    fn push(&self, serial: &str, local: &Path, remote: &str, mode: u32) -> Result<()>;

    /// Copy the phone's file `remote` to `local`, creating or truncating it.
    ///
    /// `progress` is called with the bytes received so far, as they arrive.
    /// Returns the file's size. On failure `local` may be left partial; the
    /// caller removes it.
    fn pull(
        &self,
        serial: &str,
        remote: &[u8],
        local: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<u64>;

    /// Connect to the abstract unix socket `name` on the phone, where the
    /// daemon listens.
    ///
    /// How is the transport's business (adb forwards a host port it picks).
    /// Dropping the returned stream releases it all. A tunnel to a name
    /// nothing listens on may succeed and then read end of stream at once,
    /// as an adb forward does; callers find out at the handshake.
    fn open_tunnel(&self, serial: &str, name: &str) -> Result<Box<dyn Duplex>>;
}

/// One attached device, usable or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    /// Human-readable model, for the picker. Falls back to the serial.
    pub model: String,
    pub state: DeviceState,
    /// What the user should do about a device that is not usable, when the
    /// transport knows (a missing udev rule, say).
    pub hint: Option<String>,
}

impl Device {
    /// Only a device in [`DeviceState::Ready`] can be used.
    pub fn usable(&self) -> bool {
        self.state == DeviceState::Ready
    }
}

/// A device's connection state, as adb reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// Connected and authorised (adb's `device`).
    Ready,
    /// The user has not accepted the debugging prompt on the phone.
    Unauthorized,
    /// Visible but not responding; reconnecting usually fixes it.
    Offline,
    /// The host OS will not let adb open it (Linux without a udev rule).
    NoPermission,
    /// Anything else, as reported, so a new state is shown rather than hidden.
    Other(String),
}

/// Facts about a phone that decide which daemon build it can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Android API level, from `ro.build.version.sdk`.
    pub sdk: u32,
    /// Primary ABI, from `ro.product.cpu.abi`, e.g. `arm64-v8a`.
    pub abi: String,
    /// `ro.product.model`, e.g. `Pixel 8`.
    pub model: String,
}

/// What a shell command produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The exit code, or `None` from a phone too old for adb's shell protocol
    /// v2, which mixes stderr into stdout and reports no exit code.
    pub exit: Option<i32>,
}

impl Output {
    /// Exited 0, or did not say otherwise.
    pub fn success(&self) -> bool {
        self.exit.is_none_or(|code| code == 0)
    }

    /// stdout as text, lossily, for parsing `getprop` and the like.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// Why a transport call failed. The messages are written for the user.
#[derive(Debug)]
pub enum TransportError {
    /// The transport itself (the adb server) could not be reached or started.
    Server(String),
    /// No device is attached at all.
    NoDevices,
    /// No device with this serial is attached (any more).
    NotFound { serial: String },
    /// The device is attached but not usable in its current state.
    Unusable {
        serial: String,
        state: DeviceState,
        hint: Option<String>,
    },
    /// The device or server refused the request, with its reason (adb's
    /// `FAIL`): no such file on the phone, a full disk, a refused forward.
    Refused(String),
    /// A local file could not be read or written.
    Local { path: PathBuf, source: io::Error },
    /// The connection failed part-way.
    Io(io::Error),
    /// The other end said something this transport does not understand.
    Protocol(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Server(e) => write!(f, "Cannot talk to the ADB server: {e}"),
            TransportError::NoDevices => write!(
                f,
                "No Android device detected. Connect your phone over USB and enable USB Debugging."
            ),
            TransportError::NotFound { serial } => {
                write!(f, "Device {serial} is no longer connected.")
            }
            TransportError::Unusable {
                serial,
                state,
                hint,
            } => {
                match state {
                    DeviceState::Unauthorized => write!(
                        f,
                        "Device {serial} is unauthorized. Accept the \u{201c}Allow USB debugging\u{201d} prompt on the phone."
                    )?,
                    DeviceState::Offline => {
                        write!(f, "Device {serial} is offline. Unplug and reconnect it.")?
                    }
                    DeviceState::NoPermission => write!(f, "Device {serial} cannot be opened.")?,
                    DeviceState::Ready => write!(f, "Device {serial} is not ready.")?,
                    DeviceState::Other(s) => write!(f, "Device {serial} is {s}.")?,
                }
                match hint {
                    Some(hint) => write!(f, " {hint}"),
                    None => Ok(()),
                }
            }
            TransportError::Refused(reason) => write!(f, "{reason}"),
            TransportError::Local { path, source } => {
                write!(f, "Cannot use {}: {source}", path.display())
            }
            TransportError::Io(e) => write!(f, "Lost the connection to the device: {e}"),
            TransportError::Protocol(e) => write!(f, "Unexpected reply from the device: {e}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::Local { source, .. } => Some(source),
            TransportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(e: io::Error) -> Self {
        TransportError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, TransportError>;

#[cfg(test)]
mod tests;

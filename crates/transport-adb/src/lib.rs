//! The adb implementation of [`Transport`], spoken directly to the adb
//! server's TCP socket.
//!
//! The adb *server* is still required: on Windows and macOS it owns the USB
//! claim. Going driverless would mean implementing ADB's RSA auth handshake and
//! USB transport, which is the line scrcpy chose not to cross either. What this
//! crate avoids is everything around it:
//!
//!   - **No subprocess per call.** Every request is a short-lived connection to
//!     127.0.0.1:5037 using adb's "smart socket" protocol: a 4-hex-digit length
//!     and the request, answered by `OKAY` or `FAIL` plus a 4-hex-digit length
//!     and a message. Shelling out flashed a console window on Windows for every
//!     invocation.
//!   - **No `adb start-server` unless nothing is listening.** Running
//!     `start-server` against a server of a different version (Android Studio's,
//!     say) makes the newer binary kill the older server and replace it; the two
//!     then take turns killing each other. So the bundled adb is started only
//!     when connecting to the server's port fails, and any server already
//!     running is used as is.
//!   - **Every socket has a timeout,** so an unresponsive server or phone
//!     surfaces as an error instead of a hung UI.
//!   - **Device listing never fails as a whole.** Each line of `adb devices -l`
//!     is parsed on its own and unknown states are passed through, so one phone
//!     in an odd state (e.g. Linux's "no permissions") cannot hide the others.
//!   - **Every device request names its serial,** so two attached phones never
//!     get each other's commands. Wireless devices need nothing special: their
//!     serial is simply `host:port`.
//!
//! # Tunnel: direct service, no forward
//!
//! [`Transport::open_tunnel`] opens the device service `localabstract:<name>`
//! on a device-bound smart-socket connection (`host:transport:<serial>`, then
//! the service). After the server's `OKAY` that very TCP connection is a raw
//! byte pipe to the abstract socket. The alternative, `forward tcp:0
//! localabstract:<name>` plus a connect to the chosen port, adds a listening
//! host port that something has to clean up (a crashed host would leak it in
//! the long-lived adb server) and a window in which another local process can
//! connect to it. The direct service has neither, so it is the only approach
//! implemented. The forward helpers ([`AdbTransport::forward_any`],
//! [`AdbTransport::forward_remove`]) exist for diagnostics.
//!
//! # Pull
//!
//! Uses sync `RCV2` (adb's RECV2) when the device advertises the `sendrecv_v2`
//! feature, else v1 `RECV`. Both answer with `DATA` chunks then `DONE`, or
//! `FAIL`. Bytes go to `<local>.part`, which is renamed over `local` on success
//! and deleted on failure. The mtime is restored from a `STAT` made first on
//! the same sync connection, which also turns a missing file into a clear
//! error before any data flows.

mod devices;
mod errors;
mod server;
mod transport;
mod wire;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::time::Duration;

use socketsweep_transport::{Result, Transport};

/// The adb server's own port, not the daemon tunnel.
const ADB_SERVER_PORT: u16 = 5037;

/// The server is on this machine, so a connection either succeeds at once or
/// nothing is listening.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Read/write timeout for host requests and shell commands: the longest the
/// server or phone may go silent before the request is abandoned.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Pushing writes to flash, and the final acknowledgement waits for the file to
/// be closed on the phone, so it gets more room than an ordinary request.
const PUSH_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the phone may go silent while a pull streams.
const PULL_TIMEOUT: Duration = Duration::from_secs(30);

/// `host:connect` may wait for a TCP connection to a phone to time out.
const CONNECT_DEVICE_TIMEOUT: Duration = Duration::from_secs(15);

/// How long `adb start-server` may take to bring a fresh server up.
const START_SERVER_TIMEOUT: Duration = Duration::from_secs(20);

/// The adb [`Transport`].
pub struct AdbTransport {
    /// The bundled adb, run only to start a server when none is listening.
    adb_binary: PathBuf,
    server: SocketAddr,
}

impl AdbTransport {
    /// Connect to the adb server, starting it from the bundled binary only if
    /// nothing is listening on its port.
    pub fn connect(adb_binary: &Path) -> Result<Self> {
        let adb = Self::with_server(
            adb_binary,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, ADB_SERVER_PORT).into(),
        );
        // Probe once so a dead server surfaces here rather than at first use.
        adb.devices()?;
        Ok(adb)
    }

    /// A transport for an adb server at `server`, without probing it.
    pub fn with_server(adb_binary: &Path, server: SocketAddr) -> Self {
        AdbTransport {
            adb_binary: adb_binary.to_path_buf(),
            server,
        }
    }
}

#[cfg(test)]
mod tests;

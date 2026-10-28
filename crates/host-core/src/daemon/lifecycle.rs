//! Pushing, launching, connecting to and tearing down the daemon.

use std::path::Path;
use std::time::Instant;

use rand::Rng;
use socketsweep_transport::{DeviceInfo, Transport, TransportError};
use socketsweep_wire::Caps;

use super::random::{random_session, random_socket_name, random_token};
use super::{
    check_root, launch_command, Daemon, DaemonBinaries, LaunchError, LaunchOptions, DEVICE_BIN,
    DEVICE_LOG, GRANT_STORAGE, KILL_DAEMON, LOG_EXCERPT_BYTES,
};
use crate::client::Client;

/// Kill the daemon and remove its binary. Best effort: the phone may be gone.
pub fn tear_down(transport: &dyn Transport, serial: &str) {
    let _ = transport.shell(serial, KILL_DAEMON);
    let _ = transport.shell(serial, &format!("rm -f {DEVICE_BIN}"));
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
        push_daemon(transport, serial, binary)?;

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

/// Push the daemon build at `binary` to the phone. A `.gz` build (how the app
/// bundles it, so installer tooling does not scan an Android ELF) is
/// decompressed to a temporary file first; any other path is pushed as is.
pub(super) fn push_daemon(
    transport: &dyn Transport,
    serial: &str,
    binary: &Path,
) -> Result<(), TransportError> {
    if binary.extension().and_then(|e| e.to_str()) != Some("gz") {
        return transport.push(serial, binary, DEVICE_BIN, 0o755);
    }
    let local = |source| TransportError::Local {
        path: binary.to_path_buf(),
        source,
    };
    let tmp = std::env::temp_dir().join(format!(
        "socketsweep-daemon-{}-{:016x}",
        std::process::id(),
        rand::rng().random::<u64>()
    ));
    let unpack = || -> std::io::Result<()> {
        let mut gz = flate2::read::GzDecoder::new(std::fs::File::open(binary)?);
        let mut out = std::fs::File::create(&tmp)?;
        std::io::copy(&mut gz, &mut out)?;
        Ok(())
    };
    let result = unpack().map_err(local);
    let result = result.and_then(|()| transport.push(serial, &tmp, DEVICE_BIN, 0o755));
    let _ = std::fs::remove_file(&tmp);
    result
}

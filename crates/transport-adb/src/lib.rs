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
//! [`AdbTransport::forward_remove`]) stay available for diagnostics.
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
mod wire;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use socketsweep_transport::{
    Device, DeviceInfo, DeviceState, Duplex, Output, Result, Transport, TransportError,
};

use devices::{parse_device_info, parse_devices_long, DeviceLine};
use wire::{
    read_forward_reply, read_hex_string, read_shell_v2, request, send, sync_header, sync_recv,
    sync_send, RecvError, WireError, SHELL_CLOSE_STDIN,
};

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
    /// Connect to the adb server, starting it from our bundled binary only if
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

    /// `adb connect <addr>`: attach a wireless device (`host:port`). Afterwards
    /// the device is listed with that address as its serial.
    pub fn connect_device(&self, addr: &str) -> Result<()> {
        let mut stream = self.open(CONNECT_DEVICE_TIMEOUT)?;
        let msg = request(&mut stream, &format!("host:connect:{addr}"))
            .and_then(|()| read_hex_string(&mut stream))
            .map_err(|e| wire_to_error(e, &format!("Cannot connect to {addr}")))?;
        if msg.starts_with("connected to") || msg.starts_with("already connected to") {
            Ok(())
        } else {
            Err(TransportError::Refused(msg.trim().to_string()))
        }
    }

    /// `adb forward tcp:0 <remote>`: forward a host port of the server's
    /// choosing to `remote` (e.g. `localabstract:name`), returning that port.
    /// Not used by [`Transport::open_tunnel`]; see the crate docs.
    pub fn forward_any(&self, serial: &str, remote: &str) -> Result<u16> {
        self.forward(serial, "tcp:0", remote)?.ok_or_else(|| {
            TransportError::Protocol(format!(
                "forwarding to {remote} succeeded but the ADB server did not say which port it chose"
            ))
        })
    }

    /// Remove the forward listening on host port `local_port`. Best effort: the
    /// forward may already be gone with the device or the server.
    pub fn forward_remove(&self, serial: &str, local_port: u16) {
        let service = format!("host-serial:{serial}:killforward:tcp:{local_port}");
        if let Ok(mut stream) = self.open(IO_TIMEOUT) {
            let _ =
                send(&mut stream, &service).and_then(|()| read_forward_reply(&mut stream, false));
        }
    }

    fn forward(&self, serial: &str, local: &str, remote: &str) -> Result<Option<u16>> {
        let ctx = format!("Cannot forward {local} to {remote} on {serial}");
        let mut stream = self.open(IO_TIMEOUT)?;
        send(
            &mut stream,
            &format!("host-serial:{serial}:forward:{local};{remote}"),
        )
        .map_err(|e| wire_to_error(e, &ctx))?;
        read_forward_reply(&mut stream, local == "tcp:0").map_err(|e| wire_to_error(e, &ctx))
    }

    /// A host request answered by one length-prefixed string.
    fn host_query(&self, service: &str) -> Result<String> {
        let mut stream = self.open(IO_TIMEOUT)?;
        request(&mut stream, service)
            .and_then(|()| read_hex_string(&mut stream))
            .map_err(|e| match e {
                WireError::Fail(msg) => host_fail(None, &msg),
                e => TransportError::Server(format!("{service}: {e}")),
            })
    }

    /// Whether the phone speaks sync v2 (`RCV2`). A failed query means v1.
    fn has_sendrecv_v2(&self, serial: &str) -> bool {
        self.host_query(&format!("host-serial:{serial}:features"))
            .map(|f| f.trim().split(',').any(|f| f == "sendrecv_v2"))
            .unwrap_or(false)
    }

    /// A connection bound to one device, ready for a device service request.
    fn transport(&self, serial: &str, timeout: Duration) -> Result<TcpStream> {
        let mut stream = self.open(timeout)?;
        request(&mut stream, &format!("host:transport:{serial}")).map_err(|e| match e {
            WireError::Fail(msg) => host_fail(Some(serial), &msg),
            e => TransportError::Server(e.to_string()),
        })?;
        Ok(stream)
    }

    /// A fresh connection to the adb server, starting the bundled one first if
    /// and only if nothing is listening.
    fn open(&self, timeout: Duration) -> Result<TcpStream> {
        let stream = match TcpStream::connect_timeout(&self.server, CONNECT_TIMEOUT) {
            Ok(stream) => stream,
            Err(_) => {
                self.start_server()?;
                TcpStream::connect_timeout(&self.server, CONNECT_TIMEOUT).map_err(|e| {
                    TransportError::Server(format!("started adb but cannot reach it: {e}"))
                })?
            }
        };
        let setup = || -> io::Result<()> {
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.set_nodelay(true)
        };
        setup().map_err(|e| TransportError::Server(e.to_string()))?;
        Ok(stream)
    }

    fn start_server(&self) -> Result<()> {
        let mut command = Command::new(&self.adb_binary);
        command
            .arg("start-server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|e| {
            TransportError::Server(format!("cannot run {}: {e}", self.adb_binary.display()))
        })?;
        let deadline = Instant::now() + START_SERVER_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(TransportError::Server(format!(
                        "adb start-server failed ({status})"
                    )))
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TransportError::Server("adb start-server timed out".into()));
                }
                Err(e) => return Err(TransportError::Server(format!("adb start-server: {e}"))),
            }
        }
    }
}

impl Transport for AdbTransport {
    fn devices(&self) -> Result<Vec<Device>> {
        let listing = self.host_query("host:devices-l")?;
        Ok(parse_devices_long(&listing)
            .into_iter()
            .map(DeviceLine::into_device)
            .collect())
    }

    fn device_info(&self, serial: &str) -> Result<DeviceInfo> {
        let out = self.shell(
            serial,
            "getprop ro.build.version.sdk; getprop ro.product.cpu.abi; getprop ro.product.model",
        )?;
        let text = out.stdout_text();
        parse_device_info(&text, serial).ok_or_else(|| {
            TransportError::Protocol(format!(
                "device {serial} did not report its Android version and CPU type (got {:?})",
                text.trim()
            ))
        })
    }

    fn shell(&self, serial: &str, command: &str) -> Result<Output> {
        let fail = |e: WireError| wire_to_error(e, &format!("adb shell {command}"));

        // Shell protocol v2 (Android 7+) carries stdout, stderr and the exit
        // code separately. Older phones reject it, and fall back to the raw
        // stream, which carries stdout and stderr mixed and no exit code.
        let mut stream = self.transport(serial, IO_TIMEOUT)?;
        match request(&mut stream, &format!("shell,v2,raw:{command}")) {
            Ok(()) => {
                // Nothing is ever sent on stdin; closing it stops a command
                // that reads stdin from waiting on us.
                stream
                    .write_all(&[SHELL_CLOSE_STDIN, 0, 0, 0, 0])
                    .map_err(|e| fail(e.into()))?;
                let out = read_shell_v2(&mut stream).map_err(fail)?;
                Ok(Output {
                    stdout: out.stdout,
                    stderr: out.stderr,
                    exit: out.exit.map(i32::from),
                })
            }
            Err(WireError::Fail(_)) => {
                let mut stream = self.transport(serial, IO_TIMEOUT)?;
                request(&mut stream, &format!("shell:{command}")).map_err(fail)?;
                let mut stdout = Vec::new();
                stream
                    .read_to_end(&mut stdout)
                    .map_err(|e| fail(e.into()))?;
                Ok(Output {
                    stdout,
                    stderr: Vec::new(),
                    exit: None,
                })
            }
            Err(e) => Err(fail(e)),
        }
    }

    fn push(&self, serial: &str, local: &Path, remote: &str, mode: u32) -> Result<()> {
        let mut file = File::open(local).map_err(|source| TransportError::Local {
            path: local.to_path_buf(),
            source,
        })?;
        let fail = |e: WireError| wire_to_error(e, &format!("Cannot push to {remote}"));

        let mut stream = self.transport(serial, PUSH_TIMEOUT)?;
        request(&mut stream, "sync:").map_err(fail)?;
        sync_send(&mut stream, &mut file, remote, mode).map_err(fail)?;
        // Ends the sync session cleanly; the file is already on the phone.
        let _ = stream.write_all(&sync_header(b"QUIT", 0));
        Ok(())
    }

    fn pull(
        &self,
        serial: &str,
        remote: &[u8],
        local: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<u64> {
        let shown = String::from_utf8_lossy(remote).into_owned();
        let v2 = self.has_sendrecv_v2(serial);
        let mut stream = self.transport(serial, PULL_TIMEOUT)?;
        request(&mut stream, "sync:")
            .map_err(|e| wire_to_error(e, &format!("Cannot pull {shown}")))?;

        let local_err = |source: io::Error| TransportError::Local {
            path: local.to_path_buf(),
            source,
        };
        let mut part = local.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let mut file = File::create(&part).map_err(local_err)?;

        let result = sync_recv(&mut stream, remote, v2, &mut file, progress)
            .map_err(|e| match e {
                RecvError::Wire(w) => wire_to_error(w, &format!("Cannot pull {shown}")),
                RecvError::Sink(source) => local_err(source),
            })
            .and_then(|(n, mtime)| file.flush().map(|()| (n, mtime)).map_err(local_err));
        drop(file);
        match result {
            Ok((n, mtime)) => {
                if let Err(e) = fs::rename(&part, local) {
                    let _ = fs::remove_file(&part);
                    return Err(local_err(e));
                }
                if mtime > 0 {
                    // Best effort: a copy with the wrong mtime is still a copy.
                    if let Ok(f) = File::options().write(true).open(local) {
                        let _ = f.set_modified(UNIX_EPOCH + Duration::from_secs(u64::from(mtime)));
                    }
                }
                let _ = stream.write_all(&sync_header(b"QUIT", 0));
                Ok(n)
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                Err(e)
            }
        }
    }

    fn open_tunnel(&self, serial: &str, name: &str) -> Result<Box<dyn Duplex>> {
        let mut stream = self.transport(serial, IO_TIMEOUT)?;
        request(&mut stream, &format!("localabstract:{name}"))
            .map_err(|e| wire_to_error(e, &format!("Cannot reach the daemon socket {name}")))?;
        // From here the connection is the daemon's byte stream, and the caller
        // owns its timeouts: blocking until told otherwise.
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        Ok(Box::new(stream))
    }
}

/// A host-service `FAIL`, as the transport's error. adb words these as
/// "device 'X' not found", "device unauthorized…", "device offline".
fn host_fail(serial: Option<&str>, msg: &str) -> TransportError {
    let lower = msg.to_ascii_lowercase();
    match serial {
        Some(serial) if lower.contains("not found") => TransportError::NotFound {
            serial: serial.to_string(),
        },
        Some(serial) if lower.contains("unauthorized") => TransportError::Unusable {
            serial: serial.to_string(),
            state: DeviceState::Unauthorized,
            hint: None,
        },
        Some(serial) if lower.contains("offline") => TransportError::Unusable {
            serial: serial.to_string(),
            state: DeviceState::Offline,
            hint: None,
        },
        _ => TransportError::Refused(msg.to_string()),
    }
}

/// A wire failure of some request, with what was being attempted.
fn wire_to_error(e: WireError, what: &str) -> TransportError {
    match e {
        WireError::Fail(msg) => TransportError::Refused(format!("{what}: {msg}")),
        WireError::Bad(msg) => TransportError::Protocol(format!("{what}: {msg}")),
        WireError::Io(e) => match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                TransportError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{what}: {}", WireError::Io(e)),
                ))
            }
            _ => TransportError::Io(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn host_failures_map_to_the_transport_errors() {
        assert!(matches!(
            host_fail(Some("R5"), "device 'R5' not found"),
            TransportError::NotFound { serial } if serial == "R5"
        ));
        assert!(matches!(
            host_fail(Some("R5"), "device unauthorized."),
            TransportError::Unusable {
                state: DeviceState::Unauthorized,
                ..
            }
        ));
        assert!(matches!(
            host_fail(Some("R5"), "device offline"),
            TransportError::Unusable {
                state: DeviceState::Offline,
                ..
            }
        ));
        assert!(matches!(host_fail(None, "boom"), TransportError::Refused(m) if m == "boom"));
    }

    fn read_request(s: &mut TcpStream) -> String {
        let mut len = [0u8; 4];
        s.read_exact(&mut len).unwrap();
        let n = usize::from_str_radix(std::str::from_utf8(&len).unwrap(), 16).unwrap();
        let mut body = vec![0u8; n];
        s.read_exact(&mut body).unwrap();
        String::from_utf8(body).unwrap()
    }

    /// A fake adb server on loopback: three connections, each answering
    /// `host:transport:` and one service. `localabstract:daemon` then echoes.
    fn fake_server() -> (SocketAddr, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut s, _) = listener.accept().unwrap();
                let first = read_request(&mut s);
                if first == "host:transport:gone" {
                    s.write_all(b"FAIL0017device 'gone' not found").unwrap();
                    continue;
                }
                assert!(first.starts_with("host:transport:"));
                s.write_all(b"OKAY").unwrap();
                if read_request(&mut s) == "localabstract:daemon" {
                    s.write_all(b"OKAY").unwrap();
                    let mut buf = [0u8; 4];
                    s.read_exact(&mut buf).unwrap();
                    s.write_all(&buf).unwrap();
                    let _ = s.read(&mut buf);
                } else {
                    s.write_all(b"FAIL0006closed").unwrap();
                }
            }
        });
        (addr, handle)
    }

    #[test]
    fn open_tunnel_is_a_direct_localabstract_service() {
        let (addr, server) = fake_server();
        let adb = AdbTransport::with_server(Path::new("adb-not-needed"), addr);

        let tunnel = adb.open_tunnel("R5CX1234", "daemon").unwrap();
        let mut writer = tunnel.try_clone().unwrap();
        let mut reader = tunnel;
        writer.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        writer.shutdown().unwrap();
        assert_eq!(reader.read(&mut buf).unwrap(), 0);

        // A name nothing listens on is refused with the server's reason.
        match adb.open_tunnel("R5CX1234", "nothing") {
            Err(TransportError::Refused(m)) => assert!(m.contains("closed"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
        // A vanished device is NotFound.
        assert!(matches!(
            adb.open_tunnel("gone", "daemon"),
            Err(TransportError::NotFound { serial }) if serial == "gone"
        ));
        server.join().unwrap();
    }
}

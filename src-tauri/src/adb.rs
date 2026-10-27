//! ADB access, spoken directly to the adb server over its TCP socket.
//!
//! The adb *server* is still required: on Windows and macOS it owns the USB
//! claim. Going driverless would mean implementing ADB's RSA auth handshake and
//! USB transport, which is the line scrcpy chose not to cross either. What this
//! module avoids is everything around it:
//!
//!   - **No subprocess per call.** Every request is a short-lived connection to
//!     127.0.0.1:5037 using adb's "smart socket" protocol: a 4-hex-digit length
//!     and the request, answered by `OKAY` or `FAIL` plus a 4-hex-digit length
//!     and a message. Shelling out flashed a console window on Windows for every
//!     invocation.
//!   - **No `adb start-server` unless nothing is listening.** Running
//!     `start-server` against a server of a different version (Android Studio's,
//!     say) makes the newer binary kill the older server and replace it; the two
//!     then take turns killing each other and our port forward disappears with
//!     whichever loses. So the bundled adb is started only when connecting to
//!     the server's port fails, and any server already running is used as is.
//!   - **Every socket has a timeout,** so an unresponsive server or phone
//!     surfaces as an error instead of a hung UI.
//!   - **Device listing never fails as a whole.** Each line of `adb devices -l`
//!     is parsed on its own and unknown states are passed through, so one phone
//!     in an odd state (e.g. Linux's "no permissions") cannot hide the others.
//!   - **Every device request names its serial,** so two attached phones never
//!     get each other's commands.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

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

/// How long `adb start-server` may take to bring a fresh server up.
const START_SERVER_TIMEOUT: Duration = Duration::from_secs(20);

/// Largest `DATA` chunk the sync protocol accepts.
const SYNC_CHUNK: usize = 64 * 1024;

/// Upper bound on a single shell-protocol packet, so a corrupt length cannot
/// make us allocate gigabytes.
const MAX_PACKET: usize = 16 * 1024 * 1024;

/// Shown for devices Linux will not let adb open.
const UDEV_HINT: &str = "Linux is blocking access to this USB device. Add a udev rule for it \
     (see https://developer.android.com/studio/run/device), make sure your user is in the \
     plugdev group, then unplug and reconnect the phone.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// Human-readable model, for the picker. Falls back to the serial.
    pub model: String,
    /// adb's state, lowercased: "device", "unauthorized", "offline", "noperm",
    /// … Unrecognised states are passed through as adb reported them.
    pub state: String,
    /// True only for `device`; `unauthorized` and `offline` are listed so the UI
    /// can explain what to do rather than reporting nothing found.
    pub usable: bool,
    /// What the user should do about a device that is not usable, when we know.
    pub hint: Option<String>,
}

/// Facts about the phone that decide which daemon build it can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Android API level, from `ro.build.version.sdk`.
    pub sdk: u32,
    /// Primary ABI, from `ro.product.cpu.abi`, e.g. "arm64-v8a".
    pub abi: String,
}

#[derive(Debug)]
pub enum AdbError {
    /// The adb server could not be reached or started.
    Server(String),
    NoDevices,
    /// Attached but the user has not accepted the debugging prompt.
    Unauthorized(String),
    Device(String),
}

impl fmt::Display for AdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdbError::Server(e) => write!(f, "Cannot talk to the ADB server: {e}"),
            AdbError::NoDevices => write!(
                f,
                "No Android device detected. Connect your phone over USB and enable USB Debugging."
            ),
            AdbError::Unauthorized(serial) => write!(
                f,
                "Device {serial} is unauthorized. Accept the \u{201c}Allow USB debugging\u{201d} prompt on the phone."
            ),
            AdbError::Device(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AdbError {}

pub type Result<T> = std::result::Result<T, AdbError>;

pub struct Adb {
    /// The bundled adb, run only to start a server when none is listening.
    adb_binary: PathBuf,
    server: SocketAddr,
}

impl Adb {
    /// Connect to the adb server, starting it from our bundled binary only if
    /// nothing is listening on its port.
    pub fn connect(adb_binary: &Path) -> Result<Self> {
        let adb = Adb {
            adb_binary: adb_binary.to_path_buf(),
            server: SocketAddrV4::new(Ipv4Addr::LOCALHOST, ADB_SERVER_PORT).into(),
        };
        // Probe once so a dead server surfaces here rather than at first use.
        adb.devices()?;
        Ok(adb)
    }

    /// Every attached device, including ones that are not usable yet, so the UI
    /// can distinguish "nothing plugged in" from "you need to tap Allow".
    pub fn devices(&self) -> Result<Vec<Device>> {
        let listing = self.host_query("host:devices-l")?;
        Ok(parse_devices_long(&listing)
            .into_iter()
            .map(DeviceLine::into_device)
            .collect())
    }

    /// Pick the device to work with, erroring in a way that tells the user what
    /// to do about it.
    pub fn resolve(&self, preferred: Option<&str>) -> Result<String> {
        let devices = self.devices()?;
        if devices.is_empty() {
            return Err(AdbError::NoDevices);
        }

        if let Some(serial) = preferred {
            let device = devices.iter().find(|d| d.serial == serial).ok_or_else(|| {
                AdbError::Device(format!("Device {serial} is no longer connected."))
            })?;
            return if device.usable {
                Ok(device.serial.clone())
            } else {
                Err(not_usable(device))
            };
        }

        if let Some(d) = devices.iter().find(|d| d.usable) {
            return Ok(d.serial.clone());
        }

        // Nothing usable: say which specific problem it is, preferring the one
        // the user can fix with a tap.
        let device = devices
            .iter()
            .find(|d| d.state == "unauthorized")
            .unwrap_or(&devices[0]);
        Err(not_usable(device))
    }

    /// Run a shell command, returning its stdout.
    ///
    /// The command is handed to the phone's `sh` verbatim. Anything interpolated
    /// into it — a path the user chose, for instance — must already be quoted
    /// for the shell by the caller.
    pub fn shell(&self, serial: &str, command: &str) -> Result<String> {
        let fail = |e: WireError| AdbError::Device(format!("adb shell {command}: {e}"));

        // Shell protocol v2 (Android 7+) carries stdout, stderr and the exit
        // code separately. Older phones reject it, and fall back to the raw
        // stream, which carries stdout and stderr mixed and no exit code.
        let mut stream = self.transport(serial, IO_TIMEOUT)?;
        let output = match request(&mut stream, &format!("shell,v2,raw:{command}")) {
            Ok(()) => {
                // Nothing is ever sent on stdin; closing it stops a command
                // that reads stdin from waiting on us.
                stream
                    .write_all(&[SHELL_CLOSE_STDIN, 0, 0, 0, 0])
                    .map_err(|e| fail(e.into()))?;
                read_shell_v2(&mut stream).map_err(fail)?
            }
            Err(WireError::Fail(_)) => {
                let mut stream = self.transport(serial, IO_TIMEOUT)?;
                request(&mut stream, &format!("shell:{command}")).map_err(fail)?;
                let mut stdout = Vec::new();
                stream
                    .read_to_end(&mut stdout)
                    .map_err(|e| fail(e.into()))?;
                ShellOutput {
                    stdout,
                    stderr: Vec::new(),
                    exit: None,
                }
            }
            Err(e) => return Err(fail(e)),
        };

        if let Some(code) = output.exit.filter(|&c| c != 0) {
            return Err(AdbError::Device(format!(
                "adb shell {command} exited {code}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Best-effort shell command, for cleanup where failure is expected and
    /// uninteresting — killing a daemon that is not running, for instance.
    pub fn shell_ignoring_errors(&self, serial: &str, command: &str) {
        let _ = self.shell(serial, command);
    }

    /// API level and primary ABI of the phone.
    pub fn device_info(&self, serial: &str) -> Result<DeviceInfo> {
        let out = self.shell(
            serial,
            "getprop ro.build.version.sdk; getprop ro.product.cpu.abi",
        )?;
        parse_device_info(&out).ok_or_else(|| {
            AdbError::Device(format!(
                "Device {serial} did not report its Android version and CPU type (got {:?}).",
                out.trim()
            ))
        })
    }

    /// Copy a local file to `remote` on the phone, mode 0755, over the sync
    /// protocol.
    pub fn push(&self, serial: &str, local: &Path, remote: &str) -> Result<()> {
        let mut file = std::fs::File::open(local)
            .map_err(|e| AdbError::Device(format!("Cannot read {}: {e}", local.display())))?;
        let fail = |e: WireError| AdbError::Device(format!("Cannot push to {remote}: {e}"));

        let mut stream = self.transport(serial, PUSH_TIMEOUT)?;
        request(&mut stream, "sync:").map_err(fail)?;
        sync_send(&mut stream, &mut file, remote, 0o755).map_err(fail)?;
        // Ends the sync session cleanly; the file is already on the phone.
        let _ = stream.write_all(&sync_header(b"QUIT", 0));
        Ok(())
    }

    /// `adb forward tcp:<local_port> localabstract:<name>`.
    pub fn forward_abstract(&self, serial: &str, local_port: u16, name: &str) -> Result<()> {
        self.forward(
            serial,
            &format!("tcp:{local_port}"),
            &format!("localabstract:{name}"),
        )
        .map(|_| ())
    }

    /// `adb forward tcp:0 <remote>`: forward a host port of the server's
    /// choosing to `remote` (e.g. `localabstract:name`), returning that port.
    /// Nothing else can already hold the port, unlike a fixed one.
    pub fn forward_any(&self, serial: &str, remote: &str) -> Result<u16> {
        self.forward(serial, "tcp:0", remote)?.ok_or_else(|| {
            AdbError::Device(format!(
                "Forwarding to {remote} succeeded but the ADB server did not say which port it chose."
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
        let fail = |e: WireError| {
            AdbError::Device(format!(
                "Cannot forward {local} to {remote} on {serial}: {e}"
            ))
        };
        let mut stream = self.open(IO_TIMEOUT)?;
        send(
            &mut stream,
            &format!("host-serial:{serial}:forward:{local};{remote}"),
        )
        .map_err(fail)?;
        read_forward_reply(&mut stream, local == "tcp:0").map_err(fail)
    }

    /// A host request answered by one length-prefixed string.
    fn host_query(&self, service: &str) -> Result<String> {
        let mut stream = self.open(IO_TIMEOUT)?;
        request(&mut stream, service)
            .and_then(|()| read_hex_string(&mut stream))
            .map_err(|e| AdbError::Server(format!("{service}: {e}")))
    }

    /// A connection bound to one device, ready for a device service request.
    fn transport(&self, serial: &str, timeout: Duration) -> Result<TcpStream> {
        let mut stream = self.open(timeout)?;
        request(&mut stream, &format!("host:transport:{serial}")).map_err(|e| match e {
            WireError::Fail(msg) => AdbError::Device(format!("Cannot open device {serial}: {msg}")),
            e => AdbError::Server(e.to_string()),
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
                    AdbError::Server(format!("started adb but cannot reach it: {e}"))
                })?
            }
        };
        let setup = || -> io::Result<()> {
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.set_nodelay(true)
        };
        setup().map_err(|e| AdbError::Server(e.to_string()))?;
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
            AdbError::Server(format!("cannot run {}: {e}", self.adb_binary.display()))
        })?;
        let deadline = Instant::now() + START_SERVER_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(AdbError::Server(format!(
                        "adb start-server failed ({status})"
                    )))
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AdbError::Server("adb start-server timed out".into()));
                }
                Err(e) => return Err(AdbError::Server(format!("adb start-server: {e}"))),
            }
        }
    }
}

/// The error for a device the user picked, or the only one attached, that is
/// not in a state we can use.
fn not_usable(device: &Device) -> AdbError {
    let serial = &device.serial;
    match device.state.as_str() {
        "unauthorized" => AdbError::Unauthorized(serial.clone()),
        "noperm" => AdbError::Device(format!("Device {serial} cannot be opened. {UDEV_HINT}")),
        "offline" => AdbError::Device(format!(
            "Device {serial} is offline. Unplug and reconnect it."
        )),
        "authorizing" => AdbError::Device(format!(
            "Device {serial} is still authorising this computer. Wait a moment and try again."
        )),
        state => AdbError::Device(format!(
            "Device {serial} is {state}, not ready for USB debugging. Unplug and reconnect it."
        )),
    }
}

// ── devices -l ──────────────────────────────────────────────────────────────

/// One line of `adb devices -l`, before it is dressed up for the UI.
#[derive(Debug, Default, PartialEq, Eq)]
struct DeviceLine {
    serial: String,
    /// As adb printed it, possibly several words long.
    state: String,
    model: Option<String>,
    // Not shown in the UI today, but part of what identifies a device in a
    // bug report, so they are parsed rather than discarded.
    #[allow(dead_code)]
    product: Option<String>,
    #[allow(dead_code)]
    device: Option<String>,
    #[allow(dead_code)]
    transport_id: Option<u32>,
}

impl DeviceLine {
    fn into_device(self) -> Device {
        let state = normalise_state(&self.state);
        // adb reports models as e.g. "SM_S928B"; underscores read badly in a
        // device picker. "Unk" is a placeholder some tools use for a missing
        // model.
        let model = match self.model.as_deref() {
            Some(m) if !m.is_empty() && m != "Unk" => m.replace('_', " "),
            _ => self.serial.clone(),
        };
        let hint = match state.as_str() {
            "unauthorized" => {
                Some("Accept the \u{201c}Allow USB debugging\u{201d} prompt on the phone.".into())
            }
            "offline" => Some("Unplug and reconnect the cable.".into()),
            "authorizing" => Some("Waiting for the phone to authorise this computer.".into()),
            "noperm" => Some(UDEV_HINT.into()),
            _ => None,
        };
        Device {
            usable: state == "device",
            serial: self.serial,
            model,
            state,
            hint,
        }
    }
}

/// adb's states are single lowercase words except Linux's permission failure,
/// which is a sentence: "no permissions (missing udev rules? …); see [url]".
fn normalise_state(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("no permissions") || lower == "noperm" {
        "noperm".into()
    } else {
        lower
    }
}

/// Parse the body of `host:devices-l`. Lines that cannot be read are skipped
/// rather than failing the listing.
fn parse_devices_long(text: &str) -> Vec<DeviceLine> {
    text.lines().filter_map(parse_device_line).collect()
}

/// `<serial> <state…> [key:value …]`, e.g.
/// `R5CX1234 device usb:1-1 product:e3qxeea model:SM_S928B device:e3q transport_id:3`.
fn parse_device_line(line: &str) -> Option<DeviceLine> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("List of devices") || line.starts_with('*') {
        return None;
    }
    let mut tokens = line.split_whitespace();
    let serial = tokens.next()?.to_string();
    let rest: Vec<&str> = tokens.collect();

    // The state runs up to the first key:value field. A multi-word state may
    // contain colons ("see [http://…]"), but never a bare `word:value` token.
    let fields_at = rest
        .iter()
        .position(|t| as_field(t).is_some())
        .unwrap_or(rest.len());
    let state = rest[..fields_at].join(" ");
    if state.is_empty() {
        return None;
    }

    let mut parsed = DeviceLine {
        serial,
        state,
        ..DeviceLine::default()
    };
    for (key, value) in rest[fields_at..].iter().filter_map(|t| as_field(t)) {
        match key {
            "model" => parsed.model = Some(value.to_string()),
            "product" => parsed.product = Some(value.to_string()),
            "device" => parsed.device = Some(value.to_string()),
            "transport_id" => parsed.transport_id = value.parse().ok(),
            _ => {}
        }
    }
    Some(parsed)
}

/// `key:value` where the key is a plain identifier and the value is non-empty.
fn as_field(token: &str) -> Option<(&str, &str)> {
    let (key, value) = token.split_once(':')?;
    let plain = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    (plain && !value.is_empty()).then_some((key, value))
}

fn parse_device_info(getprop: &str) -> Option<DeviceInfo> {
    let mut lines = getprop.lines().map(str::trim);
    let sdk = lines.next()?.parse().ok()?;
    let abi = lines.next()?.to_string();
    (!abi.is_empty()).then_some(DeviceInfo { sdk, abi })
}

// ── Wire protocol ───────────────────────────────────────────────────────────

#[derive(Debug)]
enum WireError {
    Io(io::Error),
    /// The server or phone refused the request, with its reason.
    Fail(String),
    /// A reply that does not follow the protocol.
    Bad(String),
}

impl From<io::Error> for WireError {
    fn from(e: io::Error) -> Self {
        WireError::Io(e)
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Io(e) => match e.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                    write!(
                        f,
                        "timed out after {}s without a reply",
                        IO_TIMEOUT.as_secs()
                    )
                }
                io::ErrorKind::UnexpectedEof => write!(f, "the connection closed early"),
                _ => write!(f, "{e}"),
            },
            WireError::Fail(msg) => write!(f, "{msg}"),
            WireError::Bad(msg) => write!(f, "unexpected reply: {msg}"),
        }
    }
}

type Wire<T> = std::result::Result<T, WireError>;

/// Send a smart-socket request: 4 hex digits of length, then the request.
fn send<W: Write>(w: &mut W, service: &str) -> Wire<()> {
    if service.len() > 0xffff {
        return Err(WireError::Bad(format!(
            "request too long ({} bytes)",
            service.len()
        )));
    }
    w.write_all(format!("{:04x}{service}", service.len()).as_bytes())?;
    Ok(())
}

/// `OKAY`, or `FAIL` followed by a length-prefixed reason.
fn read_status<R: Read>(r: &mut R) -> Wire<()> {
    let mut status = [0u8; 4];
    r.read_exact(&mut status)?;
    match &status {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(WireError::Fail(read_hex_string(r)?)),
        other => Err(WireError::Bad(String::from_utf8_lossy(other).into_owned())),
    }
}

fn request<S: Read + Write>(s: &mut S, service: &str) -> Wire<()> {
    send(s, service)?;
    read_status(s)
}

/// 4 hex digits of length, then that many bytes.
fn read_hex_string<R: Read>(r: &mut R) -> Wire<String> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = std::str::from_utf8(&len)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .ok_or_else(|| WireError::Bad(format!("bad length {:?}", String::from_utf8_lossy(&len))))?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The reply to `host-serial:…:forward:` and `…:killforward:`. The server
/// answers `OKAY` once for accepting the request and again for the result, and
/// for `tcp:0` follows with the chosen port as a length-prefixed string. A
/// refusal is a `FAIL` in place of either `OKAY`.
fn read_forward_reply<R: Read>(r: &mut R, expect_port: bool) -> Wire<Option<u16>> {
    read_status(r)?;
    read_status(r)?;
    if !expect_port {
        return Ok(None);
    }
    let port = read_hex_string(r)?;
    match port.trim().parse::<u16>() {
        Ok(p) if p != 0 => Ok(Some(p)),
        _ => Err(WireError::Bad(format!("port {port:?}"))),
    }
}

const SHELL_STDOUT: u8 = 1;
const SHELL_STDERR: u8 = 2;
const SHELL_EXIT: u8 = 3;
const SHELL_CLOSE_STDIN: u8 = 4;

struct ShellOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// None when the connection closed without reporting one.
    exit: Option<u8>,
}

/// Shell protocol v2: packets of a 1-byte stream id, a little-endian u32
/// length and the payload, ending with an exit-code packet.
fn read_shell_v2<R: Read>(r: &mut R) -> Wire<ShellOutput> {
    let mut out = ShellOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        exit: None,
    };
    loop {
        let mut header = [0u8; 5];
        match r.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(out),
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_PACKET {
            return Err(WireError::Bad(format!("shell packet of {len} bytes")));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        match header[0] {
            SHELL_STDOUT => out.stdout.extend_from_slice(&payload),
            SHELL_STDERR => out.stderr.extend_from_slice(&payload),
            SHELL_EXIT => {
                out.exit = payload.first().copied();
                return Ok(out);
            }
            _ => {}
        }
    }
}

/// A sync-protocol header: 4-byte id and a little-endian u32.
fn sync_header(id: &[u8; 4], n: u32) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[..4].copy_from_slice(id);
    h[4..].copy_from_slice(&n.to_le_bytes());
    h
}

/// `SEND <path>,<mode>`, the file as `DATA` chunks, `DONE <mtime>`, then the
/// phone's `OKAY` or `FAIL <reason>`.
///
/// The mode goes out in decimal and includes the file-type bits (`S_IFREG`),
/// the same `"%s,%d"` form upstream adb's client sends, so 0o755 becomes
/// `33261` (0o100755).
fn sync_send<S: Read + Write>(
    s: &mut S,
    file: &mut impl Read,
    remote: &str,
    mode: u32,
) -> Wire<()> {
    const S_IFREG: u32 = 0o100000;
    let spec = format!("{remote},{}", S_IFREG | mode);
    s.write_all(&sync_header(b"SEND", spec.len() as u32))?;
    s.write_all(spec.as_bytes())?;

    let mut buf = vec![0u8; SYNC_CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        s.write_all(&sync_header(b"DATA", n as u32))?;
        s.write_all(&buf[..n])?;
    }

    let mtime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    s.write_all(&sync_header(b"DONE", mtime))?;
    read_sync_status(s)
}

fn read_sync_status<R: Read>(r: &mut R) -> Wire<()> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header)?;
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    match &header[..4] {
        b"OKAY" => Ok(()),
        b"FAIL" if len <= MAX_PACKET => {
            let mut msg = vec![0u8; len];
            r.read_exact(&mut msg)?;
            Err(WireError::Fail(String::from_utf8_lossy(&msg).into_owned()))
        }
        other => Err(WireError::Bad(String::from_utf8_lossy(other).into_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices(text: &str) -> Vec<Device> {
        parse_devices_long(text)
            .into_iter()
            .map(DeviceLine::into_device)
            .collect()
    }

    #[test]
    fn parses_a_ready_device_with_all_fields() {
        let lines = parse_devices_long(
            "R5CX1234ABC            device usb:1-1 product:e3qxeea model:SM_S928B device:e3q transport_id:3\n",
        );
        assert_eq!(
            lines,
            vec![DeviceLine {
                serial: "R5CX1234ABC".into(),
                state: "device".into(),
                model: Some("SM_S928B".into()),
                product: Some("e3qxeea".into()),
                device: Some("e3q".into()),
                transport_id: Some(3),
            }]
        );
        let d = &devices("R5CX1234ABC device usb:1-1 model:SM_S928B transport_id:3")[0];
        assert!(d.usable);
        assert_eq!(d.model, "SM S928B");
        assert_eq!(d.hint, None);
    }

    #[test]
    fn no_permissions_becomes_noperm_with_a_udev_hint() {
        let text = "0123456789ABCDEF       no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html] usb:1-4 transport_id:7\n\
                    R5CX1234ABC            device usb:1-1 model:Pixel_8 transport_id:3\n";
        let list = devices(text);
        assert_eq!(list.len(), 2, "one odd line must not hide the others");
        assert_eq!(list[0].serial, "0123456789ABCDEF");
        assert_eq!(list[0].state, "noperm");
        assert!(!list[0].usable);
        assert_eq!(list[0].model, "0123456789ABCDEF");
        assert!(list[0].hint.as_deref().unwrap().contains("udev"));
        assert_eq!(parse_devices_long(text)[0].transport_id, Some(7));
        assert!(list[1].usable);
        assert_eq!(list[1].model, "Pixel 8");
    }

    #[test]
    fn unauthorized_and_offline_are_listed_but_unusable() {
        let list = devices(
            "emulator-5554 offline transport_id:1\nR5CX1234ABC\tunauthorized usb:1-1 transport_id:2\n",
        );
        assert_eq!(list[0].state, "offline");
        assert!(!list[0].usable);
        assert_eq!(list[1].state, "unauthorized");
        assert!(!list[1].usable);
        assert_eq!(list[1].model, "R5CX1234ABC");
        assert!(list[1].hint.is_some());
    }

    #[test]
    fn placeholder_or_empty_model_falls_back_to_serial() {
        assert_eq!(
            devices("abc device model:Unk transport_id:1")[0].model,
            "abc"
        );
        // `model:` with no value is not a field at all.
        assert_eq!(devices("abc device model: transport_id:1")[0].model, "abc");
    }

    #[test]
    fn unknown_states_and_fields_pass_through() {
        let lines = parse_devices_long(
            "192.168.1.5:5555 sideload product:x model:Y device:z features:shell_v2,cmd transport_id:9\nonlyserial\n\n",
        );
        assert_eq!(lines.len(), 1, "a line with no state is skipped");
        assert_eq!(lines[0].serial, "192.168.1.5:5555");
        assert_eq!(lines[0].state, "sideload");
        assert_eq!(lines[0].transport_id, Some(9));
        let d = &devices("x Recovery transport_id:1")[0];
        assert_eq!(d.state, "recovery");
        assert!(!d.usable);
    }

    #[test]
    fn empty_listing_is_no_devices() {
        assert!(parse_devices_long("").is_empty());
    }

    #[test]
    fn forward_tcp0_reply_yields_the_port() {
        let mut reply: &[u8] = b"OKAYOKAY000538561";
        assert_eq!(read_forward_reply(&mut reply, true).unwrap(), Some(38561));
    }

    #[test]
    fn forward_fixed_port_reply_has_no_port() {
        let mut reply: &[u8] = b"OKAYOKAY";
        assert_eq!(read_forward_reply(&mut reply, false).unwrap(), None);
    }

    #[test]
    fn forward_failures_carry_the_servers_reason() {
        let mut reply: &[u8] = b"FAIL0014device 'X' not found";
        match read_forward_reply(&mut reply, true) {
            Err(WireError::Fail(msg)) => assert_eq!(msg, "device 'X' not found"),
            other => panic!("{other:?}"),
        }
        let mut reply: &[u8] = b"OKAYFAIL000bcannot bind";
        assert!(matches!(
            read_forward_reply(&mut reply, true),
            Err(WireError::Fail(m)) if m == "cannot bind"
        ));
    }

    #[test]
    fn forward_reply_without_a_valid_port_is_an_error() {
        let mut reply: &[u8] = b"OKAYOKAY";
        assert!(read_forward_reply(&mut reply, true).is_err());
        let mut reply: &[u8] = b"OKAYOKAY0001x";
        assert!(matches!(
            read_forward_reply(&mut reply, true),
            Err(WireError::Bad(_))
        ));
    }

    #[test]
    fn requests_are_length_prefixed_in_hex() {
        let mut out = Vec::new();
        send(&mut out, "host:devices-l").unwrap();
        assert_eq!(out, b"000ehost:devices-l");
    }

    #[test]
    fn shell_v2_separates_streams_and_reports_exit() {
        let mut wire = Vec::new();
        for (id, data) in [
            (1u8, &b"out"[..]),
            (2, b"err"),
            (1, b"put"),
            (3, &[2u8][..]),
        ] {
            wire.push(id);
            wire.extend_from_slice(&(data.len() as u32).to_le_bytes());
            wire.extend_from_slice(data);
        }
        let out = read_shell_v2(&mut wire.as_slice()).unwrap();
        assert_eq!(out.stdout, b"output");
        assert_eq!(out.stderr, b"err");
        assert_eq!(out.exit, Some(2));
    }

    #[test]
    fn sync_send_frames_the_file_and_reads_the_ack() {
        struct Duplex {
            written: Vec<u8>,
            reply: &'static [u8],
        }
        impl Read for Duplex {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.reply.read(buf)
            }
        }
        impl Write for Duplex {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.written.write(buf)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut s = Duplex {
            written: Vec::new(),
            reply: b"OKAY\0\0\0\0",
        };
        sync_send(&mut s, &mut &b"hello"[..], "/data/local/tmp/x", 0o755).unwrap();
        let w = &s.written;
        // "/data/local/tmp/x" (17) + "," + "33261" (0o100755) = 23 bytes.
        assert_eq!(&w[..8], &sync_header(b"SEND", 23));
        assert_eq!(&w[8..31], b"/data/local/tmp/x,33261");
        assert_eq!(&w[31..39], &sync_header(b"DATA", 5));
        assert_eq!(&w[39..44], b"hello");
        assert_eq!(&w[44..48], b"DONE");

        let mut s = Duplex {
            written: Vec::new(),
            reply: b"FAIL\x06\0\0\0no way",
        };
        assert!(matches!(
            sync_send(&mut s, &mut &b""[..], "/x", 0o755),
            Err(WireError::Fail(m)) if m == "no way"
        ));
    }

    #[test]
    fn device_info_reads_sdk_then_abi() {
        assert_eq!(
            parse_device_info("34\r\narm64-v8a\r\n"),
            Some(DeviceInfo {
                sdk: 34,
                abi: "arm64-v8a".into()
            })
        );
        assert_eq!(parse_device_info("\narm64-v8a\n"), None);
        assert_eq!(parse_device_info("34\n\n"), None);
    }
}

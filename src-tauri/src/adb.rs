//! ADB access, over the `adb_client` crate rather than a subprocess per call.
//!
//! What shelling out cost us:
//!
//!   - a console window flashing on Windows for every invocation, and connecting
//!     fired six of them
//!   - a hand-rolled 15-second timeout built from a 50ms poll loop and two
//!     reader threads that were leaked whenever the timeout fired
//!   - error classification by lowercasing stdout+stderr and looking for the
//!     substrings "no devices" and "unauthorized"
//!   - device detection via `line.contains("device")`, a substring test that
//!     also matched the "List of devices attached" header, and no `-s <serial>`
//!     on any later call, so two attached phones broke every command
//!
//! The adb *server* is still required: on Windows and macOS it owns the USB
//! claim. Going driverless would mean implementing ADB's RSA auth handshake and
//! USB transport, which is the line scrcpy chose not to cross either.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::Path;

use adb_client::server::{ADBServer, DeviceState};
use adb_client::server_device::ADBServerDevice;
use adb_client::ADBDeviceExt;
use serde::Serialize;

/// The adb server's own port, not the daemon tunnel.
const ADB_SERVER_PORT: u16 = 5037;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// Human-readable model, for the picker. Falls back to the serial.
    pub model: String,
    pub state: String,
    /// True only for `device`; `unauthorized` and `offline` are listed so the UI
    /// can explain what to do rather than reporting nothing found.
    pub usable: bool,
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

impl std::fmt::Display for AdbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdbError::Server(e) => write!(f, "Cannot talk to the ADB server: {e}"),
            AdbError::NoDevices => write!(
                f,
                "No Android device detected. Connect your phone over USB and enable USB Debugging."
            ),
            AdbError::Unauthorized(serial) => write!(
                f,
                "Device {serial} has not authorised this computer. Check the confirmation dialog on the phone."
            ),
            AdbError::Device(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AdbError {}

pub type Result<T> = std::result::Result<T, AdbError>;

pub struct Adb {
    server: ADBServer,
}

impl Adb {
    /// Connect to the adb server, starting it from our bundled binary if it is
    /// not already running.
    pub fn connect(adb_binary: &Path) -> Result<Self> {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, ADB_SERVER_PORT);
        let path = adb_binary.to_string_lossy().into_owned();

        let mut server = ADBServer::new_from_path(addr, Some(path));
        // Probe once so a dead server surfaces here rather than at first use.
        server
            .devices()
            .map_err(|e| AdbError::Server(e.to_string()))?;

        Ok(Adb { server })
    }

    /// Every attached device, including ones that are not usable yet, so the UI
    /// can distinguish "nothing plugged in" from "you need to tap Allow".
    pub fn devices(&mut self) -> Result<Vec<Device>> {
        let long = self
            .server
            .devices_long()
            .map_err(|e| AdbError::Server(e.to_string()))?;

        Ok(long
            .into_iter()
            .map(|d| {
                let serial = d.identifier.clone();
                // adb reports models as e.g. "SM_S928B"; underscores read badly
                // in a device picker.
                let model = if d.model.is_empty() {
                    serial.clone()
                } else {
                    d.model.replace('_', " ")
                };
                Device {
                    usable: d.state == DeviceState::Device,
                    state: d.state.to_string().to_lowercase(),
                    serial,
                    model,
                }
            })
            .collect())
    }

    /// Pick the device to work with, erroring in a way that tells the user what
    /// to do about it.
    pub fn resolve(&mut self, preferred: Option<&str>) -> Result<String> {
        let devices = self.devices()?;
        if devices.is_empty() {
            return Err(AdbError::NoDevices);
        }

        if let Some(serial) = preferred {
            return devices
                .iter()
                .find(|d| d.serial == serial)
                .map(|d| d.serial.clone())
                .ok_or_else(|| {
                    AdbError::Device(format!("Device {serial} is no longer connected."))
                });
        }

        if let Some(d) = devices.iter().find(|d| d.usable) {
            return Ok(d.serial.clone());
        }

        // Nothing usable: say which specific problem it is.
        match devices.iter().find(|d| d.state == "unauthorized") {
            Some(d) => Err(AdbError::Unauthorized(d.serial.clone())),
            None => Err(AdbError::Device(format!(
                "Device {} is {}. Unplug and reconnect it.",
                devices[0].serial, devices[0].state
            ))),
        }
    }

    fn device(&mut self, serial: &str) -> Result<ADBServerDevice> {
        self.server
            .get_device_by_name(serial)
            .map_err(|e| AdbError::Device(format!("Cannot open device {serial}: {e}")))
    }

    /// Run a shell command, returning its stdout.
    ///
    /// Callers pass fixed command strings built from constants, never from user
    /// input, so there is no quoting to get wrong here.
    pub fn shell(&mut self, serial: &str, command: &str) -> Result<String> {
        let mut device = self.device(serial)?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = device
            .shell_command(&command, Some(&mut stdout), Some(&mut stderr))
            .map_err(|e| AdbError::Device(format!("adb shell {command}: {e}")))?;

        if let Some(code) = code {
            if code != 0 {
                return Err(AdbError::Device(format!(
                    "adb shell {command} exited {code}: {}",
                    String::from_utf8_lossy(&stderr).trim()
                )));
            }
        }
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }

    /// Best-effort shell command, for cleanup where failure is expected and
    /// uninteresting — killing a daemon that is not running, for instance.
    pub fn shell_ignoring_errors(&mut self, serial: &str, command: &str) {
        let _ = self.shell(serial, command);
    }

    pub fn push(&mut self, serial: &str, local: &Path, remote: &str) -> Result<()> {
        let mut device = self.device(serial)?;
        let mut file = std::fs::File::open(local)
            .map_err(|e| AdbError::Device(format!("Cannot read {}: {e}", local.display())))?;
        device
            .push(&mut file, remote)
            .map_err(|e| AdbError::Device(format!("Cannot push to {remote}: {e}")))
    }

    /// `adb forward tcp:<local> localabstract:<name>`.
    pub fn forward_abstract(&mut self, serial: &str, local_port: u16, name: &str) -> Result<()> {
        let mut device = self.device(serial)?;
        device
            .forward(format!("tcp:{local_port}"), format!("localabstract:{name}"))
            .map_err(|e| {
                AdbError::Device(format!(
                    "Cannot forward tcp:{local_port} to localabstract:{name}: {e}"
                ))
            })
    }

    pub fn forward_remove(&mut self, serial: &str, local_port: u16) {
        if let Ok(mut device) = self.device(serial) {
            let _ = device.forward_remove(format!("tcp:{local_port}"));
        }
    }
}

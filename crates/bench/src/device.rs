//! Reaching the phone: the adb transport, the device, the daemon build.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use socketsweep_host_core::daemon::{Daemon, DEFAULT_ROOT};
use socketsweep_host_core::{DaemonBinaries, LaunchOptions};
use socketsweep_transport::Transport;
use socketsweep_transport_adb::AdbTransport;

/// Where `npm run build:daemon` and `npm run setup` put things, tried
/// relative to the current directory and to this crate's workspace.
fn bin_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from("src-tauri/bin"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/bin"),
    ]
}

fn find_bin(name: &str) -> Option<PathBuf> {
    bin_dirs()
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The adb transport. `adb` is the binary used to start a server if none
/// is running: the given one, the bundled one, or `adb` on `PATH`.
pub fn transport(adb: Option<PathBuf>) -> Result<Arc<AdbTransport>, String> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let adb = adb
        .or_else(|| find_bin(exe))
        .unwrap_or_else(|| PathBuf::from(exe));
    AdbTransport::connect(&adb)
        .map(Arc::new)
        .map_err(|e| format!("adb ({}): {e}", adb.display()))
}

/// `serial`, or the only usable device.
pub fn serial(transport: &dyn Transport, serial: Option<String>) -> Result<String, String> {
    if let Some(s) = serial {
        return Ok(s);
    }
    let devices = transport.devices().map_err(|e| e.to_string())?;
    let usable: Vec<_> = devices.iter().filter(|d| d.usable()).collect();
    match usable.as_slice() {
        [one] => Ok(one.serial.clone()),
        [] => Err(format!(
            "no usable device (attached: {:?}); start an emulator or plug in a phone",
            devices.iter().map(|d| &d.serial).collect::<Vec<_>>()
        )),
        many => Err(format!(
            "several devices; pick one with --serial: {:?}",
            many.iter().map(|d| &d.serial).collect::<Vec<_>>()
        )),
    }
}

/// The daemon builds: `daemon` for whatever ABI the device has, or the ones
/// `npm run build:daemon` wrote to `src-tauri/bin`.
pub fn binaries(daemon: Option<PathBuf>) -> DaemonBinaries {
    match daemon {
        Some(p) => DaemonBinaries {
            arm64: Some(p.clone()),
            x86_64: Some(p),
        },
        None => DaemonBinaries {
            arm64: find_bin("daemon-arm64"),
            x86_64: find_bin("daemon-x86_64"),
        },
    }
}

/// Push and start the daemon on `serial`, confined to `confine` (one of
/// the roots host-core accepts, `/sdcard` by default), and connect.
pub fn start_daemon(
    transport: &dyn Transport,
    serial: &str,
    confine: Option<&str>,
    daemon: Option<PathBuf>,
) -> Result<Daemon, String> {
    Daemon::start(
        transport,
        serial,
        confine.unwrap_or(DEFAULT_ROOT),
        &binaries(daemon),
        &LaunchOptions::default(),
    )
    .map_err(|e| e.to_string())
}

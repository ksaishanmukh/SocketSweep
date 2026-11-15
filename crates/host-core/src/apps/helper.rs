//! Pushing and running the app helper on the phone.
//!
//! Every run has a time limit: `app_process` starts a Java runtime and the
//! helper reaches into framework services, either of which can hang on an
//! odd phone. [`Transport::shell`] has no timeout of its own, so the run
//! happens on a thread that is abandoned when the limit passes; the helper is
//! then killed on the phone, which ends that shell call too.
//!
//! Names and icons are separate runs. Drawing icons has crashed the helper on
//! some phones (a native crash in the graphics stack), so icons are asked
//! for a few packages at a time, and a crash costs only that run's icons.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use socketsweep_transport::{Output, Transport, TransportError};
use socketsweep_wire::apps::{parse_apps, AppInfo, AppsError};

use super::DEVICE_HELPER;
use crate::daemon::shell_quote;

/// The helper's entry class.
const HELPER_CLASS: &str = "dev.socketsweep.Helper";

/// Kill any running helper. The bracket stops the pattern matching the shell
/// that runs it.
pub const KILL_HELPER: &str = "pkill -f '[d]ev.socketsweep.Helper'";

/// Limits on the helper's runs.
#[derive(Debug, Clone)]
pub struct HelperOptions {
    /// How long one run may take before it is given up.
    pub timeout: Duration,
    /// Packages per icon run.
    pub icon_chunk: usize,
    /// Icon edge in pixels.
    pub icon_size: u32,
}

impl Default for HelperOptions {
    fn default() -> Self {
        HelperOptions {
            timeout: Duration::from_secs(20),
            icon_chunk: 30,
            icon_size: 48,
        }
    }
}

/// Why a helper run gave nothing.
#[derive(Debug)]
pub enum HelperError {
    Transport(TransportError),
    /// It did not finish within the time limit.
    TimedOut(Duration),
    /// It exited non-zero (or was killed by a signal, as a crash is).
    Exit {
        code: Option<i32>,
        stderr: String,
    },
    /// It printed something that is not the helper's format.
    Output(AppsError),
}

impl std::fmt::Display for HelperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelperError::Transport(e) => write!(f, "{e}"),
            HelperError::TimedOut(limit) => {
                write!(f, "the app helper did not finish within {limit:?}")
            }
            HelperError::Exit { code, stderr } => {
                write!(f, "the app helper exited with {code:?}: {}", stderr.trim())
            }
            HelperError::Output(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HelperError {}

/// Copy the helper at `local` to the phone.
pub fn push_helper(
    transport: &dyn Transport,
    serial: &str,
    local: &Path,
) -> Result<(), TransportError> {
    transport.push(serial, local, DEVICE_HELPER, 0o644)
}

/// The shell command that runs the helper, asking for the icons of
/// `icon_packages` (none: names only).
pub fn helper_command(icon_packages: &[String], icon_size: u32) -> String {
    let mut command = format!("CLASSPATH={DEVICE_HELPER} app_process / {HELPER_CLASS} apps");
    if !icon_packages.is_empty() {
        command.push_str(&format!(
            " --icons {} --icon-size {icon_size}",
            shell_quote(&icon_packages.join(","))
        ));
    }
    command
}

/// Every installed app's package and label.
pub fn list_apps(
    transport: &Arc<dyn Transport>,
    serial: &str,
    options: &HelperOptions,
) -> Result<Vec<AppInfo>, HelperError> {
    run(
        transport,
        serial,
        helper_command(&[], options.icon_size),
        options.timeout,
    )
}

/// The icons of `packages` (one run; keep it to [`HelperOptions::icon_chunk`]),
/// as PNG bytes. Packages the phone drew no icon for are missing.
pub fn fetch_icons(
    transport: &Arc<dyn Transport>,
    serial: &str,
    packages: &[String],
    options: &HelperOptions,
) -> Result<HashMap<String, Vec<u8>>, HelperError> {
    let apps = run(
        transport,
        serial,
        helper_command(packages, options.icon_size),
        options.timeout,
    )?;
    Ok(apps
        .into_iter()
        .filter(|app| packages.contains(&app.pkg))
        .filter_map(|app| Some((app.pkg, app.icon_png?)))
        .collect())
}

fn run(
    transport: &Arc<dyn Transport>,
    serial: &str,
    command: String,
    timeout: Duration,
) -> Result<Vec<AppInfo>, HelperError> {
    let output = shell_with_timeout(transport, serial, command, timeout)?;
    if !output.success() {
        return Err(HelperError::Exit {
            code: output.exit,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    parse_apps(&String::from_utf8_lossy(&output.stdout)).map_err(HelperError::Output)
}

/// Run `command`, giving up after `timeout` and killing the helper.
fn shell_with_timeout(
    transport: &Arc<dyn Transport>,
    serial: &str,
    command: String,
    timeout: Duration,
) -> Result<Output, HelperError> {
    let (tx, rx) = mpsc::channel();
    let (t, s) = (Arc::clone(transport), serial.to_string());
    std::thread::Builder::new()
        .name("socketsweep-helper".into())
        .spawn(move || {
            let _ = tx.send(t.shell(&s, &command));
        })
        .map_err(|e| HelperError::Transport(TransportError::Io(e)))?;
    match rx.recv_timeout(timeout) {
        Ok(result) => result.map_err(HelperError::Transport),
        Err(RecvTimeoutError::Disconnected) => Err(HelperError::Transport(
            TransportError::Protocol("the shell call ended without an answer".into()),
        )),
        Err(RecvTimeoutError::Timeout) => {
            let _ = transport.shell(serial, KILL_HELPER);
            Err(HelperError::TimedOut(timeout))
        }
    }
}

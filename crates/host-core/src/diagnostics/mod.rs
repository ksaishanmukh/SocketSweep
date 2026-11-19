//! The plain-text report a user pastes into a bug report, and the helpers
//! that keep personal details out of it and out of the log.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The log files' name stem: the shell writes `socketsweep.log.<date>`.
pub const LOG_FILE_PREFIX: &str = "socketsweep.log";

/// How many of the newest log lines the report carries.
pub(crate) const LOG_LINES: usize = 200;

/// Enough for 200 lines; a bigger file is read from its end only.
const TAIL_BYTES: u64 = 256 * 1024;

pub(crate) struct Phone {
    pub(crate) model: String,
    pub(crate) sdk: u32,
    pub(crate) abi: String,
    pub(crate) daemon_build: String,
    pub(crate) serial: String,
    pub(crate) root: String,
}

pub(crate) struct Report {
    pub(crate) app_version: &'static str,
    pub(crate) phone: Option<Phone>,
    pub(crate) log: Vec<String>,
}

/// `private` adds what identifies the phone and its files: the serial, the
/// scan root, and log lines as written instead of with paths and the serial
/// masked.
pub(crate) fn render(report: &Report, private: bool) -> String {
    let mut out = format!(
        "SocketSweep diagnostics\nApp: {}\nOS: {} {}\n",
        report.app_version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let serial = report.phone.as_ref().map(|p| p.serial.as_str());
    match &report.phone {
        Some(p) => {
            out += &format!("Phone: {}, Android API {}, {}\n", p.model, p.sdk, p.abi);
            out += &format!("Daemon: {}\n", p.daemon_build);
            if private {
                out += &format!("Serial: {}\nRoot: {}\n", p.serial, p.root);
            }
        }
        None => out += "Phone: not connected\n",
    }
    out += &format!("\nLog (last {} lines):\n", report.log.len());
    for line in &report.log {
        if private {
            out += line;
        } else {
            out += &scrub(line, serial);
        }
        out.push('\n');
    }
    out
}

/// Masks the phone's serial and anything that looks like a file path, since
/// paths on a phone are personal.
pub(crate) fn scrub(line: &str, serial: Option<&str>) -> String {
    let line = match serial {
        Some(s) if !s.is_empty() => line.replace(s, "<serial>"),
        _ => line.to_string(),
    };
    line.split(' ')
        .map(|word| {
            if word.contains('/') || word.contains('\\') {
                "<path>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The last `max` lines of the newest log file in `dir`, oldest first; empty
/// when there is none, since a report without a log is still worth having.
pub(crate) fn log_tail(dir: &Path, max: usize) -> Vec<String> {
    let newest = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(LOG_FILE_PREFIX))
        // Names end in the date, so the greatest is the newest.
        .max_by_key(|e| e.file_name());
    let Some(file) = newest else {
        return Vec::new();
    };
    let Ok(text) = read_end(&file.path()) else {
        return Vec::new();
    };
    let mut lines: Vec<&str> = text.lines().collect();
    // A read that began mid-file starts with a partial line.
    if text.len() as u64 >= TAIL_BYTES && !lines.is_empty() {
        lines.remove(0);
    }
    let skip = lines.len().saturating_sub(max);
    lines[skip..].iter().map(|l| (*l).to_string()).collect()
}

fn read_end(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests;

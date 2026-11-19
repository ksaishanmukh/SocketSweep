//! The diagnostics report and the GUI's log lines.

use super::errors::CmdResult;
use super::App;
use crate::diagnostics::{self, Phone, Report, LOG_LINES};
use crate::lock;

impl App {
    /// The report for a bug report. `include_private` adds the phone's
    /// serial, the scan root and the log without masking paths.
    pub fn diagnostics(&self, include_private: bool) -> CmdResult<String> {
        let phone = lock(&self.inner.conn).clone().map(|c| Phone {
            model: c.model.clone(),
            sdk: c.daemon.info.sdk,
            abi: c.daemon.info.abi.clone(),
            daemon_build: c.daemon.client.welcome().daemon_build.clone(),
            serial: c.daemon.serial.clone(),
            root: c.daemon.root.clone(),
        });
        let log = match &self.inner.config.log_dir {
            Some(dir) => diagnostics::log_tail(dir, LOG_LINES),
            None => Vec::new(),
        };
        let report = Report {
            app_version: env!("CARGO_PKG_VERSION"),
            phone,
            log,
        };
        Ok(diagnostics::render(&report, include_private))
    }

    /// Writes a line of the GUI's activity log to the log file. The serial
    /// and paths in it are masked at info level; the full line is debug.
    pub fn log_ui(&self, line: &str) -> CmdResult<()> {
        let serial = lock(&self.inner.conn)
            .as_ref()
            .map(|c| c.daemon.serial.clone());
        tracing::info!(target: "gui", "{}", diagnostics::scrub(line, serial.as_deref()));
        tracing::debug!(target: "gui", "{line}");
        Ok(())
    }
}

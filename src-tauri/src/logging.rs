//! Where the app's logs go: a rolling file in the app's log folder, and
//! stderr for `tauri dev`.

use std::path::Path;

use socketsweep_host_core::diagnostics::LOG_FILE_PREFIX;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{InitError, RollingFileAppender, Rotation};
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

/// A new file each day and only the newest three kept: a day's log is a few
/// hundred KB at info, so the folder stays well under a few MB.
const KEPT_FILES: usize = 3;

/// Starts logging to `dir`. Keep the guard alive until exit: dropping it
/// flushes the lines still queued for the writer thread.
///
/// Info by default, so paths from the phone (debug) stay out; `RUST_LOG`
/// overrides it.
pub(crate) fn init(dir: &Path) -> Result<WorkerGuard, InitError> {
    let file = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .max_log_files(KEPT_FILES)
        .build(dir)?;
    let (file, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // A subscriber installed earlier stays; logging is not worth failing startup.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(file.and(std::io::stderr))
        .try_init();
    Ok(guard)
}

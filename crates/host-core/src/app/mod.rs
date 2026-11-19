//! [`App`]: the host's brain, answering the GUI's commands.
//!
//! # Threads and locks
//!
//! Every command may block on the phone; the shell calls them off the window
//! thread. A scan runs on an ingest thread of its own, which drains the scan's
//! reply channel into the tree and emits `scan-progress` every
//! [`AppConfig::progress_interval`]. Focus changes are coalesced by a
//! [`FocusDriver`] per scan.
//!
//! **No lock is held across device I/O.** Shared state is reached through
//! `Arc`s cloned out of their slot, and the long operations that must not
//! overlap (connecting, starting a scan, deleting, copying) claim the single
//! `Busy` slot instead of holding a lock. A running scan does not hold it:
//! queries, focus, delete and copy all work while it runs.
//!
//! Lock order where several are taken: `busy`, `conn`, `scan`, then a scan's
//! `tree`, then its `meta`; a connection's app cache is taken alone.
//!
//! # App names and icons
//!
//! Once a scan has started, a thread of its own runs the app helper on the
//! phone (see [`crate::apps`]): names first, then icons a few packages at a
//! time. It never holds up the scan, and it stops when the scan is replaced
//! or the session ends. What it learns is kept per connection, so a rescan
//! shows names at once, and each answer is announced with `apps-updated`.
//!
//! # Sessions
//!
//! One phone at a time. Every scan gets a new GUI [`SessionId`], so a node
//! from an earlier scan is refused as stale. A connection carries an epoch,
//! bumped by every disconnect, so a connect that was under way when the user
//! disconnected does not install itself afterwards, and a lost-connection
//! report for an old connection is ignored.

mod apps;
mod commands;
mod connect;
mod copy;
mod delete;
mod diagnostics;
mod errors;
mod events;
mod overview;
mod queries;
mod scan;
mod staged;
mod state;

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use socketsweep_gui_contract::SCAN_PROGRESS_INTERVAL_MS;
#[cfg(doc)]
use socketsweep_gui_contract::{ErrorCode, SessionId};
use socketsweep_transport::{Transport, TransportError};
use socketsweep_wire::ScanOpts;

use crate::apps::helper::HelperOptions;
use crate::daemon::{DaemonBinaries, LaunchOptions};
use crate::focus::FOCUS_WINDOW;
use crate::history::History;
use state::{Inner, TransportFactory};

#[cfg(doc)]
use crate::diagnostics::LOG_FILE_PREFIX;
#[cfg(doc)]
use crate::focus::FocusDriver;

pub use events::{EventSink, HostEvent};

/// Everything the host needs from the shell.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// The daemon builds and app helper bundled with the app.
    pub daemon: DaemonBinaries,
    pub launch: LaunchOptions,
    /// Time limit and batch size of the app helper's runs.
    pub helper: HelperOptions,
    /// Where scan history is kept; `None` keeps none.
    pub history_file: Option<PathBuf>,
    /// Where the shell writes its log files (named [`LOG_FILE_PREFIX`]`.*`),
    /// for the diagnostics report; `None` leaves the log out of it.
    pub log_dir: Option<PathBuf>,
    /// Where `copyMany` copies when the GUI names no destination: the
    /// desktop.
    pub default_destination: Option<PathBuf>,
    pub scan_opts: ScanOpts,
    pub focus_window: Duration,
    pub progress_interval: Duration,
    /// How long `disconnect` and a rescan wait for a cancelled scan to end.
    pub scan_release_timeout: Duration,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            daemon: DaemonBinaries::default(),
            launch: LaunchOptions::default(),
            helper: HelperOptions::default(),
            history_file: None,
            log_dir: None,
            default_destination: None,
            scan_opts: ScanOpts::default(),
            focus_window: FOCUS_WINDOW,
            progress_interval: Duration::from_millis(u64::from(SCAN_PROGRESS_INTERVAL_MS)),
            scan_release_timeout: Duration::from_secs(3),
        }
    }
}

/// The host.
pub struct App {
    inner: Arc<Inner>,
}

impl App {
    /// A host using `transport`, sending events to `sink`.
    pub fn new(
        transport: Arc<dyn Transport>,
        config: AppConfig,
        sink: impl Fn(HostEvent) + Send + Sync + 'static,
    ) -> Self {
        Self::build(Some(transport), None, config, sink)
    }

    /// A host that builds its transport with `factory` the first time a
    /// command needs a phone (`list_devices`, `connect`), so a transport that
    /// cannot start, such as adb, fails that command with a
    /// [`ErrorCode::Device`] error instead of stopping the app from opening.
    /// A failed attempt is retried by the next command.
    pub fn lazy<T: Transport + 'static>(
        factory: impl Fn() -> Result<T, TransportError> + Send + Sync + 'static,
        config: AppConfig,
        sink: impl Fn(HostEvent) + Send + Sync + 'static,
    ) -> Self {
        let factory: TransportFactory =
            Box::new(move || factory().map(|t| Arc::new(t) as Arc<dyn Transport>));
        Self::build(None, Some(factory), config, sink)
    }

    fn build(
        transport: Option<Arc<dyn Transport>>,
        factory: Option<TransportFactory>,
        config: AppConfig,
        sink: impl Fn(HostEvent) + Send + Sync + 'static,
    ) -> Self {
        let history = config.history_file.clone().map(History::new);
        App {
            inner: Arc::new(Inner {
                transport: Mutex::new(transport),
                factory,
                config,
                sink: Arc::new(sink),
                history,
                busy: Mutex::new(None),
                conn: Mutex::new(None),
                scan: Mutex::new(None),
                epoch: AtomicU64::new(0),
            }),
        }
    }

    /// End the session, as on app exit. Also done when the `App` is dropped.
    pub fn shutdown(&self) {
        self.inner.end_session();
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.inner.end_session();
    }
}

#[cfg(test)]
mod tests;

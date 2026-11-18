//! The events the host sends to the GUI.

use std::sync::Arc;

use socketsweep_gui_contract::{AppsUpdated, ConnectionLost, CopyProgress, ScanProgress};

/// An event for the GUI. The shell emits the payload under its contract name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    ScanProgress(ScanProgress),
    CopyProgress(CopyProgress),
    ConnectionLost(ConnectionLost),
    AppsUpdated(AppsUpdated),
}

/// Where events go: the shell forwards each to the window.
pub type EventSink = Arc<dyn Fn(HostEvent) + Send + Sync>;

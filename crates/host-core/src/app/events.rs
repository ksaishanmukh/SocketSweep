//! The events the host sends to the GUI.

use std::sync::Arc;

use socketsweep_gui_contract::{ConnectionLost, CopyProgress, Event, ScanProgress};

/// An event for the GUI. The shell emits the payload under [`HostEvent::name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    ScanProgress(ScanProgress),
    CopyProgress(CopyProgress),
    ConnectionLost(ConnectionLost),
}

impl HostEvent {
    /// The Tauri event name, from the contract.
    pub fn name(&self) -> &'static str {
        match self {
            HostEvent::ScanProgress(_) => ScanProgress::NAME,
            HostEvent::CopyProgress(_) => CopyProgress::NAME,
            HostEvent::ConnectionLost(_) => ConnectionLost::NAME,
        }
    }
}

/// Where events go: the shell forwards each to the window.
pub type EventSink = Arc<dyn Fn(HostEvent) + Send + Sync>;

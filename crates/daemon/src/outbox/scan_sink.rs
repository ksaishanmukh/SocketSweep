//! The engine's side of the outbox.

use std::sync::Arc;

use socketsweep_engine::EventSink;
use socketsweep_wire::{ReqId, ScanEvent, ServerFrame};

use super::Outbox;

/// The engine's [`EventSink`] for one scan: its events go to the bulk queue
/// under the scan's request id.
pub struct ScanSink {
    pub id: ReqId,
    pub outbox: Arc<Outbox>,
}

impl EventSink for ScanSink {
    fn bulk(&self, events: Vec<ScanEvent>) {
        self.outbox.bulk(self.id, events);
    }
    fn control(&self, frame: ServerFrame) {
        self.outbox.control(&frame);
    }
    fn congested(&self) -> bool {
        self.outbox.congested()
    }
}

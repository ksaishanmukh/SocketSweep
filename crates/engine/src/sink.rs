//! Where scan events go (`trait EventSink`).
//!
//! Internal contract between the engine and the daemon's session writer
//! (`docs/architecture.md`, section 3, `session`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use socketsweep_wire::{ScanEvent, ServerFrame};

/// The session's two outgoing queues, as the engine sees them.
///
/// The engine calls [`EventSink::bulk`] **while holding the scheduler lock**,
/// so the order of calls is the order of state changes; that is what gives
/// the ordering guarantees G1–G3. An implementation must therefore append
/// and return at once: never block, never wait for the connection, and never
/// call back into the scan.
pub trait EventSink: Send + Sync {
    /// Append scan events to the bulk queue, in order. The session wraps them
    /// in `ServerMsg::Scan` frames under the scan's request id and batches
    /// them to at most `MAX_SCAN_BATCH_BYTES`.
    fn bulk(&self, events: Vec<ScanEvent>);

    /// Queue a control frame (`Pong`, `Deleted`, `Failed`), which is sent
    /// before anything in the bulk queue. The engine itself does not use it;
    /// it is here so one sink serves the whole session.
    fn control(&self, frame: ServerFrame);

    /// The bulk queue is above its high-water mark (and has not yet drained
    /// below the low-water mark): workers take no new tasks until it clears.
    fn congested(&self) -> bool;
}

impl<T: EventSink + ?Sized> EventSink for Arc<T> {
    fn bulk(&self, events: Vec<ScanEvent>) {
        (**self).bulk(events)
    }
    fn control(&self, frame: ServerFrame) {
        (**self).control(frame)
    }
    fn congested(&self) -> bool {
        (**self).congested()
    }
}

/// A sink that keeps everything in memory, for tests and tools.
#[derive(Debug, Default)]
pub struct RecordingSink {
    events: Mutex<Vec<ScanEvent>>,
    control: Mutex<Vec<ServerFrame>>,
    congested: AtomicBool,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make [`EventSink::congested`] report `on`.
    pub fn set_congested(&self, on: bool) {
        self.congested.store(on, Ordering::Relaxed);
    }

    /// Everything received so far, in order.
    pub fn events(&self) -> Vec<ScanEvent> {
        lock(&self.events).clone()
    }

    /// Remove and return everything received so far.
    pub fn take(&self) -> Vec<ScanEvent> {
        std::mem::take(&mut *lock(&self.events))
    }

    pub fn control_frames(&self) -> Vec<ServerFrame> {
        lock(&self.control).clone()
    }
}

impl EventSink for RecordingSink {
    fn bulk(&self, events: Vec<ScanEvent>) {
        lock(&self.events).extend(events);
    }
    fn control(&self, frame: ServerFrame) {
        lock(&self.control).push(frame);
    }
    fn congested(&self) -> bool {
        self.congested.load(Ordering::Relaxed)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

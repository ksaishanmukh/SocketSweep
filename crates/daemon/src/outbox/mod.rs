//! The session's writer: two queues and the one thread that sends them.
//!
//! - **Control queue:** whole frames (`Pong`, `Deleted`, `Failed`,
//!   `CopyManifest`, `DeleteProgress`). Every round sends all of it before any
//!   bulk, so a reply never waits behind a scan's backlog.
//! - **Bulk queue:** scan events, each encoded the moment it is queued (under
//!   the engine's scheduler lock, which is what gives G1–G3), so the queue is
//!   accounted in bytes. Above [`OutboxConfig::high_water`] it reports itself
//!   [`congested`](Outbox::congested) and the engine's workers stop taking
//!   tasks until the writer has drained it below
//!   [`OutboxConfig::low_water`]. Adding to it never blocks.
//! - **Batching:** consecutive events of one scan go out as one
//!   `ServerMsg::Scan` frame of at most [`OutboxConfig::max_batch`] bytes of
//!   events (a single larger event goes alone). Bulk waits up to
//!   [`OutboxConfig::flush_every`] to fill a batch, so a busy scan sends few,
//!   full frames and a quiet one is still prompt.
//! - **Keepalive:** a `Heartbeat` after [`OutboxConfig::heartbeat`] with
//!   nothing sent, and the connection is given up after
//!   [`OutboxConfig::dead_after`] with nothing received
//!   ([`Outbox::touch_rx`]).
//!
//! A batch frame is assembled from the pre-encoded events without decoding
//! them again: postcard encodes `ServerFrame { id, msg: Scan(events) }` as
//! `varint(id) varint(Scan's index) varint(events.len())` followed by each
//! event's own encoding. A test pins this against `encode_frame`.

//!
//! The writer thread is in `writer`; the batch frame encoding in `frame`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use socketsweep_engine::EventSink;
use socketsweep_wire::{
    encode_frame, ReqId, ScanEvent, ServerFrame, ServerMsg, BULK_HIGH_WATER, BULK_LOW_WATER,
    DAEMON_DEAD_AFTER, HEARTBEAT_INTERVAL, MAX_SCAN_BATCH_BYTES,
};

mod frame;
mod writer;

#[cfg(test)]
mod tests;

pub use frame::scan_frame;

/// Timing and sizes of one session's writer. [`Default`] follows the wire
/// constants; tests shorten the times.
#[derive(Debug, Clone)]
pub struct OutboxConfig {
    pub heartbeat: Duration,
    pub dead_after: Duration,
    pub flush_every: Duration,
    pub high_water: usize,
    pub low_water: usize,
    pub max_batch: usize,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        OutboxConfig {
            heartbeat: HEARTBEAT_INTERVAL,
            dead_after: DAEMON_DEAD_AFTER,
            flush_every: Duration::from_millis(50),
            high_water: BULK_HIGH_WATER,
            low_water: BULK_LOW_WATER,
            max_batch: MAX_SCAN_BATCH_BYTES,
        }
    }
}

/// The two queues and the writer's clock. Shared by the reader, the scans
/// (through [`ScanSink`]) and the writer thread ([`Outbox::run_writer`]).
pub struct Outbox {
    state: Mutex<State>,
    cv: Condvar,
    congested: AtomicBool,
    last_rx: Mutex<Instant>,
    cfg: OutboxConfig,
}

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

impl Outbox {
    pub fn new(cfg: OutboxConfig) -> Arc<Outbox> {
        Arc::new(Outbox {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
            congested: AtomicBool::new(false),
            last_rx: Mutex::new(Instant::now()),
            cfg,
        })
    }

    pub fn config(&self) -> &OutboxConfig {
        &self.cfg
    }

    /// Queue a control frame, sent before anything in the bulk queue.
    pub fn control(&self, frame: &ServerFrame) {
        let bytes = match encode_frame(frame) {
            Ok(b) => b,
            Err(e) => {
                crate::log!("dropping an unencodable control frame: {e}");
                return;
            }
        };
        let mut st = lock(&self.state);
        if st.closed.is_some() {
            return;
        }
        st.control.push_back(bytes);
        drop(st);
        self.cv.notify_all();
    }

    /// Shorthand for [`Outbox::control`].
    pub fn reply(&self, id: ReqId, msg: ServerMsg) {
        self.control(&ServerFrame { id, msg });
    }

    /// Append scan `id`'s events to the bulk queue, in order. Never blocks.
    pub fn bulk(&self, id: ReqId, events: Vec<ScanEvent>) {
        if events.is_empty() {
            return;
        }
        let encoded: Vec<Vec<u8>> = events
            .iter()
            .filter_map(|e| match postcard::to_allocvec(e) {
                Ok(b) => Some(b),
                Err(err) => {
                    crate::log!("dropping an unencodable scan event: {err}");
                    None
                }
            })
            .collect();
        let mut st = lock(&self.state);
        let was_empty = st.bulk.is_empty();
        if st.closed.is_some() {
            return;
        }
        for b in encoded {
            st.bulk_bytes += b.len();
            st.bulk.push_back((id, b));
        }
        if st.bulk_bytes > self.cfg.high_water {
            self.congested.store(true, Ordering::Relaxed);
        }
        // Wake the writer when the queue stops being empty (so it starts the
        // flush_every clock) and when a full batch is ready; not per event.
        let wake = was_empty || st.bulk_bytes >= self.cfg.max_batch;
        drop(st);
        if wake {
            self.cv.notify_all();
        }
    }

    /// Above the high-water mark and not yet back below the low-water mark.
    pub fn congested(&self) -> bool {
        self.congested.load(Ordering::Relaxed)
    }

    /// Bytes waiting in the bulk queue.
    pub fn bulk_bytes(&self) -> usize {
        lock(&self.state).bulk_bytes
    }

    /// Note that a frame arrived from the host.
    pub fn touch_rx(&self) {
        *lock(&self.last_rx) = Instant::now();
    }

    /// Stop at once, dropping anything queued. The first reason wins.
    pub fn close(&self, why: impl Into<String>) {
        let mut st = lock(&self.state);
        if st.closed.is_none() {
            st.closed = Some(why.into());
        }
        st.control.clear();
        st.bulk.clear();
        st.bulk_bytes = 0;
        drop(st);
        self.congested.store(false, Ordering::Relaxed);
        self.cv.notify_all();
    }

    /// Send everything already queued, then stop.
    pub fn finish(&self) {
        lock(&self.state).draining = true;
        self.cv.notify_all();
    }

    /// Why the writer stopped, once it has.
    pub fn closed(&self) -> Option<String> {
        lock(&self.state).closed.clone()
    }
}

#[derive(Default)]
struct State {
    control: VecDeque<Vec<u8>>,
    bulk: VecDeque<(ReqId, Vec<u8>)>,
    bulk_bytes: usize,
    /// Why the writer stopped (or must stop), once it has.
    closed: Option<String>,
    /// Send everything queued, then stop.
    draining: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

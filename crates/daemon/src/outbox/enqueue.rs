//! Producer side of the outbox: what the session and the engine queue.

use std::sync::atomic::Ordering;

use socketsweep_wire::{encode_frame, ReqId, ScanEvent, ServerFrame, ServerMsg};

use super::{lock, Outbox};

impl Outbox {
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
}

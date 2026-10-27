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

use std::collections::VecDeque;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use socketsweep_engine::EventSink;
use socketsweep_wire::{
    encode_frame, ReqId, ScanEvent, ServerFrame, ServerMsg, BULK_HIGH_WATER, BULK_LOW_WATER,
    DAEMON_DEAD_AFTER, HEARTBEAT_INTERVAL, MAX_SCAN_BATCH_BYTES, SESSION_LEVEL,
};

/// Bulk bytes taken per writer round at most, so control frames queued
/// meanwhile wait for at most this much bulk.
const ROUND_BYTES: usize = 1024 * 1024;

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

/// The two queues and the writer's clock. Shared by the reader, the scans
/// (through [`ScanSink`]) and the writer thread ([`Outbox::run_writer`]).
pub struct Outbox {
    state: Mutex<State>,
    cv: Condvar,
    congested: AtomicBool,
    last_rx: Mutex<Instant>,
    cfg: OutboxConfig,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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

    /// The writer thread: send the queues to `out` until closed, finished, a
    /// write fails, or the host has been silent for `dead_after`. Calls
    /// `on_dead` when it stops for a failed write or a silent host, so the
    /// session can shut the connection and wake its reader. Returns why it
    /// stopped.
    pub fn run_writer<W: Write>(&self, out: W, on_dead: impl FnOnce()) -> String {
        let mut out = BufWriter::with_capacity(256 * 1024, out);
        let mut last_tx = Instant::now();
        let mut last_bulk = Instant::now();
        loop {
            let round = match self.next_round(last_tx, last_bulk) {
                Ok(r) => r,
                Err((why, dead)) => {
                    if dead {
                        on_dead();
                    }
                    return why;
                }
            };
            let sent_bulk = !round.batches.is_empty();
            if let Err(e) = round.write(&mut out) {
                let why = format!("cannot write to the host: {e}");
                self.close(why.clone());
                on_dead();
                return why;
            }
            last_tx = Instant::now();
            if sent_bulk {
                last_bulk = last_tx;
            }
        }
    }

    /// Wait for something to send and take it. `Err((why, dead))` when the
    /// writer must stop; `dead` if the host went silent.
    fn next_round(&self, last_tx: Instant, last_bulk: Instant) -> Result<Round, (String, bool)> {
        let cfg = &self.cfg;
        let mut st = lock(&self.state);
        let heartbeat = loop {
            if let Some(why) = &st.closed {
                return Err((why.clone(), false));
            }
            let silent = lock(&self.last_rx).elapsed();
            if silent >= cfg.dead_after {
                let why = format!("the host sent nothing for {} s", silent.as_secs());
                st.closed = Some(why.clone());
                st.control.clear();
                st.bulk.clear();
                st.bulk_bytes = 0;
                self.congested.store(false, Ordering::Relaxed);
                return Err((why, true));
            }
            let now = Instant::now();
            let bulk_due = !st.bulk.is_empty()
                && (st.draining
                    || st.bulk_bytes >= cfg.max_batch
                    || now.duration_since(last_bulk) >= cfg.flush_every);
            if !st.control.is_empty() || bulk_due {
                break false;
            }
            if st.draining {
                st.closed = Some("finished".into());
                return Err(("finished".into(), false));
            }
            let idle = now.duration_since(last_tx);
            if idle >= cfg.heartbeat {
                break true;
            }
            let mut wait = (cfg.heartbeat - idle).min(cfg.dead_after - silent);
            if !st.bulk.is_empty() {
                wait = wait.min(
                    cfg.flush_every
                        .saturating_sub(now.duration_since(last_bulk)),
                );
            }
            let wait = wait.max(Duration::from_millis(1));
            st = match self.cv.wait_timeout(st, wait) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            };
        };

        let control: Vec<Vec<u8>> = st.control.drain(..).collect();
        let mut batches = Vec::new();
        let mut taken = 0;
        while taken < ROUND_BYTES {
            let Some((id, _)) = st.bulk.front() else {
                break;
            };
            let id = *id;
            let mut events = Vec::new();
            let mut bytes = 0;
            while let Some((next_id, ev)) = st.bulk.front() {
                if *next_id != id || (!events.is_empty() && bytes + ev.len() > cfg.max_batch) {
                    break;
                }
                bytes += ev.len();
                let (_, ev) = st.bulk.pop_front().expect("front exists");
                events.push(ev);
            }
            taken += bytes;
            st.bulk_bytes -= bytes;
            batches.push((id, events));
        }
        if st.bulk_bytes <= cfg.low_water {
            self.congested.store(false, Ordering::Relaxed);
        }
        Ok(Round {
            control,
            batches,
            heartbeat,
        })
    }
}

struct Round {
    control: Vec<Vec<u8>>,
    batches: Vec<(ReqId, Vec<Vec<u8>>)>,
    heartbeat: bool,
}

impl Round {
    fn write<W: Write>(self, out: &mut W) -> std::io::Result<()> {
        for frame in &self.control {
            out.write_all(frame)?;
        }
        for (id, events) in &self.batches {
            out.write_all(&scan_frame(*id, events))?;
        }
        if self.heartbeat {
            let hb = encode_frame(&ServerFrame {
                id: SESSION_LEVEL,
                msg: ServerMsg::Heartbeat,
            })
            .map_err(std::io::Error::other)?;
            out.write_all(&hb)?;
        }
        out.flush()
    }
}

fn push_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// A whole `ServerFrame { id, msg: Scan(events) }` frame, length prefix
/// included, from events already encoded one by one.
pub fn scan_frame(id: ReqId, events: &[Vec<u8>]) -> Vec<u8> {
    let body_len: usize = events.iter().map(Vec::len).sum();
    let mut frame = Vec::with_capacity(body_len + 16);
    frame.extend_from_slice(&[0; 4]);
    push_varint(&mut frame, u64::from(id));
    // `ServerMsg::Scan` is variant 2: Pong, Heartbeat, Scan.
    push_varint(&mut frame, 2);
    push_varint(&mut frame, events.len() as u64);
    for e in events {
        frame.extend_from_slice(e);
    }
    let len = (frame.len() - 4) as u32;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    frame
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

#[cfg(test)]
mod tests {
    use super::*;
    use socketsweep_wire::{read_frame, Outcome, Totals};
    use std::io::Cursor;
    use std::sync::mpsc;

    fn ev(i: u32) -> ScanEvent {
        ScanEvent::Pruned { dir: i }
    }

    fn quick() -> OutboxConfig {
        OutboxConfig {
            heartbeat: Duration::from_millis(40),
            dead_after: Duration::from_secs(30),
            flush_every: Duration::from_millis(5),
            ..OutboxConfig::default()
        }
    }

    fn frames(bytes: &[u8]) -> Vec<ServerFrame> {
        let mut r = Cursor::new(bytes);
        let mut out = Vec::new();
        while let Some(f) = read_frame::<_, ServerFrame>(&mut r).unwrap() {
            out.push(f);
        }
        out
    }

    /// A writer target that shares what was written.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            lock(&self.0).extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_batch_frame_is_byte_identical_to_encode_frame() {
        for (id, n) in [(1u32, 0usize), (7, 1), (300, 5), (u32::MAX, 200)] {
            let events: Vec<ScanEvent> = (0..n as u32)
                .map(|i| {
                    if i % 2 == 0 {
                        ev(i * 1000)
                    } else {
                        ScanEvent::Finished {
                            outcome: Outcome::Complete,
                            totals: Totals::default(),
                            elapsed_ms: i,
                        }
                    }
                })
                .collect();
            let encoded: Vec<Vec<u8>> = events
                .iter()
                .map(|e| postcard::to_allocvec(e).unwrap())
                .collect();
            let expected = encode_frame(&ServerFrame {
                id,
                msg: ServerMsg::Scan(events),
            })
            .unwrap();
            assert_eq!(scan_frame(id, &encoded), expected, "id {id}, {n} events");
        }
    }

    #[test]
    fn control_goes_before_bulk_queued_earlier() {
        let ob = Outbox::new(quick());
        ob.bulk(5, (0..100).map(ev).collect());
        ob.reply(9, ServerMsg::Pong { nonce: 3 });
        ob.finish();
        let out = Shared::default();
        ob.run_writer(out.clone(), || {});
        let got = frames(&lock(&out.0));
        assert_eq!(
            got[0],
            ServerFrame {
                id: 9,
                msg: ServerMsg::Pong { nonce: 3 }
            }
        );
        let events: Vec<ScanEvent> = got[1..]
            .iter()
            .flat_map(|f| match &f.msg {
                ServerMsg::Scan(e) => {
                    assert_eq!(f.id, 5);
                    e.clone()
                }
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(events, (0..100).map(ev).collect::<Vec<_>>());
    }

    #[test]
    fn batches_respect_the_size_bound_and_scan_boundaries() {
        let ob = Outbox::new(OutboxConfig {
            max_batch: 64,
            ..quick()
        });
        ob.bulk(1, (0..50).map(ev).collect());
        ob.bulk(2, (0..3).map(ev).collect());
        ob.bulk(1, (50..53).map(ev).collect());
        ob.finish();
        let out = Shared::default();
        ob.run_writer(out.clone(), || {});
        let got = frames(&lock(&out.0));
        let mut seq = Vec::new();
        for f in &got {
            let ServerMsg::Scan(events) = &f.msg else {
                panic!("unexpected {f:?}")
            };
            let bytes: usize = events
                .iter()
                .map(|e| postcard::to_allocvec(e).unwrap().len())
                .sum();
            assert!(bytes <= 64, "batch of {bytes} bytes");
            seq.extend(events.iter().map(|e| (f.id, e.clone())));
        }
        let mut expected: Vec<(ReqId, ScanEvent)> = (0..50).map(|i| (1, ev(i))).collect();
        expected.extend((0..3).map(|i| (2, ev(i))));
        expected.extend((50..53).map(|i| (1, ev(i))));
        assert_eq!(seq, expected);
        assert!(got.len() > 3);
    }

    #[test]
    fn congestion_turns_on_above_high_water_and_off_below_low_water() {
        let ob = Outbox::new(OutboxConfig {
            high_water: 100,
            low_water: 50,
            ..quick()
        });
        // Each Pruned event is two or three bytes.
        ob.bulk(1, (0..40).map(ev).collect());
        assert!(!ob.congested());
        ob.bulk(1, (0..40).map(ev).collect());
        assert!(ob.congested());
        ob.finish();
        ob.run_writer(Shared::default(), || {});
        assert!(!ob.congested());
        assert_eq!(ob.bulk_bytes(), 0);
    }

    #[test]
    fn a_heartbeat_is_sent_after_idle_time() {
        let ob = Outbox::new(quick());
        let out = Shared::default();
        let (tx, rx) = mpsc::channel();
        let o = Arc::clone(&ob);
        let w = out.clone();
        std::thread::spawn(move || tx.send(o.run_writer(w, || {})).unwrap());
        std::thread::sleep(Duration::from_millis(150));
        ob.close("test over");
        assert_eq!(rx.recv().unwrap(), "test over");
        let got = frames(&lock(&out.0));
        assert!(got.len() >= 2, "{got:?}");
        assert!(got
            .iter()
            .all(|f| f.id == SESSION_LEVEL && f.msg == ServerMsg::Heartbeat));
    }

    #[test]
    fn a_silent_host_is_given_up() {
        let ob = Outbox::new(OutboxConfig {
            dead_after: Duration::from_millis(60),
            ..quick()
        });
        let dead = Arc::new(AtomicBool::new(false));
        let d = Arc::clone(&dead);
        let why = ob.run_writer(Shared::default(), move || d.store(true, Ordering::SeqCst));
        assert!(why.contains("sent nothing"), "{why}");
        assert!(dead.load(Ordering::SeqCst));
        // Nothing is queued after the end.
        ob.reply(1, ServerMsg::Pong { nonce: 1 });
        ob.bulk(1, vec![ev(1)]);
        assert_eq!(ob.bulk_bytes(), 0);
    }
}

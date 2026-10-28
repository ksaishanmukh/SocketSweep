//! The writer thread: waiting for something to send, taking a round of it
//! from the queues and writing it out.

use std::io::{BufWriter, Write};
use std::sync::atomic::Ordering;
use std::sync::MutexGuard;
use std::time::{Duration, Instant};

use socketsweep_wire::{encode_frame, ReqId, ServerFrame, ServerMsg, SESSION_LEVEL};

use super::{lock, scan_frame, Outbox, State};

/// Bulk bytes taken per writer round at most, so control frames queued
/// meanwhile wait for at most this much bulk.
const ROUND_BYTES: usize = 1024 * 1024;

impl Outbox {
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
        let st = lock(&self.state);
        let (mut st, heartbeat) = self.wait_for_work(st, last_tx, last_bulk)?;
        let control: Vec<Vec<u8>> = st.control.drain(..).collect();
        let batches = take_batches(&mut st, self.cfg.max_batch);
        if st.bulk_bytes <= self.cfg.low_water {
            self.congested.store(false, Ordering::Relaxed);
        }
        Ok(Round {
            control,
            batches,
            heartbeat,
        })
    }

    /// Wait until a control frame is queued, bulk is due, or a heartbeat is.
    /// Returns the guard and whether a heartbeat is due.
    fn wait_for_work<'a>(
        &'a self,
        mut st: MutexGuard<'a, State>,
        last_tx: Instant,
        last_bulk: Instant,
    ) -> Result<(MutexGuard<'a, State>, bool), (String, bool)> {
        let cfg = &self.cfg;
        let heartbeat = loop {
            if let Some(why) = &st.closed {
                return Err((why.clone(), false));
            }
            let silent = lock(&self.last_rx).elapsed();
            if silent >= cfg.dead_after {
                return Err((self.give_up_silent(&mut st, silent), true));
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
        Ok((st, heartbeat))
    }

    /// The host has been silent for `silent`: close, drop the queues, and
    /// return why.
    fn give_up_silent(&self, st: &mut State, silent: Duration) -> String {
        let why = format!("the host sent nothing for {} s", silent.as_secs());
        st.closed = Some(why.clone());
        st.control.clear();
        st.bulk.clear();
        st.bulk_bytes = 0;
        self.congested.store(false, Ordering::Relaxed);
        why
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

/// Take up to [`ROUND_BYTES`] of bulk, as batches of one scan's events of at
/// most `max_batch` bytes each.
fn take_batches(st: &mut State, max_batch: usize) -> Vec<(ReqId, Vec<Vec<u8>>)> {
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
            if *next_id != id || (!events.is_empty() && bytes + ev.len() > max_batch) {
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
    batches
}

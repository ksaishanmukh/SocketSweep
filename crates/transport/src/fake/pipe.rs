//! An in-memory duplex pipe: what a fake tunnel is made of.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::lock;
use crate::Duplex;

// ── In-memory duplex pipe ───────────────────────────────────────────────────

/// Bytes buffered in each direction of a [`pipe`] before writes block.
pub const DEFAULT_PIPE_CAPACITY: usize = 1024 * 1024;

/// One direction of a pipe.
struct Channel {
    state: Mutex<ChannelState>,
    changed: Condvar,
}

struct ChannelState {
    buf: VecDeque<u8>,
    capacity: usize,
    /// The writing end shut down or went away: reads drain, then see EOF.
    writer_closed: bool,
    /// The reading end shut down or went away: writes fail.
    reader_closed: bool,
}

impl Channel {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Channel {
            state: Mutex::new(ChannelState {
                buf: VecDeque::new(),
                capacity: capacity.max(1),
                writer_closed: false,
                reader_closed: false,
            }),
            changed: Condvar::new(),
        })
    }

    /// Wait for a change, until `deadline` if there is one. Returns the guard
    /// and whether the deadline passed.
    fn wait<'a>(
        &self,
        guard: MutexGuard<'a, ChannelState>,
        deadline: Option<Instant>,
    ) -> (MutexGuard<'a, ChannelState>, bool) {
        match deadline {
            None => (
                self.changed.wait(guard).unwrap_or_else(|e| e.into_inner()),
                false,
            ),
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return (guard, true);
                }
                let (guard, _) = self
                    .changed
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(|e| e.into_inner());
                (guard, false)
            }
        }
    }
}

/// What every handle of one end shares.
struct EndShared {
    rx: Arc<Channel>,
    tx: Arc<Channel>,
    read_timeout: Mutex<Option<Duration>>,
    write_timeout: Mutex<Option<Duration>>,
}

impl EndShared {
    fn close(&self) {
        {
            let mut tx = lock(&self.tx.state);
            tx.writer_closed = true;
        }
        self.tx.changed.notify_all();
        {
            let mut rx = lock(&self.rx.state);
            rx.reader_closed = true;
        }
        self.rx.changed.notify_all();
    }
}

impl Drop for EndShared {
    fn drop(&mut self) {
        self.close();
    }
}

/// One end of an in-memory [`pipe`]. Behaves like a connected socket: reads
/// block until data arrives, return end of stream once the other end has
/// closed and everything it sent has been read, and time out with
/// [`io::ErrorKind::TimedOut`]; writes block while the other direction's
/// buffer is full and fail with [`io::ErrorKind::BrokenPipe`] once the other
/// end has gone.
pub struct PipeEnd(Arc<EndShared>);

/// A connected pair of in-memory [`Duplex`] streams with
/// [`DEFAULT_PIPE_CAPACITY`] buffered each way.
pub fn pipe() -> (PipeEnd, PipeEnd) {
    pipe_with_capacity(DEFAULT_PIPE_CAPACITY)
}

/// As [`pipe`], buffering at most `capacity` bytes each way, so tests can
/// exercise back-pressure.
pub fn pipe_with_capacity(capacity: usize) -> (PipeEnd, PipeEnd) {
    let a_to_b = Channel::new(capacity);
    let b_to_a = Channel::new(capacity);
    let end = |rx: &Arc<Channel>, tx: &Arc<Channel>| {
        PipeEnd(Arc::new(EndShared {
            rx: Arc::clone(rx),
            tx: Arc::clone(tx),
            read_timeout: Mutex::new(None),
            write_timeout: Mutex::new(None),
        }))
    };
    (end(&b_to_a, &a_to_b), end(&a_to_b, &b_to_a))
}

fn deadline(timeout: &Mutex<Option<Duration>>) -> Option<Instant> {
    lock(timeout).map(|t| Instant::now() + t)
}

impl Read for PipeEnd {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let deadline = deadline(&self.0.read_timeout);
        let channel = &self.0.rx;
        let mut state = lock(&channel.state);
        loop {
            if !state.buf.is_empty() {
                let n = out.len().min(state.buf.len());
                for (slot, byte) in out.iter_mut().zip(state.buf.drain(..n)) {
                    *slot = byte;
                }
                drop(state);
                channel.changed.notify_all();
                return Ok(n);
            }
            if state.writer_closed || state.reader_closed {
                return Ok(0);
            }
            let (next, timed_out) = channel.wait(state, deadline);
            if timed_out {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pipe read timed out",
                ));
            }
            state = next;
        }
    }
}

impl Write for PipeEnd {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let deadline = deadline(&self.0.write_timeout);
        let channel = &self.0.tx;
        let mut state = lock(&channel.state);
        loop {
            if state.reader_closed || state.writer_closed {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "the other end of the pipe is closed",
                ));
            }
            let space = state.capacity - state.buf.len();
            if space > 0 {
                let n = space.min(data.len());
                state.buf.extend(&data[..n]);
                drop(state);
                channel.changed.notify_all();
                return Ok(n);
            }
            let (next, timed_out) = channel.wait(state, deadline);
            if timed_out {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pipe write timed out",
                ));
            }
            state = next;
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Duplex for PipeEnd {
    fn shutdown(&self) -> io::Result<()> {
        self.0.close();
        Ok(())
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *lock(&self.0.read_timeout) = timeout;
        Ok(())
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *lock(&self.0.write_timeout) = timeout;
        Ok(())
    }

    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(PipeEnd(Arc::clone(&self.0))))
    }
}

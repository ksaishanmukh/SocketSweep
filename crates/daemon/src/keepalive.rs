//! Keeping a connection alive through a long, silent operation.
//!
//! The host bounds how long it waits in silence, not how long a reply takes in
//! all, so anything that can run for minutes without producing output sends
//! [`Frame::Keepalive`] while it works.
//!
//! Free of socket and Android specifics so it can be tested on any development
//! machine.

use std::io::{self, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use socketsweep_protocol::{write_msg, Frame};

/// What [`run_with_keepalives`] saw.
pub struct Finished<T> {
    /// The job's result, or why there is none: the thread could not be
    /// started, or the job panicked.
    pub result: io::Result<T>,
    /// Whether every keepalive was written and flushed. Once one fails nothing
    /// more is sent, because part of a frame may already be on the wire and
    /// anything after it would be misread.
    pub connected: bool,
}

/// Run `job` on a thread of its own, writing a [`Frame::Keepalive`] to `out`
/// and flushing it every `interval` until the job finishes.
///
/// The job is always waited for, even after a keepalive fails to send: it
/// cannot be stopped part-way, and the daemon's next request must not race it.
pub fn run_with_keepalives<T, W, F>(out: &mut W, interval: Duration, job: F) -> Finished<T>
where
    T: Send + 'static,
    W: Write,
    F: FnOnce() -> T + Send + 'static,
{
    let (done_tx, done_rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("worker".into())
        .spawn(move || {
            let _ = done_tx.send(job());
        });
    if let Err(e) = spawned {
        return Finished {
            result: Err(e),
            connected: true,
        };
    }

    let mut connected = true;
    let result = loop {
        match done_rx.recv_timeout(interval) {
            Ok(value) => break Ok(value),
            Err(RecvTimeoutError::Timeout) => {
                if connected {
                    connected = write_msg(out, &Frame::Keepalive).is_ok() && out.flush().is_ok();
                }
            }
            // The thread ended without answering, which only a panic does.
            Err(RecvTimeoutError::Disconnected) => {
                break Err(io::Error::other("the operation stopped unexpectedly"))
            }
        }
    };

    Finished { result, connected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use socketsweep_protocol::read_msg;
    use std::sync::mpsc::Receiver;

    fn frames(bytes: &[u8]) -> Vec<Frame> {
        let mut cursor = bytes;
        let mut out = Vec::new();
        while let Some(f) = read_msg::<_, Frame>(&mut cursor).unwrap() {
            out.push(f);
        }
        out
    }

    /// A job that finishes only when the test says so, so nothing below
    /// depends on how fast the machine is.
    fn gated(gate: Receiver<()>) -> impl FnOnce() -> u64 + Send + 'static {
        move || {
            gate.recv().unwrap();
            42
        }
    }

    #[test]
    fn a_quick_job_sends_nothing_extra() {
        let mut out = Vec::new();
        let done = run_with_keepalives(&mut out, Duration::from_secs(3600), || 7u64);
        assert_eq!(done.result.unwrap(), 7);
        assert!(done.connected);
        assert!(out.is_empty());
    }

    /// Writes into a shared buffer and lets the job go once a keepalive has
    /// been flushed, which proves one was sent while the job was still running.
    struct ReleaseOnFlush {
        written: Vec<u8>,
        release: Option<mpsc::Sender<()>>,
    }

    impl Write for ReleaseOnFlush {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if let Some(release) = self.release.take() {
                release.send(()).unwrap();
            }
            Ok(())
        }
    }

    #[test]
    fn a_slow_job_is_kept_alive_until_it_answers() {
        let (release, gate) = mpsc::channel();
        let mut out = ReleaseOnFlush {
            written: Vec::new(),
            release: Some(release),
        };
        let done = run_with_keepalives(&mut out, Duration::from_millis(1), gated(gate));

        assert_eq!(done.result.unwrap(), 42);
        assert!(done.connected);
        let sent = frames(&out.written);
        assert!(!sent.is_empty());
        assert!(sent.iter().all(|f| *f == Frame::Keepalive), "got {sent:?}");
    }

    /// Refuses every write, as a socket whose host has gone does.
    struct Gone {
        attempts: u32,
        release: Option<mpsc::Sender<()>>,
    }

    impl Write for Gone {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            self.attempts += 1;
            if let Some(release) = self.release.take() {
                release.send(()).unwrap();
            }
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_lost_host_stops_the_keepalives_but_not_the_wait() {
        let (release, gate) = mpsc::channel();
        let mut out = Gone {
            attempts: 0,
            release: Some(release),
        };
        let done = run_with_keepalives(&mut out, Duration::from_millis(1), gated(gate));

        assert_eq!(done.result.unwrap(), 42, "the job is still waited for");
        assert!(!done.connected);
        assert_eq!(out.attempts, 1, "nothing is written after a failed write");
    }

    #[test]
    fn a_panicking_job_is_an_error_not_a_hang() {
        let mut out = Vec::new();
        let done = run_with_keepalives(&mut out, Duration::from_secs(3600), || -> u64 {
            panic!("boom")
        });
        assert!(done.result.is_err());
    }
}

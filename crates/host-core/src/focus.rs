//! Focus: which folder the user has open, sent to the daemon as a hint.
//!
//! The first change is sent at once; changes after it are combined over
//! [`FOCUS_WINDOW`] and only the last is sent, so ten clicks in a second move
//! the phone's workers a few times, not ten. Every message carries the next
//! `seq`, so the daemon can discard one that arrives late.
//!
//! [`Focuser`] is the pure state machine, with time passed in so tests control
//! the clock. [`FocusDriver`] runs it on a thread with the real clock and sends
//! through a callback, never holding its lock while sending.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use socketsweep_wire::DirId;

use crate::lock;

/// How long focus changes are combined after one is sent.
pub const FOCUS_WINDOW: Duration = Duration::from_millis(150);

/// Where the phone should measure first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Dir(DirId),
    /// The root, or nothing: a plain parallel walk.
    Clear,
}

/// A focus message to send now: the target and its `seq`.
pub type FocusSend = (FocusTarget, u32);

/// The coalescing state machine.
#[derive(Debug)]
pub struct Focuser {
    window: Duration,
    /// What the daemon was last told. A scan starts with no focus.
    sent: FocusTarget,
    sent_at: Option<Instant>,
    pending: Option<FocusTarget>,
    seq: u32,
}

impl Focuser {
    pub fn new(window: Duration) -> Self {
        Focuser {
            window,
            sent: FocusTarget::Clear,
            sent_at: None,
            pending: None,
            seq: 0,
        }
    }

    /// The user opened `target` at `now`. Returns what to send at once, if
    /// anything; otherwise it is held until [`Focuser::deadline`].
    pub fn request(&mut self, target: FocusTarget, now: Instant) -> Option<FocusSend> {
        let quiet = self
            .sent_at
            .is_none_or(|at| now.saturating_duration_since(at) >= self.window);
        if self.pending.is_none() && quiet {
            if target == self.sent {
                return None;
            }
            return Some(self.send(target, now));
        }
        self.pending = Some(target);
        None
    }

    /// When the held change is due, if there is one.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending?;
        Some(
            self.sent_at
                .map_or_else(Instant::now, |at| at + self.window),
        )
    }

    /// Send the held change if it is due at `now`. A change back to what was
    /// last sent is dropped.
    pub fn poll(&mut self, now: Instant) -> Option<FocusSend> {
        let due = self.deadline()?;
        if now < due {
            return None;
        }
        let target = self.pending.take()?;
        if target == self.sent {
            return None;
        }
        Some(self.send(target, now))
    }

    /// The last `seq` sent, 0 before any.
    pub fn seq(&self) -> u32 {
        self.seq
    }

    fn send(&mut self, target: FocusTarget, now: Instant) -> FocusSend {
        self.seq += 1;
        self.sent = target;
        self.sent_at = Some(now);
        (target, self.seq)
    }
}

type Sender = Box<dyn Fn(FocusSend) + Send + Sync>;

struct Shared {
    state: Mutex<(Focuser, bool)>,
    changed: Condvar,
}

/// Runs a [`Focuser`] on a thread of its own, sending through `send`.
/// Stopped when dropped.
pub struct FocusDriver {
    shared: Arc<Shared>,
    send: Arc<Sender>,
}

impl FocusDriver {
    pub fn start(window: Duration, send: impl Fn(FocusSend) + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new((Focuser::new(window), false)),
            changed: Condvar::new(),
        });
        let send: Arc<Sender> = Arc::new(Box::new(send));
        let (s, f) = (Arc::clone(&shared), Arc::clone(&send));
        // If the thread cannot start, focus is only a hint: changes after
        // the first are lost, and the scan still completes.
        let _ = std::thread::Builder::new()
            .name("socketsweep-focus".into())
            .spawn(move || drive(s, f));
        FocusDriver { shared, send }
    }

    /// The user opened `target`.
    pub fn request(&self, target: FocusTarget) {
        let now = Instant::now();
        let out = lock(&self.shared.state).0.request(target, now);
        self.shared.changed.notify_all();
        if let Some(out) = out {
            (self.send)(out);
        }
    }

    pub fn stop(&self) {
        lock(&self.shared.state).1 = true;
        self.shared.changed.notify_all();
    }
}

impl Drop for FocusDriver {
    fn drop(&mut self) {
        self.stop();
    }
}

fn drive(shared: Arc<Shared>, send: Arc<Sender>) {
    let mut state = lock(&shared.state);
    loop {
        if state.1 {
            return;
        }
        match state.0.deadline() {
            None => {
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner());
            }
            Some(due) => {
                let now = Instant::now();
                if now >= due {
                    let out = state.0.poll(now);
                    drop(state);
                    if let Some(out) = out {
                        send(out);
                    }
                    state = lock(&shared.state);
                } else {
                    state = shared
                        .changed
                        .wait_timeout(state, due - now)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_first_change_goes_at_once_and_later_ones_are_combined() {
        let t0 = Instant::now();
        let mut f = Focuser::new(FOCUS_WINDOW);
        assert_eq!(
            f.request(FocusTarget::Dir(1), t0),
            Some((FocusTarget::Dir(1), 1))
        );

        // Three more clicks inside the window: held, the last one wins.
        assert_eq!(f.request(FocusTarget::Dir(2), t0 + ms(10)), None);
        assert_eq!(f.request(FocusTarget::Dir(3), t0 + ms(50)), None);
        assert_eq!(f.request(FocusTarget::Dir(4), t0 + ms(140)), None);
        assert_eq!(f.deadline(), Some(t0 + ms(150)));
        assert_eq!(f.poll(t0 + ms(149)), None);
        assert_eq!(f.poll(t0 + ms(150)), Some((FocusTarget::Dir(4), 2)));
        assert_eq!(f.deadline(), None);

        // Once the window has passed quietly, the next change goes at once.
        assert_eq!(
            f.request(FocusTarget::Clear, t0 + ms(400)),
            Some((FocusTarget::Clear, 3))
        );
    }

    #[test]
    fn a_change_inside_the_window_waits_even_after_a_flush() {
        let t0 = Instant::now();
        let mut f = Focuser::new(FOCUS_WINDOW);
        f.request(FocusTarget::Dir(1), t0);
        f.request(FocusTarget::Dir(2), t0 + ms(10));
        assert!(f.poll(t0 + ms(150)).is_some());
        // 50 ms after the flush: inside the new window.
        assert_eq!(f.request(FocusTarget::Dir(3), t0 + ms(200)), None);
        assert_eq!(f.deadline(), Some(t0 + ms(300)));
    }

    #[test]
    fn going_back_to_what_was_sent_sends_nothing() {
        let t0 = Instant::now();
        let mut f = Focuser::new(FOCUS_WINDOW);
        // The scan starts unfocused: the root is no change.
        assert_eq!(f.request(FocusTarget::Clear, t0), None);
        f.request(FocusTarget::Dir(1), t0);
        f.request(FocusTarget::Dir(2), t0 + ms(10));
        f.request(FocusTarget::Dir(1), t0 + ms(20));
        assert_eq!(f.poll(t0 + ms(200)), None);
        assert_eq!(f.seq(), 1);
        assert_eq!(f.request(FocusTarget::Dir(1), t0 + ms(500)), None);
    }

    #[test]
    fn the_driver_sends_the_held_change_when_due() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let driver = FocusDriver::start(ms(40), move |s| lock(&tx).send(s).unwrap());
        driver.request(FocusTarget::Dir(1));
        driver.request(FocusTarget::Dir(2));
        driver.request(FocusTarget::Dir(3));
        let got: Vec<_> = (0..2)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(got, [(FocusTarget::Dir(1), 1), (FocusTarget::Dir(3), 2)]);
        assert!(rx.recv_timeout(ms(150)).is_err());
        drop(driver);
    }
}

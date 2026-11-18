//! [`FocusDriver`]: the debouncer on a thread of its own.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::{FocusSend, FocusTarget, Focuser};
use crate::lock;

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
        let Some(due) = state.0.deadline() else {
            state = shared
                .changed
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
            continue;
        };
        let now = Instant::now();
        if now < due {
            state = shared
                .changed
                .wait_timeout(state, due - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
            continue;
        }
        let out = state.0.poll(now);
        drop(state);
        if let Some(out) = out {
            send(out);
        }
        state = lock(&shared.state);
    }
}

//! `pool`: the worker threads that drive a [`ScanState`].
//!
//! One mutex guards the state; workers loop `lock; pick; unlock; do task;
//! lock; apply; unlock`, and every event produced under the lock is handed
//! to the [`EventSink`] before the lock is released, which is what makes the
//! event order the order of state changes (G1–G3). A ticker thread wakes
//! the state periodically (skeleton budget, `Progress`, stalled
//! directories) while every worker is busy in a long task.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use socketsweep_wire::{DirId, Limits, Outcome, ScanOpts, Totals};

use crate::fs::{Fs, Interrupt, RootInfo};
use crate::sched::{Micros, Pick, ScanState, TierSnapshot};
use crate::sink::EventSink;

/// How long a worker waits before looking again when the sink is congested.
const CONGESTED_POLL: Duration = Duration::from_millis(10);
/// How long an idle worker waits for another's result before looking again.
const IDLE_POLL: Duration = Duration::from_millis(50);
/// How often the ticker thread wakes.
const TICK: Duration = Duration::from_millis(100);

/// `clamp(2 × cores, 4, 8)`: the default worker count, reported in
/// `Welcome::workers`.
pub fn default_workers() -> u8 {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    (2 * cores).clamp(4, 8) as u8
}

/// The worker count a scan with `opts` uses: `opts.workers`, or
/// [`default_workers`], within `1..=Limits::max_workers`.
pub fn workers_for(opts: &ScanOpts) -> u8 {
    opts.workers
        .unwrap_or_else(default_workers)
        .clamp(1, Limits::default().max_workers)
}

struct Shared<S> {
    state: Mutex<ScanState>,
    cv: Condvar,
    sink: S,
    cancel: AtomicBool,
    t0: Instant,
}

/// A running (or about to run) scan. Cheap to clone: every clone is a handle
/// on the same scan, so the session's reader thread can keep one to deliver
/// `Focus`, `ClearFocus`, `Cancel` and delete prunes while another thread is
/// inside [`Scan::run`].
pub struct Scan<S: EventSink> {
    shared: Arc<Shared<S>>,
}

impl<S: EventSink> Clone for Scan<S> {
    fn clone(&self) -> Self {
        Scan {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<S: EventSink> Scan<S> {
    /// Set up a scan of an already resolved root ([`Fs::root`]) with
    /// [`workers_for`]`(opts)` workers. Emits `Started` to `sink` at once.
    pub fn new(root: RootInfo, opts: &ScanOpts, sink: S) -> Self {
        Self::with_workers(root, opts, workers_for(opts), sink)
    }

    /// As [`Scan::new`] with an explicit worker count.
    pub fn with_workers(root: RootInfo, opts: &ScanOpts, workers: u8, sink: S) -> Self {
        let mut state = ScanState::new(root, opts, workers, 0);
        let started = state.take_events();
        let shared = Arc::new(Shared {
            state: Mutex::new(state),
            cv: Condvar::new(),
            sink,
            cancel: AtomicBool::new(false),
            t0: Instant::now(),
        });
        shared.sink.bulk(started);
        Scan { shared }
    }

    fn now(&self) -> Micros {
        self.shared.t0.elapsed().as_micros() as Micros
    }

    fn lock(&self) -> MutexGuard<'_, ScanState> {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Hand the events produced under `g` to the sink, still under the lock.
    fn flush(&self, g: &mut MutexGuard<'_, ScanState>) {
        if g.has_events() {
            self.shared.sink.bulk(g.take_events());
        }
    }

    /// Run `f` on the state under the lock, flush its events and wake the
    /// workers.
    fn update<R>(&self, f: impl FnOnce(&mut ScanState, Micros) -> R) -> R {
        let now = self.now();
        let mut g = self.lock();
        let r = f(&mut g, now);
        self.flush(&mut g);
        drop(g);
        self.shared.cv.notify_all();
        r
    }

    /// `ClientMsg::Focus`. Returns whether the focus changed (stale `seq`s
    /// and unknown or pruned directories are ignored).
    pub fn focus(&self, seq: u32, dir: DirId) -> bool {
        self.update(|s, now| s.set_focus(seq, dir, now))
    }

    /// `ClientMsg::ClearFocus`.
    pub fn clear_focus(&self, seq: u32) -> bool {
        self.update(|s, now| s.clear_focus(seq, now))
    }

    /// `ClientMsg::Cancel` or a lost connection: emits `Finished {
    /// Cancelled }`, interrupts the file system calls in flight, and makes
    /// [`Scan::run`] return.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.update(|s, now| s.cancel(now));
    }

    /// A `Delete` whose `hint` names directory `dir` of this scan, after the
    /// guard has confirmed the hint matches the resolved path. Call it
    /// **before** deleting anything: it emits `Pruned`, and the session then
    /// deletes and replies `Deleted`. Returns whether anything was pruned.
    pub fn prune(&self, dir: DirId) -> bool {
        self.update(|s, now| s.prune(dir, now))
    }

    pub fn is_finished(&self) -> bool {
        self.lock().is_finished()
    }

    /// How the scan ended, once it has.
    pub fn outcome(&self) -> Option<Outcome> {
        self.lock().outcome()
    }

    /// The root's totals so far.
    pub fn totals(&self) -> Totals {
        self.lock().root_totals()
    }

    /// The current allocation between tiers.
    pub fn tiers(&self) -> TierSnapshot {
        let now = self.now();
        self.lock().tiers(now)
    }

    /// The path of a directory this scan has introduced.
    pub fn path_of(&self, dir: DirId) -> Option<Vec<u8>> {
        let g = self.lock();
        (dir < g.known()).then(|| g.path_of(dir))
    }

    /// Walk the tree with `fs`: spawns the workers and a ticker, and returns
    /// when the scan has finished (complete or cancelled). Call it once.
    pub fn run<F: Fs + ?Sized>(&self, fs: &F) -> Outcome {
        let workers = self.lock().workers();
        std::thread::scope(|s| {
            for w in 0..usize::from(workers) {
                s.spawn(move || self.worker(fs, w));
            }
            s.spawn(|| self.ticker());
        });
        self.outcome().unwrap_or(Outcome::Cancelled)
    }

    fn worker<F: Fs + ?Sized>(&self, fs: &F, w: usize) {
        let intr = Interrupt::new(&self.shared.cancel);
        loop {
            let task = {
                let mut g = self.lock();
                loop {
                    if g.is_finished() {
                        return;
                    }
                    if self.shared.sink.congested() {
                        g = self.wait(g, CONGESTED_POLL);
                        continue;
                    }
                    let now = self.now();
                    let pick = g.pick(w, now);
                    self.flush(&mut g);
                    match pick {
                        Pick::Run(t) => break t,
                        Pick::Idle => g = self.wait(g, IDLE_POLL),
                        Pick::Exit => return,
                    }
                }
            };
            let output = task.run(fs, intr);
            self.update(|s, now| s.apply(w, output, now));
        }
    }

    fn ticker(&self) {
        let mut g = self.lock();
        while !g.is_finished() {
            let now = self.now();
            g.tick(now);
            self.flush(&mut g);
            g = self.wait(g, TICK);
        }
    }

    fn wait<'a>(&'a self, g: MutexGuard<'a, ScanState>, d: Duration) -> MutexGuard<'a, ScanState> {
        match self.shared.cv.wait_timeout(g, d) {
            Ok((g, _)) => g,
            Err(e) => e.into_inner().0,
        }
    }
}

//! The latency model and the shared "storage service" both simulated engines
//! run on.

use socketsweep_engine::{Micros, TaskKind, TaskOutput};

/// Costs, in microseconds of work at full speed. Separate costs for opening a
/// directory, reading one name, and stat'ing one entry; all of it slowed by
/// contention (see [`Storage`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Latency {
    /// Opening a directory (each task opens one handle).
    pub open: f64,
    /// Per name read (`getdents`).
    pub name: f64,
    /// Per `lstat`.
    pub stat: f64,
    /// Each extra concurrent request slows every request down by this
    /// fraction (the storage service handles few requests at once).
    pub contention: f64,
}

impl Latency {
    /// Roughly a phone's FUSE `/sdcard`: a 60,000-file directory takes about
    /// 6 s to stat.
    pub const STANDARD: Latency = Latency {
        open: 500.0,
        name: 3.0,
        stat: 100.0,
        contention: 0.15,
    };

    /// Unit costs and no contention, for small property-test trees.
    pub const UNIT: Latency = Latency {
        open: 50.0,
        name: 1.0,
        stat: 10.0,
        contention: 0.0,
    };

    /// Work for one task, from what it did.
    pub fn cost(&self, kind: TaskKind, out: &TaskOutput, stat_names: usize) -> f64 {
        let (names, stats) = match (kind, out) {
            (TaskKind::Names, TaskOutput::Listing(r)) => (
                r.entries.len(),
                r.entries.iter().filter(|e| e.stat.is_some()).count(),
            ),
            (TaskKind::Full, TaskOutput::Listing(r)) => (r.entries.len(), r.entries.len()),
            (TaskKind::StatDir, TaskOutput::Stats(_)) => (0, stat_names),
            _ => (0, 0),
        };
        self.open + names as f64 * self.name + stats as f64 * self.stat
    }

    /// The rate each of `busy` concurrent requests progresses at.
    pub fn rate(&self, busy: usize) -> f64 {
        1.0 / (1.0 + self.contention * (busy.max(1) - 1) as f64)
    }
}

/// Processor-sharing model of the storage service: every running task
/// progresses at [`Latency::rate`] of the number running, recomputed whenever
/// that number changes.
#[derive(Debug, Clone)]
pub struct Storage {
    lat: Latency,
    /// Remaining work per worker slot.
    remaining: Vec<Option<f64>>,
    /// Total work done, for utilisation figures.
    pub work_done: f64,
}

impl Storage {
    pub fn new(lat: Latency, workers: usize) -> Self {
        Storage {
            lat,
            remaining: vec![None; workers],
            work_done: 0.0,
        }
    }

    pub fn busy(&self) -> usize {
        self.remaining.iter().filter(|r| r.is_some()).count()
    }

    pub fn is_idle(&self) -> bool {
        self.busy() == 0
    }

    pub fn start(&mut self, w: usize, work: f64) {
        debug_assert!(self.remaining[w].is_none());
        self.remaining[w] = Some(work.max(1.0));
        self.work_done += work;
    }

    /// The next worker to finish and when, from `now`.
    pub fn next_done(&self, now: Micros) -> Option<(usize, Micros)> {
        let rate = self.lat.rate(self.busy());
        let (w, rem) = self
            .remaining
            .iter()
            .enumerate()
            .filter_map(|(w, r)| r.map(|r| (w, r)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))?;
        Some((w, now + (rem / rate).ceil().max(0.0) as Micros))
    }

    /// Let `dt` pass.
    pub fn advance(&mut self, dt: Micros) {
        let rate = self.lat.rate(self.busy());
        for r in self.remaining.iter_mut().flatten() {
            *r -= dt as f64 * rate;
        }
    }

    /// End worker `w`'s task.
    pub fn finish(&mut self, w: usize) {
        self.remaining[w] = None;
    }

    /// Latest time any running task can end if nothing else starts: the
    /// bound for "within one task length".
    pub fn horizon(&self, now: Micros) -> Micros {
        let rate = self.lat.rate(self.busy());
        self.remaining
            .iter()
            .flatten()
            .map(|r| now + (r / rate).ceil().max(0.0) as Micros)
            .max()
            .unwrap_or(now)
    }
}

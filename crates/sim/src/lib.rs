//! Dev-only simulation tools for the v4 engine:
//!
//! - [`SimFs`]: seeded `/sdcard`-shaped (or random) trees implementing the
//!   engine's `Fs`, with ground-truth totals and per-node call counters.
//! - [`Latency`] / [`Storage`]: separate costs for names and stats, slowed by
//!   a contention factor (processor sharing).
//! - [`run_v4`]: a discrete-event simulator with a virtual clock and W
//!   virtual workers driving `ScanState` directly, with navigation scripts
//!   and injected deletes.
//! - [`run_baseline`]: a jwalk-like engine (per-worker LIFO, steal oldest)
//!   on the same model, for comparison.
//! - [`Checker`]: an event-stream checker for G1–G3.
//! - [`SlowFs`]: wraps an `Fs` with real sleeps, for the threaded pool.
//! - [`FakeDaemon`]: the real daemon session over an in-memory pipe, walking
//!   a `SimFs` with the real engine, for host-side tests.
//!
//! `cargo run -p socketsweep-sim --example report --release` prints the
//! comparison on the standard tree.

pub mod checker;
pub mod des;
pub mod fake_daemon;
pub mod latency;
pub mod report;
pub mod rng;
pub mod script;
pub mod simfs;
pub mod slowfs;

pub use checker::{done_totals, Checker};
pub use des::{run_baseline, run_v4, Action, BaselineRun, FocusRecord, Script, SimConfig, V4Run};
pub use fake_daemon::{FakeDaemon, SimActions};
pub use latency::{Latency, Storage};
pub use report::{compare_on, compare_standard, format_report, Click, ScriptResult};
pub use rng::Rng;
pub use script::{largest_child, random_script, script_a, script_b, script_c};
pub use simfs::{NodeId, RandomShape, SimFs, TreeBuilder, Truth};
pub use slowfs::SlowFs;

/// FNV-1a over the postcard encoding of each event: a golden-trace hash.
pub fn trace_hash<'a>(events: impl IntoIterator<Item = &'a socketsweep_wire::ScanEvent>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for e in events {
        let bytes = postcard::to_allocvec(e).expect("encode event");
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

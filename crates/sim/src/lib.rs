//! Dev-only simulation tools for the engine:
//!
//! - [`SimFs`]: seeded `/sdcard`-shaped (or random) trees implementing the
//!   engine's `Fs`, with ground-truth totals and per-node call counters.
//! - [`Checker`]: an event-stream checker for G1–G3.
//! - [`SlowFs`]: wraps an `Fs` with real sleeps, for the threaded pool.
//! - [`FakeDaemon`]: the real daemon session over an in-memory pipe, walking
//!   a `SimFs` with the real engine, for host-side tests.

pub mod checker;
pub mod fake_daemon;
pub mod rng;
pub mod simfs;
pub mod slowfs;

pub use checker::{done_totals, Checker};
pub use fake_daemon::{FakeDaemon, SimActions};
pub use rng::Rng;
pub use simfs::{NodeId, RandomShape, SimFs, TreeBuilder, Truth};
pub use slowfs::SlowFs;

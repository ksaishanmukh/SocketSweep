//! The scan engine: scheduler, aggregator and worker pool. Host-portable; the
//! phone daemon supplies a rustix [`Fs`] and its session's [`EventSink`].
//!
//! # Contracts
//!
//! - [`Fs`]: read a directory's names, stat a directory's entries, or both
//!   at once. Byte paths, never following symbolic links.
//! - [`EventSink`]: the session's bulk and control queues. `bulk` is called
//!   under the scheduler lock and must not block.
//! - [`ScanState`]: the pure scheduler and aggregator. Inputs (picks, results,
//!   focus, prunes, ticks, with the time passed in) produce wire
//!   [`ScanEvent`](socketsweep_wire::ScanEvent)s at the moment the state
//!   changes, so a simulator can drive it directly.
//!
//! # Modes
//!
//! [`ScanOpts::mode`](socketsweep_wire::ScanOpts::mode) picks the scheduler.
//! `Focus` uses a skeleton phase, deferred stats and proximity allocation.
//! `Plain` is the benchmark baseline: no skeleton,
//! `Full` tasks only, and the focus is acknowledged (`FocusState`) but does
//! not steer the workers, which spread and descend as with no focus.
//!
//! # Running a scan (daemon)
//!
//! ```no_run
//! use socketsweep_engine::{Fs, RecordingSink, Scan, StdFs};
//! use socketsweep_wire::ScanOpts;
//!
//! let fs = StdFs; // the daemon passes its rustix Fs
//! let opts = ScanOpts::default();
//! // StartScan: resolve the root first; an error here is `Failed`, not a scan.
//! let root = fs.root(b"/sdcard").expect("root");
//! // The sink wraps events in ServerMsg::Scan frames under the scan's ReqId.
//! let scan = Scan::new(root, &opts, RecordingSink::new()); // emits Started
//! let handle = scan.clone(); // kept by the reader thread, keyed by ReqId
//! let runner = std::thread::spawn(move || scan.run(&fs)); // returns at Finished
//! handle.focus(1, 7); // ClientMsg::Focus { dir: 7, seq: 1 }
//! handle.clear_focus(2); // ClientMsg::ClearFocus { seq: 2 }
//! handle.prune(7); // Delete with hint (scan, 7), before deleting anything
//! handle.cancel(); // ClientMsg::Cancel or a lost connection
//! let outcome = runner.join().unwrap();
//! ```

pub mod aggregate;
pub mod alloc;
pub mod fs;
pub mod pool;
pub mod sched;
pub mod sink;
pub mod stdfs;

pub use fs::{errno, DirRead, Fs, FsEntry, FsError, FsStat, Interrupt, RootInfo, StatResult};
pub use pool::{default_workers, workers_for, Scan};
pub use sched::{
    Micros, Pick, ScanState, Task, TaskKind, TaskOutput, TierSnapshot, BIG_JOB_ENTRIES,
    PROGRESS_INTERVAL,
};
pub use sink::{EventSink, RecordingSink};
pub use stdfs::StdFs;

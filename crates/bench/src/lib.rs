//! Dev-only: the benchmark and the end-to-end test, run against a real phone
//! or an emulator over adb.
//!
//! - [`bench`]: the engine's focus mode against its plain mode (the
//!   baseline, `ScanMode::Plain`), with navigation scripts (a), (b) and (c),
//!   interleaved runs, a median/p90 table and JSON. It drives host-core's
//!   [`Client`](socketsweep_host_core::Client) directly, because it needs the
//!   raw events with their arrival times and a `ScanOpts` per scan.
//! - [`fixture`]: the seeded fixture tree (50,000 files, 3,000 directories,
//!   one 30,000-file directory), as a `SimFs` and as a tar archive pushed to
//!   the phone.
//! - [`e2e`]: connect, scan, focus, delete, copy and disconnect through
//!   host-core's [`App`](socketsweep_host_core::App), as the GUI does.
//!
//! The binary is `socketsweep-bench`; run it with `--help`.

pub mod bench;
pub mod device;
pub mod e2e;
pub mod fixture;
pub mod record;
pub mod summary;

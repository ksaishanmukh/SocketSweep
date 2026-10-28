//! The seeded fixture tree of `docs/architecture.md`, section 6: 50,000
//! files in 3,000 directories, one of which holds 30,000 files, shaped like
//! a small `/sdcard`.
//!
//! The tree is built with the simulator's [`TreeBuilder`] and [`Rng`], so the
//! same seed gives the same tree on the device (pushed as a tar archive and
//! extracted there), in [`SimFs`] for offline tests, and in the end-to-end
//! test's expectations. File contents are pseudo-random bytes derived from
//! the seed and the path, so a copy can be checked byte for byte without
//! keeping the archive.

mod build;
mod push;
mod tar;
#[cfg(test)]
mod tests;

pub use build::{build, content, expected, expected_below, rel_path};
pub use push::{measure_on_device, push};
pub use tar::write_tar;

/// The fixture's folder name, created under the parent (`/sdcard`).
pub const FIXTURE_DIR: &str = "ssfixture";
/// Where the archive is staged on the phone before extraction.
pub const DEVICE_TAR: &str = "/data/local/tmp/ssfixture.tar";

pub const FILES: u32 = 50_000;
/// Directories below the fixture root (the root itself not counted).
pub const DIRS: u32 = 3_000;
/// Files in the one big directory (`Movies/.thumbcache`).
pub const BIG_FILES: u32 = 30_000;
/// The levels of the `Documents/deep` chain.
const CHAIN: u32 = 30;

/// What a complete scan of the fixture must report, apart from the
/// directories' own sizes (which depend on the phone's file system).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    pub files: u32,
    pub dirs: u32,
    /// The sum of the files' lengths.
    pub file_bytes: u64,
}

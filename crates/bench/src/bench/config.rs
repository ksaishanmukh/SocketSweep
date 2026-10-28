//! How to run the benchmark.

use std::time::Duration;

use super::ScriptKind;

/// How to run the benchmark.
#[derive(Debug, Clone)]
pub struct Config {
    /// The scan root on the phone.
    pub root: Vec<u8>,
    /// Measured runs per script and mode.
    pub runs: usize,
    /// Unmeasured runs per mode before the measured ones (the first scan
    /// also chooses the scripts' folders).
    pub warmup: usize,
    pub scripts: Vec<ScriptKind>,
    pub workers: Option<u8>,
    /// Seed of script (c).
    pub seed: u64,
    /// Longest one scan may take.
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            root: b"/sdcard".to_vec(),
            runs: 10,
            warmup: 1,
            scripts: vec![ScriptKind::A, ScriptKind::B, ScriptKind::C],
            workers: None,
            seed: 1,
            timeout: Duration::from_secs(600),
        }
    }
}

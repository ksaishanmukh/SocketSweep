//! A discrete-event simulator: a virtual clock and W virtual workers driving
//! [`ScanState`] directly, on a [`SimFs`] with a [`Latency`] model.

mod baseline;
mod ids;
mod v4;

use std::collections::HashMap;

use socketsweep_engine::Micros;
use socketsweep_wire::{DirId, Outcome, ScanEvent, ScanOpts, Totals};

use crate::latency::Latency;

pub use baseline::{run_baseline, BaselineRun};
pub use v4::run_v4;

/// One scripted action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open the folder at this path (skipped if not yet introduced).
    Focus(Vec<u8>),
    ClearFocus,
    /// Delete the directory at this path, with a prune hint (skipped if it
    /// has no `DirId` yet).
    Delete(Vec<u8>),
    Cancel,
}

/// Actions at virtual times (microseconds from the scan start).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    pub steps: Vec<(Micros, Action)>,
}

/// Simulator settings.
#[derive(Debug, Clone)]
pub struct SimConfig {
    pub workers: u8,
    pub opts: ScanOpts,
    pub latency: Latency,
    /// Virtual time between ticks.
    pub tick: Micros,
    /// Check the tier properties (Back floor, convergence to targets).
    pub check_tiers: bool,
    /// Run [`ScanState::validate`] every this many steps (0: never).
    pub validate_every: usize,
}

impl SimConfig {
    pub fn standard() -> Self {
        SimConfig {
            workers: 8,
            opts: ScanOpts::default(),
            latency: Latency::STANDARD,
            tick: 10_000,
            check_tiers: true,
            validate_every: 0,
        }
    }
}

/// What happened to one focus action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRecord {
    pub seq: u32,
    pub path: Vec<u8>,
    /// `None`: the folder was not introduced yet, so the action was skipped.
    pub dir: Option<DirId>,
    pub at: Micros,
    pub settled_at: Option<Micros>,
}

/// The result of a simulated v4 scan.
#[derive(Debug, Clone, Default)]
pub struct V4Run {
    pub events: Vec<(Micros, ScanEvent)>,
    pub finished_at: Micros,
    pub outcome: Option<Outcome>,
    pub totals: Totals,
    pub skeleton_at: Option<Micros>,
    /// `SubtreeDone` time per path.
    pub done_at: HashMap<Vec<u8>, Micros>,
    pub focus: Vec<FocusRecord>,
    pub deleted: Vec<Vec<u8>>,
    pub deletes_skipped: usize,
    /// Tier-property violations (empty when `check_tiers` is off).
    pub violations: Vec<String>,
    /// Instants at which the tier-convergence property was checked (after
    /// a ramp), so tests can tell it was exercised.
    pub tier_samples: u64,
    /// Tasks run, per kind: names, statdir, full.
    pub tasks: [u64; 3],
    /// Total work given to the storage service, in microseconds at full
    /// speed.
    pub work: f64,
}

impl V4Run {
    pub fn events(&self) -> impl Iterator<Item = &ScanEvent> {
        self.events.iter().map(|(_, e)| e)
    }
}

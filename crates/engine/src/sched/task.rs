//! What a worker is handed and what it gives back.

use socketsweep_wire::{DirId, FsOp, Tier};

#[cfg(doc)]
use super::ScanState;
use crate::fs::{DirRead, Fs, FsError, Interrupt, StatResult};

/// What a task does to its directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    /// `getdents` + `d_type` only (skeleton listing); the stats are owed.
    Names,
    /// The deferred stats of a directory listed by `Names`.
    StatDir,
    /// `getdents` and `lstat` together.
    Full,
}

/// A unit of work handed to a worker by [`ScanState::pick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub dir: DirId,
    pub kind: TaskKind,
    /// The tier the task was taken from.
    pub tier: Tier,
    /// The directory's full path.
    pub path: Vec<u8>,
    /// [`TaskKind::StatDir`] only: the names to stat, in order.
    pub names: Vec<Vec<u8>>,
}

impl Task {
    /// Run the task against `fs`. This is the "do task" step of the worker
    /// loop, done without the lock.
    pub fn run<F: Fs + ?Sized>(&self, fs: &F, intr: Interrupt<'_>) -> TaskOutput {
        match self.kind {
            TaskKind::Names => fs.read_names(&self.path, intr).into(),
            TaskKind::Full => fs.list_full(&self.path, intr).into(),
            TaskKind::StatDir => match fs.stat_all(&self.path, &self.names, intr) {
                Ok(v) => TaskOutput::Stats(v),
                Err(FsError::Failed { op, errno }) => TaskOutput::Failed { op, errno },
                Err(FsError::Interrupted) => TaskOutput::Interrupted,
            },
        }
    }
}

/// The result of a [`Task`], given back to [`ScanState::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutput {
    /// `Names` or `Full`.
    Listing(DirRead),
    /// `StatDir`: one result per name, in the task's order.
    Stats(Vec<StatResult>),
    Failed {
        op: FsOp,
        errno: i32,
    },
    /// The interrupt was raised (the scan was cancelled).
    Interrupted,
}

impl From<Result<DirRead, FsError>> for TaskOutput {
    fn from(r: Result<DirRead, FsError>) -> Self {
        match r {
            Ok(d) => TaskOutput::Listing(d),
            Err(FsError::Failed { op, errno }) => TaskOutput::Failed { op, errno },
            Err(FsError::Interrupted) => TaskOutput::Interrupted,
        }
    }
}

/// What [`ScanState::pick`] tells a worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Run(Task),
    /// Nothing runnable right now: wait for another worker's result.
    Idle,
    /// The scan has finished or was cancelled: exit.
    Exit,
}

//! `SlowFs`: an [`Fs`] wrapper that adds real delays, so the threaded pool
//! sees overlapping tasks.

use std::time::Duration;

use socketsweep_engine::{DirRead, Fs, FsError, Interrupt, RootInfo, StatResult};

#[derive(Debug)]
pub struct SlowFs<F> {
    pub inner: F,
    /// Added to every directory read or stat batch.
    pub per_call: Duration,
    /// Added per entry read or stat'ed.
    pub per_entry: Duration,
}

impl<F: Fs> SlowFs<F> {
    pub fn new(inner: F, per_call: Duration, per_entry: Duration) -> Self {
        SlowFs {
            inner,
            per_call,
            per_entry,
        }
    }

    fn pause(&self, entries: usize) {
        let d = self.per_call + self.per_entry * entries as u32;
        if !d.is_zero() {
            std::thread::sleep(d);
        }
    }
}

impl<F: Fs> Fs for SlowFs<F> {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        self.inner.root(path)
    }

    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        let r = self.inner.read_names(dir, intr);
        self.pause(r.as_ref().map_or(0, |r| r.entries.len()));
        r
    }

    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError> {
        let r = self.inner.stat_all(dir, names, intr);
        self.pause(names.len());
        r
    }

    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        let r = self.inner.list_full(dir, intr);
        self.pause(r.as_ref().map_or(0, |r| r.entries.len()));
        r
    }
}

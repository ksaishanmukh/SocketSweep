//! An event-stream checker for the ordering guarantees G1–G3 (spec section
//! 2), written from the host's point of view: it accepts exactly the streams
//! a host without a reorder buffer can build its tree from.

mod done_totals;
mod handlers;

use socketsweep_wire::{DirId, Outcome, ScanEvent};

pub use done_totals::done_totals;

#[derive(Debug, Clone, Default)]
pub(super) struct CDir {
    pub(super) known: bool,
    pub(super) parent: Option<DirId>,
    pub(super) parts: u16,
    pub(super) listed: bool,
    pub(super) failed: bool,
    /// Positions whose stat is owed (stat `None` in the listing) and not yet
    /// delivered.
    pub(super) owed: Vec<bool>,
    pub(super) owed_left: u32,
    pub(super) entries: u32,
    pub(super) open_children: u32,
    pub(super) done: bool,
    pub(super) pruned: bool,
}

/// Feed every event in order with [`Checker::feed`], then call
/// [`Checker::finish`].
#[derive(Debug, Default)]
pub struct Checker {
    pub(super) dirs: Vec<CDir>,
    pub(super) started: bool,
    pub(super) finished: Option<Outcome>,
    count: usize,
}

impl Checker {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn dir(&mut self, d: DirId) -> Result<&mut CDir, String> {
        let r = self
            .dirs
            .get_mut(d as usize)
            .filter(|r| r.known)
            .ok_or_else(|| format!("G1: dir {d} referenced before it was introduced"))?;
        if r.pruned {
            return Err(format!("dir {d} referenced after it was pruned"));
        }
        Ok(r)
    }

    fn dead(&self, mut d: DirId) -> bool {
        loop {
            let r = &self.dirs[d as usize];
            if r.pruned {
                return true;
            }
            match r.parent {
                Some(p) => d = p,
                None => return false,
            }
        }
    }

    pub(super) fn live(&mut self, d: DirId) -> Result<&mut CDir, String> {
        self.dir(d)?;
        if self.dead(d) {
            return Err(format!("dir {d} referenced inside a pruned subtree"));
        }
        self.dir(d)
    }

    pub(super) fn introduce(&mut self, d: DirId, parent: Option<DirId>) -> Result<(), String> {
        let i = d as usize;
        if self.dirs.len() <= i {
            self.dirs.resize_with(i + 1, Default::default);
        }
        if self.dirs[i].known {
            return Err(format!("G3: dir {d} introduced twice"));
        }
        self.dirs[i] = CDir {
            known: true,
            parent,
            ..Default::default()
        };
        Ok(())
    }

    pub fn feed(&mut self, ev: &ScanEvent) -> Result<(), String> {
        self.count += 1;
        let n = self.count;
        self.feed_inner(ev)
            .map_err(|e| format!("event #{n} {}: {e}", short(ev)))
    }

    fn feed_inner(&mut self, ev: &ScanEvent) -> Result<(), String> {
        if self.finished.is_some() {
            return Err("event after Finished".into());
        }
        if !self.started && !matches!(ev, ScanEvent::Started { .. }) {
            return Err("first event is not Started".into());
        }
        match ev {
            ScanEvent::Started { .. } => self.on_started()?,
            ScanEvent::Listing {
                dir,
                part,
                last,
                entries,
            } => self.on_listing(*dir, *part, *last, entries)?,
            ScanEvent::Stats { dir, start, stats } => self.on_stats(*dir, *start, stats.len())?,
            ScanEvent::DirFailed { dir, .. } => self.on_dir_failed(*dir)?,
            ScanEvent::SubtreeDone { dir, .. } => self.on_subtree_done(*dir)?,
            ScanEvent::Pruned { dir } => self.on_pruned(*dir)?,
            ScanEvent::FocusState { dir, .. } => {
                self.live(*dir)?;
            }
            ScanEvent::Progress { stalled, .. } => {
                for (d, _) in stalled {
                    self.dir(*d)?;
                }
            }
            ScanEvent::SkeletonReady { .. } => {}
            ScanEvent::Finished { outcome, .. } => self.on_finished(*outcome)?,
        }
        Ok(())
    }

    /// The stream ended: it must have ended with `Finished`, and a complete
    /// scan must have finished or pruned every directory it introduced.
    pub fn finish(&self) -> Result<(), String> {
        let Some(outcome) = self.finished else {
            return Err("no Finished".into());
        };
        if outcome == Outcome::Complete {
            for (i, r) in self.dirs.iter().enumerate() {
                if r.known && !r.done && !self.dead(i as DirId) {
                    return Err(format!("G3: dir {i} never finished"));
                }
            }
        }
        Ok(())
    }

    /// Check a whole stream.
    pub fn check<'a>(events: impl IntoIterator<Item = &'a ScanEvent>) -> Result<(), String> {
        let mut c = Checker::new();
        for e in events {
            c.feed(e)?;
        }
        c.finish()
    }
}

fn short(ev: &ScanEvent) -> String {
    match ev {
        ScanEvent::Listing {
            dir,
            part,
            last,
            entries,
        } => format!(
            "Listing(dir {dir}, part {part}, last {last}, {} entries)",
            entries.len()
        ),
        ScanEvent::Stats { dir, start, stats } => {
            format!("Stats(dir {dir}, {start}+{})", stats.len())
        }
        e => format!("{e:?}"),
    }
}

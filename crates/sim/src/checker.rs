//! An event-stream checker for the ordering guarantees G1–G3 (spec section
//! 2), written from the host's point of view: it accepts exactly the streams
//! a host without a reorder buffer can build its tree from.

use socketsweep_wire::{DirId, Outcome, ScanEvent, ROOT_DIR};

#[derive(Debug, Clone, Default)]
struct CDir {
    known: bool,
    parent: Option<DirId>,
    parts: u16,
    listed: bool,
    failed: bool,
    /// Positions whose stat is owed (stat `None` in the listing) and not yet
    /// delivered.
    owed: Vec<bool>,
    owed_left: u32,
    entries: u32,
    open_children: u32,
    done: bool,
    pruned: bool,
}

/// Feed every event in order with [`Checker::feed`], then call
/// [`Checker::finish`].
#[derive(Debug, Default)]
pub struct Checker {
    dirs: Vec<CDir>,
    started: bool,
    finished: Option<Outcome>,
    count: usize,
}

impl Checker {
    pub fn new() -> Self {
        Self::default()
    }

    fn dir(&mut self, d: DirId) -> Result<&mut CDir, String> {
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

    fn live(&mut self, d: DirId) -> Result<&mut CDir, String> {
        self.dir(d)?;
        if self.dead(d) {
            return Err(format!("dir {d} referenced inside a pruned subtree"));
        }
        self.dir(d)
    }

    fn introduce(&mut self, d: DirId, parent: Option<DirId>) -> Result<(), String> {
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
            ScanEvent::Started { .. } => {
                if self.started {
                    return Err("Started twice".into());
                }
                self.started = true;
                self.introduce(ROOT_DIR, None)?;
            }
            ScanEvent::Listing {
                dir,
                part,
                last,
                entries,
            } => {
                let r = self.live(*dir)?;
                if r.listed || r.failed {
                    return Err("G3: listed twice".into());
                }
                if *part != r.parts {
                    return Err(format!("G2: part {part}, expected {}", r.parts));
                }
                r.parts += 1;
                for e in entries.iter() {
                    let owed = e.stat.is_none();
                    r.owed.push(owed);
                    r.owed_left += u32::from(owed);
                }
                r.entries += entries.len() as u32;
                r.listed = *last;
                let kids: Vec<DirId> = entries.iter().filter_map(|e| e.id).collect();
                self.dirs[*dir as usize].open_children += kids.len() as u32;
                for k in kids {
                    self.introduce(k, Some(*dir))?;
                }
            }
            ScanEvent::Stats { dir, start, stats } => {
                let r = self.live(*dir)?;
                if !r.listed {
                    return Err("G2: Stats before the last Listing part".into());
                }
                let end = *start as usize + stats.len();
                if end > r.owed.len() {
                    return Err(format!(
                        "Stats range {start}..{end} beyond {} entries",
                        r.owed.len()
                    ));
                }
                for i in *start as usize..end {
                    if !r.owed[i] {
                        return Err(format!("stat of entry {i} delivered twice or not owed"));
                    }
                    r.owed[i] = false;
                    r.owed_left -= 1;
                }
            }
            ScanEvent::DirFailed { dir, .. } => {
                let r = self.live(*dir)?;
                if r.listed || r.parts > 0 || r.failed {
                    return Err("DirFailed after a listing".into());
                }
                r.failed = true;
            }
            ScanEvent::SubtreeDone { dir, .. } => {
                let r = self.live(*dir)?;
                if r.done {
                    return Err("G3: finished twice".into());
                }
                if !(r.listed || r.failed) {
                    return Err("G2: SubtreeDone before its listing".into());
                }
                if r.owed_left > 0 {
                    return Err(format!("G2: SubtreeDone with {} stats owed", r.owed_left));
                }
                if r.open_children > 0 {
                    return Err(format!(
                        "G2: SubtreeDone with {} children open",
                        r.open_children
                    ));
                }
                r.done = true;
                if let Some(p) = r.parent {
                    let pr = &mut self.dirs[p as usize];
                    if pr.owed_left > 0 {
                        return Err(format!("G2: child finished before Stats of its parent {p}"));
                    }
                    pr.open_children -= 1;
                }
            }
            ScanEvent::Pruned { dir } => {
                if *dir == ROOT_DIR {
                    return Err("root pruned".into());
                }
                let r = self.live(*dir)?;
                let was_done = r.done;
                r.pruned = true;
                if !was_done {
                    if let Some(p) = r.parent {
                        self.dirs[p as usize].open_children -= 1;
                    }
                }
            }
            ScanEvent::FocusState { dir, .. } => {
                self.live(*dir)?;
            }
            ScanEvent::Progress { stalled, .. } => {
                for (d, _) in stalled {
                    self.dir(*d)?;
                }
            }
            ScanEvent::SkeletonReady { .. } => {}
            ScanEvent::Finished { outcome, .. } => {
                if *outcome == Outcome::Complete {
                    let root = &self.dirs[0];
                    if !root.done {
                        return Err("Finished(Complete) before the root's SubtreeDone".into());
                    }
                }
                self.finished = Some(*outcome);
            }
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

/// The totals of every `SubtreeDone` in a stream, by path (built from the
/// `Started` root and the listings, as a host would).
pub fn done_totals<'a>(
    events: impl IntoIterator<Item = &'a ScanEvent>,
) -> Vec<(Vec<u8>, socketsweep_wire::Totals)> {
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::new();
    for e in events {
        match e {
            ScanEvent::Started { root, .. } => paths = vec![root.clone()],
            ScanEvent::Listing { dir, entries, .. } => {
                let base = paths[*dir as usize].clone();
                for en in entries {
                    if let Some(id) = en.id {
                        let mut p = base.clone();
                        if p.last() != Some(&b'/') {
                            p.push(b'/');
                        }
                        p.extend_from_slice(&en.name);
                        if paths.len() <= id as usize {
                            paths.resize(id as usize + 1, Vec::new());
                        }
                        paths[id as usize] = p;
                    }
                }
            }
            ScanEvent::SubtreeDone { dir, totals } => {
                out.push((paths[*dir as usize].clone(), *totals));
            }
            _ => {}
        }
    }
    out
}

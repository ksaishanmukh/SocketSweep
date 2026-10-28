//! One handler per event kind: the rules of G1–G3.

use socketsweep_wire::{DirId, Entry, Outcome, ROOT_DIR};

use super::Checker;

impl Checker {
    pub(super) fn on_started(&mut self) -> Result<(), String> {
        if self.started {
            return Err("Started twice".into());
        }
        self.started = true;
        self.introduce(ROOT_DIR, None)
    }

    pub(super) fn on_listing(
        &mut self,
        dir: DirId,
        part: u16,
        last: bool,
        entries: &[Entry],
    ) -> Result<(), String> {
        let r = self.live(dir)?;
        if r.listed || r.failed {
            return Err("G3: listed twice".into());
        }
        if part != r.parts {
            return Err(format!("G2: part {part}, expected {}", r.parts));
        }
        r.parts += 1;
        for e in entries.iter() {
            let owed = e.stat.is_none();
            r.owed.push(owed);
            r.owed_left += u32::from(owed);
        }
        r.entries += entries.len() as u32;
        r.listed = last;
        let kids: Vec<DirId> = entries.iter().filter_map(|e| e.id).collect();
        self.dirs[dir as usize].open_children += kids.len() as u32;
        for k in kids {
            self.introduce(k, Some(dir))?;
        }
        Ok(())
    }

    pub(super) fn on_stats(&mut self, dir: DirId, start: u32, count: usize) -> Result<(), String> {
        let r = self.live(dir)?;
        if !r.listed {
            return Err("G2: Stats before the last Listing part".into());
        }
        let end = start as usize + count;
        if end > r.owed.len() {
            return Err(format!(
                "Stats range {start}..{end} beyond {} entries",
                r.owed.len()
            ));
        }
        for i in start as usize..end {
            if !r.owed[i] {
                return Err(format!("stat of entry {i} delivered twice or not owed"));
            }
            r.owed[i] = false;
            r.owed_left -= 1;
        }
        Ok(())
    }

    pub(super) fn on_dir_failed(&mut self, dir: DirId) -> Result<(), String> {
        let r = self.live(dir)?;
        if r.listed || r.parts > 0 || r.failed {
            return Err("DirFailed after a listing".into());
        }
        r.failed = true;
        Ok(())
    }

    pub(super) fn on_subtree_done(&mut self, dir: DirId) -> Result<(), String> {
        let r = self.live(dir)?;
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
        Ok(())
    }

    pub(super) fn on_pruned(&mut self, dir: DirId) -> Result<(), String> {
        if dir == ROOT_DIR {
            return Err("root pruned".into());
        }
        let r = self.live(dir)?;
        let was_done = r.done;
        r.pruned = true;
        if !was_done {
            if let Some(p) = r.parent {
                self.dirs[p as usize].open_children -= 1;
            }
        }
        Ok(())
    }

    pub(super) fn on_finished(&mut self, outcome: Outcome) -> Result<(), String> {
        if outcome == Outcome::Complete {
            let root = &self.dirs[0];
            if !root.done {
                return Err("Finished(Complete) before the root's SubtreeDone".into());
            }
        }
        self.finished = Some(outcome);
        Ok(())
    }
}

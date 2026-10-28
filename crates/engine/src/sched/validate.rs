//! A from-scratch recount of the table's counters, for tests.

use socketsweep_wire::DirId;

use super::{ScanState, St, StatSt, NONE};

/// The counters recomputed from the table, one slot per directory.
struct Recount {
    pending: Vec<u32>,
    active: Vec<u32>,
    open: Vec<u32>,
}

impl ScanState {
    /// Recompute every counter from scratch and compare (tests).
    pub fn validate(&self) -> Result<(), String> {
        let recount = self.recount();
        for (i, r) in self.dirs.iter().enumerate() {
            if r.state == St::Pruned {
                continue;
            }
            if r.pending_desc != recount.pending[i] {
                return Err(format!(
                    "dir {i}: pending_desc {} != {}",
                    r.pending_desc, recount.pending[i]
                ));
            }
            if u32::from(r.active_desc) != recount.active[i] {
                return Err(format!(
                    "dir {i}: active_desc {} != {}",
                    r.active_desc, recount.active[i]
                ));
            }
            let listed = !matches!(r.state, St::Pending | St::InFlight);
            if listed && r.open_children != recount.open[i] {
                return Err(format!(
                    "dir {i}: open_children {} != {}",
                    r.open_children, recount.open[i]
                ));
            }
        }
        let unlisted = self
            .dirs
            .iter()
            .filter(|r| matches!(r.state, St::Pending | St::InFlight))
            .count() as u32;
        if unlisted != self.unlisted {
            return Err(format!("unlisted {} != {unlisted}", self.unlisted));
        }
        Ok(())
    }

    fn recount(&self) -> Recount {
        let n = self.dirs.len();
        let mut pending = vec![0u32; n];
        let mut active = vec![0u32; n];
        let mut open = vec![0u32; n];
        for (i, r) in self.dirs.iter().enumerate() {
            if r.state == St::Pruned {
                continue;
            }
            let mut own = u32::from(r.state == St::Pending);
            own += u32::from(r.state == St::NamesOnly && r.stat == StatSt::Pending);
            let mut x = i as DirId;
            while x != NONE {
                pending[x as usize] += own;
                x = self.dirs[x as usize].parent;
            }
            if r.parent != NONE && r.state != St::Done {
                open[r.parent as usize] += 1;
            }
        }
        for w in &self.workers {
            if let Some(t) = w.task {
                let mut x = t.dir;
                while x != NONE {
                    active[x as usize] += 1;
                    x = self.dirs[x as usize].parent;
                }
            }
        }
        Recount {
            pending,
            active,
            open,
        }
    }
}

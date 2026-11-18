//! What the engine did to a [`SimFs`], and what it should have done.

use std::sync::atomic::Ordering;

use socketsweep_wire::{Kind, Totals};

use super::{NodeId, SimFs};

#[derive(Debug, Clone)]
pub struct Truth {
    /// Totals of each directory the engine walks (`None` otherwise).
    pub totals: Vec<Option<Totals>>,
    /// The engine must read this directory exactly once.
    pub listed: Vec<bool>,
    /// The engine must stat this entry exactly once.
    pub stated: Vec<bool>,
    /// Entries of a directory on another file system: the engine reads that
    /// directory once before it can see its device, so they may have been
    /// stat'ed once (or not at all).
    pub maybe: Vec<bool>,
}

impl Truth {
    pub fn root(&self) -> Totals {
        self.totals[0].expect("root is walked")
    }
}

impl SimFs {
    /// Times each node was read as a directory.
    pub fn lists(&self, id: NodeId) -> u32 {
        self.lists[id as usize].load(Ordering::Relaxed)
    }

    /// Times each node was stat'ed.
    pub fn stats(&self, id: NodeId) -> u32 {
        self.stats[id as usize].load(Ordering::Relaxed)
    }

    pub fn reset_counters(&self) {
        for c in self.lists.iter().chain(&self.stats) {
            c.store(0, Ordering::Relaxed);
        }
    }

    /// Directories in a live subtree, itself included.
    pub fn subtree_dirs(&self, id: NodeId) -> u64 {
        let mut n = 0;
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            n += 1;
            stack.extend(
                self.children(x)
                    .filter(|&c| self.nodes[c as usize].kind == Kind::Dir),
            );
        }
        n
    }

    /// Disk usage of a live subtree, itself excluded.
    pub fn subtree_disk(&self, id: NodeId) -> u64 {
        let mut n = 0;
        let mut stack: Vec<NodeId> = self.children(id).collect();
        while let Some(x) = stack.pop() {
            n += self.nodes[x as usize].blocks * 512;
            if self.nodes[x as usize].kind == Kind::Dir {
                stack.extend(self.children(x));
            }
        }
        n
    }

    /// Check the counters against `truth`: every directory the engine had to
    /// read was read exactly once and every entry it had to stat was stat'ed
    /// exactly once, and nothing else at all. Nodes deleted during the scan
    /// may have been touched at most once.
    pub fn check_counters(&self, truth: &Truth) -> Result<(), String> {
        for id in 0..self.nodes.len() {
            let (l, s) = (self.lists(id as NodeId), self.stats(id as NodeId));
            let path = || String::from_utf8_lossy(&self.path_of(id as NodeId)).into_owned();
            // Deleted during the scan, or below a mount point: touched at
            // most once.
            let loose = self.is_deleted(id as NodeId) || truth.maybe[id];
            if loose && (l > 1 || s > 1) {
                return Err(format!(
                    "{} (deleted): listed {l}, stat'ed {s} times",
                    path()
                ));
            }
            if loose {
                continue;
            }
            if l != u32::from(truth.listed[id]) {
                return Err(format!(
                    "{}: listed {l} times (expected {})",
                    path(),
                    u8::from(truth.listed[id])
                ));
            }
            if s != u32::from(truth.stated[id]) {
                return Err(format!(
                    "{}: stat'ed {s} times (expected {})",
                    path(),
                    u8::from(truth.stated[id])
                ));
            }
        }
        Ok(())
    }
}

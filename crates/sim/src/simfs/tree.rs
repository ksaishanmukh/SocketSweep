//! [`SimFs`] and [`Truth`]: the tree, its accessors, deletes and counters.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use socketsweep_wire::{Kind, Totals};

use super::{Node, NodeId};

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

#[derive(Debug)]
pub struct SimFs {
    pub(super) root_path: Vec<u8>,
    pub(super) nodes: Vec<Node>,
    pub(super) paths: HashMap<Vec<u8>, NodeId>,
    pub(super) deleted: Vec<AtomicBool>,
    pub(super) lists: Vec<AtomicU32>,
    pub(super) stats: Vec<AtomicU32>,
}

impl SimFs {
    pub fn root_path(&self) -> &[u8] {
        &self.root_path
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn is_deleted(&self, id: NodeId) -> bool {
        self.deleted[id as usize].load(Ordering::Relaxed)
    }

    /// Live children of a directory.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id as usize]
            .children
            .iter()
            .copied()
            .filter(|&c| !self.is_deleted(c))
    }

    pub fn path_of(&self, id: NodeId) -> Vec<u8> {
        let mut parts = Vec::new();
        let mut x = id;
        while x != 0 {
            parts.push(&self.nodes[x as usize].name);
            x = self.nodes[x as usize].parent;
        }
        let mut p = self.root_path.clone();
        for n in parts.iter().rev() {
            if p.last() != Some(&b'/') {
                p.push(b'/');
            }
            p.extend_from_slice(n);
        }
        p
    }

    /// A live directory by path.
    pub fn lookup(&self, path: &[u8]) -> Option<NodeId> {
        self.paths
            .get(path)
            .copied()
            .filter(|&d| !self.is_deleted(d))
    }

    /// A live node of any kind by path.
    pub fn find(&self, path: &[u8]) -> Option<NodeId> {
        if let Some(d) = self.lookup(path) {
            return Some(d);
        }
        let slash = path.iter().rposition(|&b| b == b'/')?;
        let (parent, name) = (&path[..slash], &path[slash + 1..]);
        let parent = if parent.is_empty() {
            b"/" as &[u8]
        } else {
            parent
        };
        let p = self.lookup(parent)?;
        let kids = &self.nodes[p as usize].children;
        kids.binary_search_by(|&c| self.nodes[c as usize].name.as_slice().cmp(name))
            .ok()
            .map(|i| kids[i])
            .filter(|&c| !self.is_deleted(c))
    }

    /// Delete a file, or a directory and everything below it. Returns
    /// whether it existed. The root is never deleted.
    pub fn delete(&self, path: &[u8]) -> bool {
        let Some(d) = self.find(path) else {
            return false;
        };
        if d == 0 {
            return false;
        }
        let mut stack = vec![d];
        while let Some(x) = stack.pop() {
            self.deleted[x as usize].store(true, Ordering::Relaxed);
            stack.extend(self.nodes[x as usize].children.iter().copied());
        }
        true
    }

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
            if self.is_deleted(id as NodeId) || truth.maybe[id] {
                // Deleted during the scan, or below a mount point: touched at
                // most once.
                if l > 1 || s > 1 {
                    return Err(format!(
                        "{} (deleted): listed {l}, stat'ed {s} times",
                        path()
                    ));
                }
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

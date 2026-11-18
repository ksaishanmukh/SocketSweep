//! [`SimFs`]: the tree, its accessors and deletes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::{Node, NodeId};

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
}

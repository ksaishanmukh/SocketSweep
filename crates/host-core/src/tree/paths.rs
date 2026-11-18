//! Paths of nodes, and which of a selection is outermost.

use std::collections::HashSet;

use socketsweep_gui_contract::{NodeId, NodeRef};

use super::{Tree, TreeError, NONE, ROOT};

impl Tree {
    /// The byte-exact phone path of `id`, built from the resolved root.
    pub fn path_of(&self, id: NodeId) -> Result<Vec<u8>, TreeError> {
        self.live(id)?;
        let mut out = self.root_path.clone();
        for name in self.names_from_root(id) {
            if out.last() != Some(&b'/') {
                out.push(b'/');
            }
            out.extend_from_slice(name);
        }
        Ok(out)
    }

    /// The path to delete for `id`: a live node of this tree, not the root.
    pub fn delete_target(&self, node: NodeRef) -> Result<Vec<u8>, TreeError> {
        let id = self.resolve(node)?;
        if id == ROOT {
            return Err(TreeError::RootNotDeletable);
        }
        self.path_of(id)
    }

    /// The path as the user knows it, from the root label, lossily.
    pub fn display_path(&self, id: NodeId) -> String {
        let mut out = self.root_label.clone();
        for name in self.names_from_root(id) {
            if !out.ends_with('/') {
                out.push('/');
            }
            out.push_str(&String::from_utf8_lossy(name));
        }
        out
    }

    fn names_from_root(&self, id: NodeId) -> Vec<&[u8]> {
        let mut names = Vec::new();
        let mut cur = id;
        while cur != ROOT && cur != NONE {
            let n = &self.nodes[cur as usize];
            names.push(&*n.name);
            cur = n.parent;
        }
        names.reverse();
        names
    }

    /// `ids` in their order, without repeats and without any node inside
    /// another of them: acting on the outer one covers it.
    pub fn outermost(&self, ids: &[NodeId]) -> Vec<NodeId> {
        let asked: HashSet<NodeId> = ids.iter().copied().collect();
        let mut seen = HashSet::new();
        ids.iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .filter(|&id| {
                let mut ancestors = std::iter::successors(self.parent(id), |&p| self.parent(p));
                !ancestors.any(|p| asked.contains(&p))
            })
            .collect()
    }
}

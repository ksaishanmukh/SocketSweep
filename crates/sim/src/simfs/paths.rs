//! Finding nodes of a [`SimFs`] by path, and paths by node.

use super::{NodeId, SimFs};

impl SimFs {
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
}

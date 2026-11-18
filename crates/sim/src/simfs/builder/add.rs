//! The calls that add nodes to a [`TreeBuilder`].

use socketsweep_wire::Kind;

use super::TreeBuilder;
use crate::simfs::NodeId;

impl TreeBuilder {
    pub fn dir(&mut self, parent: NodeId, name: impl Into<Vec<u8>>) -> NodeId {
        self.push(parent, name.into(), Kind::Dir, 4096)
    }

    pub fn file(&mut self, parent: NodeId, name: impl Into<Vec<u8>>, size: u64) -> NodeId {
        self.push(parent, name.into(), Kind::File, size)
    }

    pub fn symlink(&mut self, parent: NodeId, name: impl Into<Vec<u8>>, target_len: u64) -> NodeId {
        let id = self.push(parent, name.into(), Kind::Symlink, target_len);
        self.nodes[id as usize].blocks = 0;
        id
    }

    /// Another link to file `of`.
    pub fn hardlink(&mut self, parent: NodeId, name: impl Into<Vec<u8>>, of: NodeId) -> NodeId {
        let src = self.nodes[of as usize].clone();
        let id = self.push(parent, name.into(), Kind::File, src.apparent);
        let n = &mut self.nodes[id as usize];
        n.ino = src.ino;
        n.dev = src.dev;
        n.blocks = src.blocks;
        id
    }

    /// Make `d` a mount point of another file system (it and everything
    /// below it get device `dev`).
    pub fn mount(&mut self, d: NodeId, dev: u64) {
        let mut stack = vec![d];
        while let Some(x) = stack.pop() {
            self.nodes[x as usize].dev = dev;
            stack.extend(self.nodes[x as usize].children.iter().copied());
        }
    }
}

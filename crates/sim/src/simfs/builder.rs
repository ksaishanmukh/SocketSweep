//! [`Node`] and [`TreeBuilder`]: assembling a [`SimFs`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};

use socketsweep_wire::Kind;

use super::{NodeId, SimFs};

/// One file system object.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: Vec<u8>,
    pub parent: NodeId,
    pub kind: Kind,
    pub apparent: u64,
    pub blocks: u64,
    pub mtime: i64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    /// `getdents` reports `DT_UNKNOWN`, so a names listing must stat it.
    pub unknown_dtype: bool,
    /// Opening this directory fails with this errno.
    pub fail_open: Option<i32>,
    /// `lstat` of this entry fails with this errno.
    pub fail_stat: Option<i32>,
    /// Sorted by name once built.
    pub children: Vec<NodeId>,
    /// The order `getdents` returns the children in: by a hash of the name,
    /// as on ext4 (htree) and the FUSE layer above it.
    pub readdir: Vec<NodeId>,
}

/// Builds a [`SimFs`].
#[derive(Debug)]
pub struct TreeBuilder {
    nodes: Vec<Node>,
}

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeBuilder {
    /// A tree with just its root directory (node 0, device 1).
    pub fn new() -> Self {
        let mut b = TreeBuilder { nodes: Vec::new() };
        b.push(0, Vec::new(), Kind::Dir, 4096);
        b
    }

    fn push(&mut self, parent: NodeId, name: Vec<u8>, kind: Kind, size: u64) -> NodeId {
        let id = self.nodes.len() as NodeId;
        let dev = if id == 0 {
            1
        } else {
            self.nodes[parent as usize].dev
        };
        self.nodes.push(Node {
            name,
            parent,
            kind,
            apparent: size,
            blocks: size.div_ceil(4096) * 8,
            mtime: 1_700_000_000 + i64::from(id),
            dev,
            ino: u64::from(id) + 2,
            nlink: 1,
            unknown_dtype: false,
            fail_open: None,
            fail_stat: None,
            children: Vec::new(),
            readdir: Vec::new(),
        });
        if id != 0 {
            self.nodes[parent as usize].children.push(id);
        }
        id
    }

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

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn build(mut self, root_path: &[u8]) -> SimFs {
        let mut links: HashMap<(u64, u64), u64> = HashMap::new();
        for n in &self.nodes {
            *links.entry((n.dev, n.ino)).or_default() += 1;
        }
        for n in &mut self.nodes {
            n.nlink = links[&(n.dev, n.ino)];
        }
        let names: Vec<Vec<u8>> = self.nodes.iter().map(|n| n.name.clone()).collect();
        for n in &mut self.nodes {
            n.readdir = n.children.clone();
            n.readdir.sort_by_key(|&c| name_hash(&names[c as usize]));
            n.children
                .sort_by(|a, b| names[*a as usize].cmp(&names[*b as usize]));
            debug_assert!(
                n.children
                    .windows(2)
                    .all(|w| names[w[0] as usize] != names[w[1] as usize]),
                "duplicate name in a directory"
            );
        }
        let n = self.nodes.len();
        let mut fs = SimFs {
            root_path: root_path.to_vec(),
            nodes: self.nodes,
            paths: HashMap::new(),
            deleted: (0..n).map(|_| AtomicBool::new(false)).collect(),
            lists: (0..n).map(|_| AtomicU32::new(0)).collect(),
            stats: (0..n).map(|_| AtomicU32::new(0)).collect(),
        };
        for id in 0..n as NodeId {
            if fs.nodes[id as usize].kind == Kind::Dir {
                let p = fs.path_of(id);
                fs.paths.insert(p, id);
            }
        }
        fs
    }
}

fn name_hash(name: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

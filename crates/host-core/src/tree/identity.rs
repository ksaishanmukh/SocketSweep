//! Nodes and identity: references, liveness, parents and paths.

use socketsweep_gui_contract::{NodeId, NodeRef};
use socketsweep_wire::{DirId, ROOT_DIR};

use super::{DirState, Node, Tree, TreeError, NONE, ROOT};

impl Tree {
    /// The node a [`NodeRef`] names, if it is from this tree and still in it.
    pub fn resolve(&self, node: NodeRef) -> Result<NodeId, TreeError> {
        if node.session != self.session {
            return Err(TreeError::Stale);
        }
        self.live(node.id)?;
        Ok(node.id)
    }

    pub(super) fn live(&self, id: NodeId) -> Result<&Node, TreeError> {
        match self.nodes.get(id as usize) {
            None => Err(TreeError::UnknownNode(id)),
            Some(n) if n.removed => Err(TreeError::Removed),
            Some(n) => Ok(n),
        }
    }

    pub fn node_ref(&self, id: NodeId) -> NodeRef {
        NodeRef {
            session: self.session,
            id,
        }
    }

    pub fn exists(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(|n| !n.removed)
    }

    pub fn is_dir(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(Node::is_dir)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes.get(id as usize)?.parent;
        (p != NONE).then_some(p)
    }

    /// The node of a walked directory, if it is still in the tree.
    pub fn dir_node(&self, dir: DirId) -> Option<NodeId> {
        let rec = self.dirs.get(dir as usize)?.as_ref()?;
        (rec.state != DirState::Gone).then_some(rec.node)
    }

    /// What to focus for `id`: its own [`DirId`] if it is a walked directory,
    /// otherwise that of its nearest walked ancestor (a file means its
    /// folder). The root is [`ROOT_DIR`].
    pub fn focus_dir(&self, id: NodeId) -> Result<(NodeId, DirId), TreeError> {
        self.live(id)?;
        let mut cur = id;
        loop {
            let n = &self.nodes[cur as usize];
            if n.dir != NONE {
                return Ok((cur, n.dir));
            }
            cur = n.parent;
            if cur == NONE {
                return Ok((ROOT, ROOT_DIR));
            }
        }
    }

    /// The walked directory `id` is, if any.
    pub fn dir_of(&self, id: NodeId) -> Option<DirId> {
        let n = self.nodes.get(id as usize)?;
        (n.dir != NONE && !n.removed).then_some(n.dir)
    }

    /// `id` if it is still in the tree, otherwise its nearest ancestor that
    /// is. Where the GUI lands after the folder it showed is deleted.
    pub fn nearest_live(&self, id: NodeId) -> NodeId {
        if id as usize >= self.nodes.len() {
            return ROOT;
        }
        let mut cur = id;
        while cur != NONE && self.nodes[cur as usize].removed {
            cur = self.nodes[cur as usize].parent;
        }
        if cur == NONE {
            ROOT
        } else {
            cur
        }
    }

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

    /// Remove `id` and everything below it, once the phone confirms the
    /// delete. Already removed (by a `Pruned`) is not an error.
    pub fn remove(&mut self, id: NodeId) -> Result<(), TreeError> {
        if id == ROOT {
            return Err(TreeError::RootNotDeletable);
        }
        if id as usize >= self.nodes.len() {
            return Err(TreeError::UnknownNode(id));
        }
        self.remove_subtree(id);
        Ok(())
    }
}

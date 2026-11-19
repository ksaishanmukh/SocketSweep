//! Keeping sub totals and category sizes in step up the ancestor chain, and
//! detaching subtrees.

use socketsweep_gui_contract::NodeId;
use socketsweep_wire::Totals;

use super::category::{self, CatSizes};
use super::{DirState, Tree, NONE};

impl Tree {
    /// Mark `node`'s directory as no longer open in its parent's record.
    pub(super) fn close_in_parent(&mut self, node: NodeId) {
        let parent = self.nodes[node as usize].parent;
        if parent == NONE {
            return;
        }
        let pdir = self.nodes[parent as usize].dir;
        if let Some(Some(prec)) = self.dirs.get_mut(pdir as usize) {
            prec.open = prec.open.saturating_sub(1);
        }
    }

    /// Add `delta` to the sub totals of `from` and every ancestor.
    pub(super) fn add_up(&mut self, from: NodeId, delta: &Totals) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.sub.add(delta);
            cur = n.parent;
        }
    }

    /// Add `delta` to the category sizes of `from` and every ancestor. Every
    /// ancestor of a node is a walked directory, so each has a record.
    pub(super) fn add_cats_up(&mut self, from: NodeId, delta: &CatSizes) {
        self.each_ancestor_cats(from, |cats| category::add(cats, delta));
    }

    fn sub_cats_up(&mut self, from: NodeId, delta: &CatSizes) {
        self.each_ancestor_cats(from, |cats| category::subtract(cats, delta));
    }

    fn each_ancestor_cats(&mut self, from: NodeId, mut f: impl FnMut(&mut CatSizes)) {
        let mut cur = from;
        while cur != NONE {
            let n = &self.nodes[cur as usize];
            let (dir, parent) = (n.dir, n.parent);
            if let Some(Some(rec)) = self.dirs.get_mut(dir as usize) {
                f(&mut rec.cats);
            }
            cur = parent;
        }
    }

    /// Everything `node` contributes to its parent's category sizes.
    fn cat_contribution(&self, node: NodeId) -> CatSizes {
        let n = &self.nodes[node as usize];
        let mut cats = match self.dirs.get(n.dir as usize) {
            Some(Some(rec)) if n.dir != NONE => rec.cats,
            _ => CatSizes::default(),
        };
        category::put(&mut cats, n.own_category(), n.own);
        cats
    }

    /// Subtract `delta` from the sub totals of `from` and every ancestor.
    fn sub_up(&mut self, from: NodeId, delta: &Totals) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.sub.subtract(delta);
            cur = n.parent;
        }
    }

    /// Replace `old` by `new` in the sub totals of `from` and every ancestor.
    pub(super) fn replace_up(&mut self, from: NodeId, old: &Totals, new: &Totals) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.sub.subtract(old);
            n.sub.add(new);
            cur = n.parent;
        }
    }

    /// Detach `node` and everything below it, discounting it from every
    /// ancestor. Directories inside it are remembered as gone.
    pub(super) fn remove_subtree(&mut self, node: NodeId) {
        if self.nodes[node as usize].removed {
            return;
        }
        let contribution = self.nodes[node as usize].contribution();
        let cats = self.cat_contribution(node);
        let parent = self.nodes[node as usize].parent;
        let was_open = self.nodes[node as usize].dir != NONE
            && self
                .dirs
                .get(self.nodes[node as usize].dir as usize)
                .and_then(Option::as_ref)
                .is_some_and(|r| r.state != DirState::Done);

        self.unlink(parent, node);
        let mut stack = vec![node];
        while let Some(id) = stack.pop() {
            let n = &mut self.nodes[id as usize];
            if n.removed {
                continue;
            }
            n.removed = true;
            let dir = n.dir;
            let mut child = n.first_child;
            while child != NONE {
                stack.push(child);
                child = self.nodes[child as usize].next_sibling;
            }
            self.drop_staged(id);
            let rec = self.dirs.get_mut(dir as usize).filter(|_| dir != NONE);
            if let Some(Some(rec)) = rec {
                rec.state = DirState::Gone;
                rec.entries = Vec::new();
            }
        }
        self.sub_up(parent, &contribution);
        self.sub_cats_up(parent, &cats);
        if was_open {
            self.close_in_parent(node);
        }
    }

    fn unlink(&mut self, parent: NodeId, id: NodeId) {
        let next = self.nodes[id as usize].next_sibling;
        if self.nodes[parent as usize].first_child == id {
            self.nodes[parent as usize].first_child = next;
            if self.nodes[parent as usize].last_child == id {
                self.nodes[parent as usize].last_child = NONE;
            }
            return;
        }
        let mut cur = self.nodes[parent as usize].first_child;
        while cur != NONE && self.nodes[cur as usize].next_sibling != id {
            cur = self.nodes[cur as usize].next_sibling;
        }
        if cur == NONE {
            return;
        }
        self.nodes[cur as usize].next_sibling = next;
        if self.nodes[parent as usize].last_child == id {
            self.nodes[parent as usize].last_child = cur;
        }
    }
}

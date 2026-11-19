//! The pending-delete queue and what it does to the sizes the GUI shows.
//!
//! The queue is a set of nodes, kept as the user staged them. What counts is
//! the **outermost** staged nodes: one staged inside another staged one goes
//! with it and is never counted twice. Unstaging the outer one brings the inner
//! one back as still staged, which is why the set is not trimmed on the way in.
//!
//! A staged node that leaves the tree (a prune from the scan, a finished
//! delete) is dropped from the set, and `take_staged_dropped` tells the caller
//! so it can announce the change.
//!
//! # Projected sizes
//!
//! A node's projected size is its size minus the sizes of the outermost staged
//! nodes strictly below it, and zero when it is staged or lies under a staged
//! node. [`Projection`] sums those once per query by walking up from each
//! outermost staged node, so a view of N rows costs O(staged x depth) and
//! nothing when the queue is empty. Sizes carry both ways of counting, so the
//! result is right for whichever metric the GUI shows. A staged node that is
//! not fully measured yet only lowers the projection by what is known so far,
//! and its folder's `measure` stays `partial`, which marks the figure as moving.

use std::collections::{BTreeSet, HashMap};

use socketsweep_gui_contract::{
    Measure, NodeId, NodeRef, Size, StagedItem, StagedSummary, StagedTotals,
};

use super::nodes::entry_kind;
use super::{Tree, TreeError, NONE, ROOT};

/// What staged nodes take off each ancestor, and which nodes are staged.
pub(super) struct Projection<'a> {
    staged: &'a BTreeSet<NodeId>,
    /// Per ancestor of an outermost staged node: the sizes of those below it.
    freed: HashMap<NodeId, Size>,
}

impl Projection<'_> {
    pub(super) fn active(&self) -> bool {
        !self.staged.is_empty()
    }
}

impl Tree {
    /// The node a staging command names: live, from this session and not the
    /// scan root, which cannot be deleted.
    pub fn resolve_deletable(&self, node: NodeRef) -> Result<NodeId, TreeError> {
        let id = self.resolve(node)?;
        if id == ROOT {
            return Err(TreeError::RootNotDeletable);
        }
        Ok(id)
    }

    /// Queue `ids`. `true` if the queue changed.
    pub fn stage(&mut self, ids: &[NodeId]) -> bool {
        let before = self.staged.len();
        self.staged.extend(ids.iter().copied());
        self.staged.len() != before
    }

    /// Take `ids` out of the queue. `true` if the queue changed.
    pub fn unstage(&mut self, ids: &[NodeId]) -> bool {
        let before = self.staged.len();
        for id in ids {
            self.staged.remove(id);
        }
        self.staged.len() != before
    }

    /// Empty the queue. `true` if there was anything in it.
    pub fn clear_staged(&mut self) -> bool {
        let had = !self.staged.is_empty();
        self.staged.clear();
        had
    }

    /// Forget a node that has left the tree.
    pub(super) fn drop_staged(&mut self, id: NodeId) {
        if self.staged.remove(&id) {
            self.staged_dropped = true;
        }
    }

    /// Whether nodes left the queue because they left the tree since the last
    /// call.
    pub fn take_staged_dropped(&mut self) -> bool {
        std::mem::take(&mut self.staged_dropped)
    }

    /// Staged nodes not inside another staged node, in node order.
    pub fn staged_outermost(&self) -> Vec<NodeId> {
        self.staged
            .iter()
            .copied()
            .filter(|&id| !self.under_staged(id))
            .collect()
    }

    /// An ancestor of `id` is staged.
    fn under_staged(&self, id: NodeId) -> bool {
        let mut cur = self.nodes[id as usize].parent;
        while cur != NONE {
            if self.staged.contains(&cur) {
                return true;
            }
            cur = self.nodes[cur as usize].parent;
        }
        false
    }

    /// `id` is staged or lies inside a staged folder.
    pub(super) fn is_staged(&self, id: NodeId) -> bool {
        !self.staged.is_empty() && (self.staged.contains(&id) || self.under_staged(id))
    }

    pub fn staged_totals(&self) -> StagedTotals {
        let mut totals = StagedTotals::default();
        for id in self.staged_outermost() {
            let n = &self.nodes[id as usize];
            totals.items += 1;
            totals.size.apparent = totals.size.apparent.saturating_add(n.size().apparent);
            totals.size.disk = totals.size.disk.saturating_add(n.size().disk);
            totals.files = totals.files.saturating_add(Self::files_of(n));
            if n.measure != Measure::Final {
                totals.measure = Measure::Partial;
            }
        }
        totals
    }

    /// The queue as a list, largest first by disk usage.
    pub fn staged_summary(&self) -> StagedSummary {
        let mut items: Vec<StagedItem> = self
            .staged_outermost()
            .into_iter()
            .map(|id| {
                let n = &self.nodes[id as usize];
                StagedItem {
                    node: self.node_ref(id),
                    name: String::from_utf8_lossy(&n.name).into_owned(),
                    path: self.display_path(id),
                    kind: entry_kind(n.kind),
                    size: n.size(),
                    measure: n.measure,
                    files: Self::files_of(n),
                }
            })
            .collect();
        items.sort_by(|a, b| b.size.disk.cmp(&a.size.disk).then(a.path.cmp(&b.path)));
        StagedSummary {
            items,
            totals: self.staged_totals(),
        }
    }

    pub(super) fn projection(&self) -> Projection<'_> {
        let mut freed: HashMap<NodeId, Size> = HashMap::new();
        for id in self.staged_outermost() {
            let size = self.nodes[id as usize].size();
            let mut cur = self.nodes[id as usize].parent;
            while cur != NONE {
                let sum = freed.entry(cur).or_default();
                sum.apparent = sum.apparent.saturating_add(size.apparent);
                sum.disk = sum.disk.saturating_add(size.disk);
                cur = self.nodes[cur as usize].parent;
            }
        }
        Projection {
            staged: &self.staged,
            freed,
        }
    }

    /// Whether `id` is staged (itself or through a folder above) and its size
    /// with the queue applied.
    pub(super) fn project(&self, proj: &Projection, id: NodeId) -> (bool, Size) {
        let size = self.nodes[id as usize].size();
        if !proj.active() {
            return (false, size);
        }
        if self.is_staged(id) {
            return (true, Size::default());
        }
        let freed = proj.freed.get(&id).copied().unwrap_or_default();
        (
            false,
            Size {
                apparent: size.apparent.saturating_sub(freed.apparent),
                disk: size.disk.saturating_sub(freed.disk),
            },
        )
    }

    /// The whole tree's size with the queue applied.
    pub fn projected_total(&self) -> Size {
        self.project(&self.projection(), ROOT).1
    }
}

//! The pending-delete queue: staging commands, and the one delete that runs
//! what is queued.
//!
//! The queue itself lives in the scan's tree (see `tree/staged.rs`), so a
//! rescan, a disconnect or a lost connection discards it with the tree.

use socketsweep_gui_contract::{DeletedMany, NodeRef, StagedChanged, StagedSummary, StagedTotals};

use super::errors::{tree_err, CmdResult};
use super::state::{Inner, Scan};
use super::{delete, App, HostEvent};
use crate::lock;
use crate::tree::Tree;

impl App {
    /// Queue nodes for deletion. Nothing is removed until `delete_staged`.
    pub fn stage_delete(&self, nodes: Vec<NodeRef>) -> CmdResult<StagedTotals> {
        stage(&self.inner, &nodes)
    }

    pub fn unstage_delete(&self, nodes: Vec<NodeRef>) -> CmdResult<StagedTotals> {
        unstage(&self.inner, &nodes)
    }

    pub fn clear_staged(&self) -> CmdResult<StagedTotals> {
        clear(&self.inner)
    }

    pub fn staged_summary(&self) -> CmdResult<StagedSummary> {
        summary(&self.inner)
    }

    /// Delete everything queued; what the phone refuses stays queued.
    pub fn delete_staged(&self) -> CmdResult<DeletedMany> {
        delete_all(&self.inner)
    }
}

/// Check every node first, as `delete_many` does: one stale, removed or root
/// node refuses the whole call.
fn resolve(tree: &Tree, nodes: &[NodeRef]) -> CmdResult<Vec<u32>> {
    nodes
        .iter()
        .map(|n| tree.resolve_deletable(*n).map_err(tree_err))
        .collect()
}

fn stage(inner: &Inner, nodes: &[NodeRef]) -> CmdResult<StagedTotals> {
    change(inner, |tree| {
        let ids = resolve(tree, nodes)?;
        Ok(tree.stage(&ids))
    })
}

fn unstage(inner: &Inner, nodes: &[NodeRef]) -> CmdResult<StagedTotals> {
    // A node that has since left the tree is already out of the queue.
    change(inner, |tree| {
        let live: Vec<u32> = nodes.iter().filter_map(|n| tree.resolve(*n).ok()).collect();
        Ok(tree.unstage(&live))
    })
}

fn clear(inner: &Inner) -> CmdResult<StagedTotals> {
    change(inner, |tree| Ok(tree.clear_staged()))
}

fn summary(inner: &Inner) -> CmdResult<StagedSummary> {
    let scan = inner.scan()?;
    let tree = lock(&scan.tree);
    Ok(tree.staged_summary())
}

/// Delete the queue's outermost nodes with `delete_many`. A node deleted
/// leaves the queue with the tree; one the phone refuses stays queued, so it
/// can be retried or dropped.
fn delete_all(inner: &Inner) -> CmdResult<DeletedMany> {
    let scan = inner.scan()?;
    let nodes = {
        let tree = lock(&scan.tree);
        tree.staged_outermost()
            .into_iter()
            .map(|id| tree.node_ref(id))
            .collect()
    };
    delete::delete_many(inner, nodes)
}

/// Tell the GUI when nodes left the queue because they left the tree.
pub(super) fn announce_drops(inner: &Inner, scan: &Scan) {
    let changed = {
        let mut tree = lock(&scan.tree);
        tree.take_staged_dropped().then(|| tree.staged_totals())
    };
    if let Some(totals) = changed {
        inner.emit(HostEvent::StagedChanged(StagedChanged {
            session: scan.session,
            totals,
        }));
    }
}

/// Apply `edit` to the queue and announce the change, if it made one.
fn change(
    inner: &Inner,
    edit: impl FnOnce(&mut Tree) -> CmdResult<bool>,
) -> CmdResult<StagedTotals> {
    let scan = inner.scan()?;
    let (changed, totals) = {
        let mut tree = lock(&scan.tree);
        let changed = edit(&mut tree)?;
        (changed, tree.staged_totals())
    };
    if changed {
        inner.emit(HostEvent::StagedChanged(StagedChanged {
            session: scan.session,
            totals,
        }));
    }
    Ok(totals)
}

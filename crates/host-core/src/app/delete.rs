//! Deleting on the phone, then discounting it from the tree.

use std::collections::HashMap;

use socketsweep_gui_contract::{DeleteFailure, DeletedMany, NodeId, NodeRef, Size, Totals, View};

use super::errors::{client_err, tree_err, CmdResult};
use super::state::{Busy, Inner, Scan};
use crate::lock;
use crate::tree::{Tree, ROOT};

/// One node to delete.
struct Target {
    node: NodeRef,
    id: NodeId,
    path: Vec<u8>,
    display: String,
}

pub(super) fn delete_many(inner: &Inner, nodes: Vec<NodeRef>) -> CmdResult<DeletedMany> {
    let _busy = inner.claim(Busy::Deleting)?;
    let conn = inner.conn()?;
    let scan = inner.scan()?;
    let targets = {
        let tree = lock(&scan.tree);
        targets(&tree, &nodes)?
    };

    let mut deleted = 0;
    let mut items = 0;
    let mut size = Size::default();
    let mut failures = Vec::new();
    for t in targets {
        let hint = scan
            .running()
            .then(|| lock(&scan.tree).dir_of(t.id).map(|dir| (scan.req, dir)))
            .flatten();
        match conn.daemon.client.delete(t.path, hint, &mut |_| {}) {
            Ok((n, freed)) => {
                discount(&scan, t.id)?;
                deleted += 1;
                items += n;
                size.apparent = size.apparent.saturating_add(freed.apparent);
                size.disk = size.disk.saturating_add(freed.disk);
            }
            Err(e) => failures.push(DeleteFailure {
                node: t.node,
                path: t.display,
                error: client_err(e),
            }),
        }
    }
    let (totals, view) = refreshed(&scan)?;
    super::staged::announce_drops(inner, &scan);
    Ok(DeletedMany {
        deleted,
        items,
        size,
        failures,
        totals,
        view,
    })
}

/// Check every node before deleting any, and drop those inside another one
/// asked for: deleting the outer one takes them too.
fn targets(tree: &Tree, nodes: &[NodeRef]) -> CmdResult<Vec<Target>> {
    let mut paths = HashMap::with_capacity(nodes.len());
    for node in nodes {
        paths.insert(
            node.id,
            (*node, tree.delete_target(*node).map_err(tree_err)?),
        );
    }
    let ids: Vec<NodeId> = nodes.iter().map(|n| n.id).collect();
    Ok(tree
        .outermost(&ids)
        .into_iter()
        .filter_map(|id| {
            let (node, path) = paths.remove(&id)?;
            Some(Target {
                node,
                id,
                path,
                display: tree.display_path(id),
            })
        })
        .collect())
}

/// Only discount it locally once the phone confirms it is gone. A `Pruned`
/// may have done it already.
fn discount(scan: &Scan, id: NodeId) -> CmdResult<()> {
    let mut tree = lock(&scan.tree);
    if tree.exists(id) {
        tree.remove(id).map_err(tree_err)?;
    }
    Ok(())
}

/// The totals, and the focused folder (or its nearest survivor) as the GUI
/// last asked to see it.
fn refreshed(scan: &Scan) -> CmdResult<(Totals, View)> {
    let tree = lock(&scan.tree);
    let mut meta = lock(&scan.meta);
    let watching = tree.nearest_live(meta.focus.unwrap_or(ROOT));
    if meta.focus.is_some() {
        meta.focus = Some(watching);
    }
    let view = tree.view_query(watching, &meta.view).map_err(tree_err)?;
    Ok((tree.totals(), view))
}

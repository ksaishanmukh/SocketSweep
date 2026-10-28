//! Deleting on the phone, then discounting it from the tree.

use socketsweep_gui_contract::{Deleted, NodeRef};

use super::errors::{client_err, tree_err, CmdResult};
use super::state::{Busy, Inner};
use crate::lock;
use crate::tree::ROOT;

pub(super) fn delete(inner: &Inner, node: NodeRef) -> CmdResult<Deleted> {
    let _busy = inner.claim(Busy::Deleting)?;
    let conn = inner.conn()?;
    let scan = inner.scan()?;
    let (id, path, hint) = {
        let tree = lock(&scan.tree);
        let path = tree.delete_target(node).map_err(tree_err)?;
        let id = node.id;
        let hint = scan
            .running()
            .then(|| tree.dir_of(id).map(|dir| (scan.req, dir)))
            .flatten();
        (id, path, hint)
    };

    let (items, _) = conn
        .daemon
        .client
        .delete(path, hint, &mut |_| {})
        .map_err(client_err)?;

    // Only discount it locally once the phone confirms it is gone. A
    // `Pruned` may have done it already.
    let mut tree = lock(&scan.tree);
    if tree.exists(id) {
        tree.remove(id).map_err(tree_err)?;
    }
    let mut meta = lock(&scan.meta);
    let watching = tree.nearest_live(meta.focus.unwrap_or(ROOT));
    if meta.focus.is_some() {
        meta.focus = Some(watching);
    }
    let view = tree
        .view(watching, meta.view_limit, meta.view_by)
        .map_err(tree_err)?;
    Ok(Deleted {
        items,
        totals: tree.totals(),
        view,
    })
}

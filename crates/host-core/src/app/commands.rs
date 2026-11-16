//! The [`Commands`] implementation: queries answered here, long operations
//! handed to their modules.

use socketsweep_gui_contract::{
    Commands, Connected, CopyResult, Crumb, DeletedMany, Device, NodeRef, Overview, Row,
    ScanStarted, SearchFilter, SizeMetric, TreemapNode, View, ViewFilter, ViewSort,
    DEFAULT_SEARCH_LIMIT, DEFAULT_TREEMAP_DEPTH, DEFAULT_VIEW_LIMIT,
};

use super::errors::{tree_err, CmdResult};
use super::{apps, connect, copy, delete, overview, scan, App};
use crate::focus::FocusTarget;
use crate::lock;
use crate::tree::{SearchQuery, Tree, ViewQuery, ROOT};

/// The node a command names, the root when it names none.
fn node_or_root(tree: &Tree, node: Option<NodeRef>) -> CmdResult<u32> {
    match node {
        Some(node) => tree.resolve(node).map_err(tree_err),
        None => Ok(ROOT),
    }
}

fn view_query(
    limit: Option<u32>,
    by: Option<SizeMetric>,
    sort: Option<ViewSort>,
    filter: Option<ViewFilter>,
) -> ViewQuery {
    ViewQuery {
        limit: limit.unwrap_or(DEFAULT_VIEW_LIMIT),
        by: by.unwrap_or_default(),
        sort: sort.unwrap_or_default(),
        filter: filter.unwrap_or_default(),
    }
}

impl Commands for App {
    fn list_devices(&self) -> CmdResult<Vec<Device>> {
        connect::list_devices(&self.inner)
    }

    fn connect(&self, serial: Option<String>, root: Option<String>) -> CmdResult<Connected> {
        connect::connect(&self.inner, serial, root)
    }

    fn disconnect(&self) -> CmdResult<()> {
        self.inner.end_session();
        Ok(())
    }

    fn start_scan(&self, root: Option<String>) -> CmdResult<ScanStarted> {
        scan::start(&self.inner, root)
    }

    fn cancel_scan(&self) -> CmdResult<()> {
        if let Some(scan) = lock(&self.inner.scan).clone() {
            scan.cancel();
        }
        Ok(())
    }

    fn focus(
        &self,
        node: Option<NodeRef>,
        limit: Option<u32>,
        by: Option<SizeMetric>,
        sort: Option<ViewSort>,
        filter: Option<ViewFilter>,
    ) -> CmdResult<()> {
        let scan = self.inner.scan()?;
        let (folder, dir) = {
            let tree = lock(&scan.tree);
            let id = node_or_root(&tree, node)?;
            tree.focus_dir(id).map_err(tree_err)?
        };
        let running = {
            let mut meta = lock(&scan.meta);
            meta.focus = Some(folder);
            meta.view = view_query(limit, by, sort, filter);
            meta.finished.is_none()
        };
        if running {
            let target = if folder == ROOT {
                FocusTarget::Clear
            } else {
                FocusTarget::Dir(dir)
            };
            scan.focus.request(target);
        }
        Ok(())
    }

    fn get_view(
        &self,
        node: Option<NodeRef>,
        limit: Option<u32>,
        by: Option<SizeMetric>,
        sort: Option<ViewSort>,
        filter: Option<ViewFilter>,
    ) -> CmdResult<View> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = node_or_root(&tree, node)?;
        tree.view_query(id, &view_query(limit, by, sort, filter))
            .map_err(tree_err)
    }

    fn get_breadcrumbs(&self, node: NodeRef) -> CmdResult<Vec<Crumb>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = tree.resolve(node).map_err(tree_err)?;
        tree.breadcrumbs(id).map_err(tree_err)
    }

    fn get_treemap(
        &self,
        node: Option<NodeRef>,
        depth: Option<u32>,
        by: Option<SizeMetric>,
    ) -> CmdResult<TreemapNode> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = node_or_root(&tree, node)?;
        tree.treemap(
            id,
            depth.unwrap_or(DEFAULT_TREEMAP_DEPTH),
            by.unwrap_or_default(),
        )
        .map_err(tree_err)
    }

    fn search(
        &self,
        query: String,
        limit: Option<u32>,
        by: Option<SizeMetric>,
        filter: Option<SearchFilter>,
        sort: Option<ViewSort>,
        within: Option<NodeRef>,
    ) -> CmdResult<Vec<Row>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let within = within
            .map(|n| tree.resolve(n).map_err(tree_err))
            .transpose()?;
        Ok(tree.search_with(&SearchQuery {
            query,
            limit: limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
            by: by.unwrap_or_default(),
            filter: filter.unwrap_or_default(),
            sort: sort.unwrap_or_default(),
            within,
        }))
    }

    fn overview(&self, by: Option<SizeMetric>) -> CmdResult<Overview> {
        overview::overview(&self.inner, by.unwrap_or_default())
    }

    fn app_icon(&self, app: String) -> CmdResult<Option<String>> {
        Ok(apps::icon(&self.inner, &app))
    }

    fn delete_many(&self, nodes: Vec<NodeRef>) -> CmdResult<DeletedMany> {
        delete::delete_many(&self.inner, nodes)
    }

    fn copy_many(&self, nodes: Vec<NodeRef>, destination: Option<String>) -> CmdResult<CopyResult> {
        copy::copy_many(&self.inner, nodes, destination)
    }

    fn cancel_copy(&self) -> CmdResult<()> {
        copy::cancel_copy(&self.inner)
    }
}

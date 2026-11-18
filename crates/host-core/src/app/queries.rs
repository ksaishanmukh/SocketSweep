//! The queries a view is built from: focus, folder views, the treemap,
//! search, the overview and app icons.

use socketsweep_gui_contract::{
    Crumb, NodeRef, Overview, Row, SearchRequest, SizeMetric, TreemapNode, View, ViewRequest,
    DEFAULT_SEARCH_LIMIT, DEFAULT_TREEMAP_DEPTH, DEFAULT_VIEW_LIMIT,
};

use super::errors::{tree_err, CmdResult};
use super::{apps, overview, App};
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

fn view_query(request: &ViewRequest) -> ViewQuery {
    ViewQuery {
        limit: request.limit.unwrap_or(DEFAULT_VIEW_LIMIT),
        by: request.by.unwrap_or_default(),
        sort: request.sort.unwrap_or_default(),
        filter: request.filter.clone().unwrap_or_default(),
    }
}

impl App {
    /// The user opened a folder: the phone measures it first, and
    /// `scan-progress` carries its view from now on.
    pub fn focus(&self, request: ViewRequest) -> CmdResult<()> {
        let scan = self.inner.scan()?;
        let (folder, dir) = {
            let tree = lock(&scan.tree);
            let id = node_or_root(&tree, request.node)?;
            tree.focus_dir(id).map_err(tree_err)?
        };
        let running = {
            let mut meta = lock(&scan.meta);
            meta.focus = Some(folder);
            meta.view = view_query(&request);
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

    pub fn get_view(&self, request: ViewRequest) -> CmdResult<View> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = node_or_root(&tree, request.node)?;
        tree.view_query(id, &view_query(&request)).map_err(tree_err)
    }

    pub fn get_breadcrumbs(&self, node: NodeRef) -> CmdResult<Vec<Crumb>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let id = tree.resolve(node).map_err(tree_err)?;
        tree.breadcrumbs(id).map_err(tree_err)
    }

    pub fn get_treemap(
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

    pub fn search(&self, request: SearchRequest) -> CmdResult<Vec<Row>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        let within = request
            .within
            .map(|n| tree.resolve(n).map_err(tree_err))
            .transpose()?;
        Ok(tree.search_with(&SearchQuery {
            query: request.query,
            limit: request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
            by: request.by.unwrap_or_default(),
            filter: request.filter.unwrap_or_default(),
            sort: request.sort.unwrap_or_default(),
            within,
        }))
    }

    pub fn overview(&self, by: Option<SizeMetric>) -> CmdResult<Overview> {
        overview::overview(&self.inner, by.unwrap_or_default())
    }

    pub fn app_icon(&self, app: String) -> CmdResult<Option<String>> {
        Ok(apps::icon(&self.inner, &app))
    }
}

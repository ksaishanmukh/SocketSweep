//! The [`Commands`] implementation: queries answered here, long operations
//! handed to their modules.

use socketsweep_gui_contract::{
    AppUsage, Commands, Connected, CopyResult, Crumb, Deleted, Device, NodeRef, Row, ScanStarted,
    SizeMetric, TreemapNode, TypeGroup, View, DEFAULT_APPS_LIMIT, DEFAULT_LARGEST_LIMIT,
    DEFAULT_SEARCH_LIMIT, DEFAULT_TREEMAP_DEPTH, DEFAULT_VIEW_LIMIT,
};

use super::errors::{tree_err, CmdResult};
use super::{connect, copy, delete, scan, App};
use crate::focus::FocusTarget;
use crate::lock;
use crate::tree::ROOT;

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
    ) -> CmdResult<()> {
        let scan = self.inner.scan()?;
        let (folder, dir) = {
            let tree = lock(&scan.tree);
            let id = match node {
                Some(node) => tree.resolve(node).map_err(tree_err)?,
                None => ROOT,
            };
            tree.focus_dir(id).map_err(tree_err)?
        };
        let running = {
            let mut meta = lock(&scan.meta);
            meta.focus = Some(folder);
            meta.view_limit = limit.unwrap_or(DEFAULT_VIEW_LIMIT);
            meta.view_by = by.unwrap_or_default();
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
    ) -> CmdResult<View> {
        let scan = self.inner.scan()?;
        let limit = limit.unwrap_or(DEFAULT_VIEW_LIMIT);
        let by = by.unwrap_or_default();
        let tree = lock(&scan.tree);
        let id = match node {
            Some(node) => tree.resolve(node).map_err(tree_err)?,
            None => ROOT,
        };
        tree.view(id, limit, by).map_err(tree_err)
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
        let id = match node {
            Some(node) => tree.resolve(node).map_err(tree_err)?,
            None => ROOT,
        };
        tree.treemap(
            id,
            depth.unwrap_or(DEFAULT_TREEMAP_DEPTH),
            by.unwrap_or_default(),
        )
        .map_err(tree_err)
    }

    fn largest_files(&self, limit: Option<u32>, by: Option<SizeMetric>) -> CmdResult<Vec<Row>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.largest_files(
            limit.unwrap_or(DEFAULT_LARGEST_LIMIT),
            by.unwrap_or_default(),
        ))
    }

    fn type_breakdown(&self, by: Option<SizeMetric>) -> CmdResult<Vec<TypeGroup>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.type_breakdown(by.unwrap_or_default()))
    }

    fn app_breakdown(
        &self,
        limit: Option<u32>,
        by: Option<SizeMetric>,
    ) -> CmdResult<Vec<AppUsage>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.app_breakdown(limit.unwrap_or(DEFAULT_APPS_LIMIT), by.unwrap_or_default()))
    }

    fn search(
        &self,
        query: String,
        limit: Option<u32>,
        by: Option<SizeMetric>,
    ) -> CmdResult<Vec<Row>> {
        let scan = self.inner.scan()?;
        let tree = lock(&scan.tree);
        Ok(tree.search(
            &query,
            limit.unwrap_or(DEFAULT_SEARCH_LIMIT),
            by.unwrap_or_default(),
        ))
    }

    fn delete(&self, node: NodeRef) -> CmdResult<Deleted> {
        delete::delete(&self.inner, node)
    }

    fn copy_to_desktop(&self, node: NodeRef, destination: Option<String>) -> CmdResult<CopyResult> {
        copy::copy_to_desktop(&self.inner, node, destination)
    }

    fn cancel_copy(&self) -> CmdResult<()> {
        copy::cancel_copy(&self.inner)
    }
}

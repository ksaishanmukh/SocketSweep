//! The GUI's queries: totals, rows, views, breadcrumbs, the treemap, the
//! largest files and search.

use socketsweep_gui_contract::{
    self as gui, Crumb, Measure, NodeId, Row, Size, SizeMetric, TreemapNode, View,
};
use socketsweep_wire::Totals;

use super::nodes::entry_kind;
use super::{metric, Node, Tree, TreeError, NONE, ROOT};

impl Tree {
    /// The whole tree's totals.
    pub fn totals(&self) -> gui::Totals {
        let root = &self.nodes[ROOT as usize];
        gui::Totals {
            size: root.size(),
            files: root.sub.files,
            dirs: root.sub.dirs,
            errors: root.sub.errors,
        }
    }

    /// How far the root is measured.
    pub fn root_measure(&self) -> Measure {
        self.nodes[ROOT as usize].measure
    }

    pub fn measure(&self, id: NodeId) -> Option<Measure> {
        self.nodes.get(id as usize).map(|n| n.measure)
    }

    pub fn size(&self, id: NodeId) -> Option<Size> {
        self.nodes.get(id as usize).map(Node::size)
    }

    /// The newest modification time (Unix seconds) of this entry and, for a
    /// directory, everything inside it; `None` when the node does not exist
    /// or no mtime is known yet. A file's is its own; a directory's is the
    /// maximum of its own and the running (`Partial`) or final (`Final`)
    /// maximum below it, and is never lowered by deletes.
    pub fn newest_mtime(&self, id: NodeId) -> Option<i64> {
        let n = self.nodes.get(id as usize)?;
        Some(n.newest_mtime()).filter(|m| *m != 0)
    }

    fn files_of(n: &Node) -> u32 {
        if n.is_dir() {
            n.sub.files
        } else {
            1
        }
    }

    pub(super) fn row(&self, id: NodeId) -> Row {
        let n = &self.nodes[id as usize];
        Row {
            node: self.node_ref(id),
            name: String::from_utf8_lossy(&n.name).into_owned(),
            kind: entry_kind(n.kind),
            size: n.size(),
            measure: n.measure,
            files: Self::files_of(n),
            unreadable: n.sub.flags & Totals::UNREADABLE != 0,
            parent: None,
            parent_node: None,
        }
    }

    fn row_with_parent(&self, id: NodeId) -> Row {
        let mut row = self.row(id);
        let parent = self.nodes[id as usize].parent;
        if parent != NONE {
            row.parent = Some(self.display_path(parent));
            row.parent_node = Some(self.node_ref(parent));
        }
        row
    }

    pub(super) fn children_of(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.nodes[id as usize].first_child;
        while cur != NONE {
            if !self.nodes[cur as usize].removed {
                out.push(cur);
            }
            cur = self.nodes[cur as usize].next_sibling;
        }
        out
    }

    /// Largest first by `by`, then by name so equal rows do not swap places
    /// between refreshes.
    pub(super) fn sort(&self, ids: &mut [NodeId], by: SizeMetric) {
        ids.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            metric(y.size(), by)
                .cmp(&metric(x.size(), by))
                .then_with(|| x.name.cmp(&y.name))
                .then_with(|| a.cmp(b))
        });
    }

    /// One folder's rows (a file shows as an empty folder of itself).
    pub fn view(&self, id: NodeId, limit: u32, by: SizeMetric) -> Result<View, TreeError> {
        let n = self.live(id)?;
        let mut kids = self.children_of(id);
        self.sort(&mut kids, by);
        let hidden = kids.len().saturating_sub(limit as usize) as u32;
        let rows = kids
            .iter()
            .take(limit as usize)
            .map(|k| self.row(*k))
            .collect();
        Ok(View {
            node: self.node_ref(id),
            path: self.display_path(id),
            size: n.size(),
            measure: n.measure,
            files: Self::files_of(n),
            dirs: n.sub.dirs,
            unreadable: n.sub.flags & Totals::UNREADABLE != 0,
            rows,
            hidden,
        })
    }

    pub fn breadcrumbs(&self, id: NodeId) -> Result<Vec<Crumb>, TreeError> {
        self.live(id)?;
        let mut out = Vec::new();
        let mut cur = id;
        while cur != NONE {
            let n = &self.nodes[cur as usize];
            out.push(Crumb {
                node: self.node_ref(cur),
                name: String::from_utf8_lossy(&n.name).into_owned(),
            });
            cur = n.parent;
        }
        out.reverse();
        Ok(out)
    }

    /// `id` and its descendants down to `depth` levels, largest first, with
    /// a per-level cap: past it, tiles are too small to read.
    pub fn treemap(
        &self,
        id: NodeId,
        depth: u32,
        by: SizeMetric,
    ) -> Result<TreemapNode, TreeError> {
        self.live(id)?;
        const CAPS: [usize; 3] = [48, 12, 6];
        let depth = (depth as usize).min(CAPS.len());
        Ok(self.treemap_at(id, depth, &CAPS, 0, by))
    }

    fn treemap_at(
        &self,
        id: NodeId,
        remaining: usize,
        caps: &[usize],
        level: usize,
        by: SizeMetric,
    ) -> TreemapNode {
        let n = &self.nodes[id as usize];
        let mut out = TreemapNode {
            node: self.node_ref(id),
            name: String::from_utf8_lossy(&n.name).into_owned(),
            kind: entry_kind(n.kind),
            size: n.size(),
            measure: n.measure,
            children: Vec::new(),
        };
        if remaining == 0 || !n.is_dir() {
            return out;
        }
        let mut kids = self.children_of(id);
        self.sort(&mut kids, by);
        kids.truncate(caps.get(level).copied().unwrap_or(0));
        out.children = kids
            .into_iter()
            .filter(|k| metric(self.nodes[*k as usize].size(), by) > 0)
            .map(|k| self.treemap_at(k, remaining - 1, caps, level + 1, by))
            .collect();
        out
    }

    /// The largest files anywhere in the tree.
    pub fn largest_files(&self, limit: u32, by: SizeMetric) -> Vec<Row> {
        let mut files: Vec<NodeId> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.is_dir() && !n.removed)
            .map(|(i, _)| i as NodeId)
            .collect();
        self.sort(&mut files, by);
        files.truncate(limit as usize);
        files.iter().map(|f| self.row_with_parent(*f)).collect()
    }

    /// Files and folders whose name contains `query`, case-insensitively,
    /// largest first by `by`.
    pub fn search(&self, query: &str, limit: u32, by: SizeMetric) -> Vec<Row> {
        if query.is_empty() {
            return Vec::new();
        }
        let needle = query.to_lowercase();
        let mut hits: Vec<NodeId> = self
            .nodes
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, n)| {
                !n.removed
                    && String::from_utf8_lossy(&n.name)
                        .to_lowercase()
                        .contains(&needle)
            })
            .map(|(i, _)| i as NodeId)
            .collect();
        self.sort(&mut hits, by);
        hits.truncate(limit as usize);
        hits.iter().map(|h| self.row_with_parent(*h)).collect()
    }
}

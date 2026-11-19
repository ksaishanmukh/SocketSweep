//! Turning nodes into what the GUI shows: rows and treemap tiles.

use socketsweep_gui_contract::{Category, NodeId, Row, SizeMetric, TreemapNode, ViewSort};
use socketsweep_wire::Totals;

use super::category::dominant;
use super::nodes::entry_kind;
use super::staged::Projection;
use super::{metric, Node, Tree, TreeError, NONE};

/// How a treemap is drawn: by which size, with which nodes staged.
struct Lens<'a> {
    by: SizeMetric,
    proj: Projection<'a>,
}

/// Most treemap tiles per level, by depth below the requested node.
const TREEMAP_CAPS: [usize; 3] = [48, 12, 6];

impl Tree {
    pub(super) fn files_of(n: &Node) -> u32 {
        if n.is_dir() {
            n.sub.files
        } else {
            1
        }
    }

    /// A file's category from its name; a walked folder's from the bytes
    /// below it, by `by`.
    pub fn category(&self, id: NodeId, by: SizeMetric) -> Category {
        let n = &self.nodes[id as usize];
        if !n.is_dir() {
            return n.own_category();
        }
        match self.dirs.get(n.dir as usize) {
            Some(Some(rec)) => dominant(&rec.cats, by),
            _ => Category::Other,
        }
    }

    pub(super) fn row(&self, id: NodeId, by: SizeMetric, proj: &Projection) -> Row {
        let n = &self.nodes[id as usize];
        let (staged, projected) = self.project(proj, id);
        Row {
            node: self.node_ref(id),
            name: String::from_utf8_lossy(&n.name).into_owned(),
            kind: entry_kind(n.kind),
            size: n.size(),
            measure: n.measure,
            files: Self::files_of(n),
            unreadable: n.sub.flags & Totals::UNREADABLE != 0,
            changed: self.newest_mtime(id),
            category: self.category(id, by),
            app: self.app_tag(id),
            parent: None,
            parent_node: None,
            staged,
            projected,
        }
    }

    /// A row that also says which folder it is in, for results that span the
    /// tree.
    pub(super) fn row_with_parent(&self, id: NodeId, by: SizeMetric, proj: &Projection) -> Row {
        let mut row = self.row(id, by, proj);
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

    /// `id` and its descendants down to `depth` levels, largest first, with
    /// a per-level cap: past it, tiles are too small to read.
    pub fn treemap(
        &self,
        id: NodeId,
        depth: u32,
        by: SizeMetric,
    ) -> Result<TreemapNode, TreeError> {
        self.live(id)?;
        let depth = (depth as usize).min(TREEMAP_CAPS.len());
        let lens = Lens {
            by,
            proj: self.projection(),
        };
        Ok(self.treemap_at(id, depth, 0, &lens))
    }

    fn treemap_at(&self, id: NodeId, remaining: usize, level: usize, lens: &Lens) -> TreemapNode {
        let by = lens.by;
        let n = &self.nodes[id as usize];
        let (staged, projected) = self.project(&lens.proj, id);
        let mut out = TreemapNode {
            node: self.node_ref(id),
            name: String::from_utf8_lossy(&n.name).into_owned(),
            kind: entry_kind(n.kind),
            size: n.size(),
            measure: n.measure,
            changed: self.newest_mtime(id),
            category: self.category(id, by),
            app: self.app_tag(id),
            staged,
            projected,
            children: Vec::new(),
        };
        if remaining == 0 || !n.is_dir() {
            return out;
        }
        let mut kids = self.children_of(id);
        self.order(&mut kids, ViewSort::default(), by);
        kids.truncate(TREEMAP_CAPS.get(level).copied().unwrap_or(0));
        out.children = kids
            .into_iter()
            .filter(|k| metric(self.nodes[*k as usize].size(), by) > 0)
            .map(|k| self.treemap_at(k, remaining - 1, level + 1, lens))
            .collect();
        out
    }
}

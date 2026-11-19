//! The GUI's queries: totals, folder views, breadcrumbs, the largest files
//! and unreadable folders.

use socketsweep_gui_contract::{
    self as gui, Crumb, Measure, NodeId, Row, Size, SizeMetric, UnreadableFolder, View, ViewSort,
};
use socketsweep_wire::Totals;

use super::order::{Matcher, ViewQuery};
use super::{Node, Tree, TreeError, NONE, ROOT};

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

    /// One folder's rows, largest first, unfiltered (a file shows as an empty
    /// folder of itself).
    pub fn view(&self, id: NodeId, limit: u32, by: SizeMetric) -> Result<View, TreeError> {
        self.view_query(
            id,
            &ViewQuery {
                limit,
                by,
                ..ViewQuery::default()
            },
        )
    }

    /// One folder's rows as `q` asks: filtered over every child, then sorted,
    /// then cut to the limit.
    pub fn view_query(&self, id: NodeId, q: &ViewQuery) -> Result<View, TreeError> {
        let n = self.live(id)?;
        let kids = self.children_of(id);
        let total_children = kids.len() as u32;
        let matcher = Matcher::view(&q.filter, q.by);
        let mut kept: Vec<NodeId> = if matcher.is_empty() {
            kids
        } else {
            kids.into_iter()
                .filter(|k| self.matches(*k, &matcher))
                .collect()
        };
        let total_matching = kept.len() as u32;
        self.order(&mut kept, q.sort, q.by);
        let proj = self.projection();
        let rows: Vec<Row> = kept
            .iter()
            .take(q.limit as usize)
            .map(|k| self.row(*k, q.by, &proj))
            .collect();
        let (staged, projected) = self.project(&proj, id);
        Ok(View {
            node: self.node_ref(id),
            path: self.display_path(id),
            size: n.size(),
            measure: n.measure,
            files: Self::files_of(n),
            dirs: n.sub.dirs,
            unreadable: n.sub.flags & Totals::UNREADABLE != 0,
            changed: self.newest_mtime(id),
            category: self.category(id, q.by),
            app: self.app_tag(id),
            staged,
            projected,
            hidden: total_matching - rows.len() as u32,
            rows,
            total_matching,
            total_children,
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

    /// The largest files anywhere in the tree.
    pub fn largest_files(&self, limit: u32, by: SizeMetric) -> Vec<Row> {
        let mut files: Vec<NodeId> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.is_dir() && !n.removed)
            .map(|(i, _)| i as NodeId)
            .collect();
        self.order(&mut files, ViewSort::default(), by);
        files.truncate(limit as usize);
        let proj = self.projection();
        files
            .iter()
            .map(|f| self.row_with_parent(*f, by, &proj))
            .collect()
    }

    /// Folders that could not be read and are still in the tree, in the order
    /// the phone reported them.
    pub fn unreadable_folders(&self) -> Vec<UnreadableFolder> {
        self.unreadable
            .iter()
            .filter(|id| self.exists(**id))
            .map(|id| UnreadableFolder {
                node: self.node_ref(*id),
                path: self.display_path(*id),
            })
            .collect()
    }
}

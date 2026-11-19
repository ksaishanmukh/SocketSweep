//! Finding files and folders by name and filter, anywhere or inside one
//! folder.

use socketsweep_gui_contract::{NodeId, Row, SearchFilter, SizeMetric, ViewSort};

use super::order::Matcher;
use super::{Tree, NONE};

/// What a search asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Names containing this, case-insensitively.
    pub query: String,
    pub limit: u32,
    pub by: SizeMetric,
    pub filter: SearchFilter,
    pub sort: ViewSort,
    /// Only inside this folder (not the folder itself); `None` is the whole
    /// tree.
    pub within: Option<NodeId>,
}

impl Tree {
    /// Files and folders whose name contains `query`, case-insensitively,
    /// largest first by `by`.
    pub fn search(&self, query: &str, limit: u32, by: SizeMetric) -> Vec<Row> {
        self.search_with(&SearchQuery {
            query: query.to_string(),
            limit,
            by,
            ..SearchQuery::default()
        })
    }

    /// Everything that matches `q`, in its order, cut to its limit. An empty
    /// query with no filter finds nothing rather than the whole tree.
    pub fn search_with(&self, q: &SearchQuery) -> Vec<Row> {
        let matcher = Matcher::search(&q.query, &q.filter, q.by);
        if matcher.is_empty() {
            return Vec::new();
        }
        let within = q.within.filter(|w| self.exists(*w));
        if q.within.is_some() && within.is_none() {
            return Vec::new();
        }
        let mut hits: Vec<NodeId> = (1..self.nodes.len() as NodeId)
            .filter(|id| !self.nodes[*id as usize].removed)
            .filter(|id| within.is_none_or(|w| self.is_inside(*id, w)))
            .filter(|id| self.matches(*id, &matcher))
            .collect();
        self.order(&mut hits, q.sort, q.by);
        hits.truncate(q.limit as usize);
        let proj = self.projection();
        hits.iter()
            .map(|h| self.row_with_parent(*h, q.by, &proj))
            .collect()
    }

    /// `id` is strictly below `folder`.
    fn is_inside(&self, id: NodeId, folder: NodeId) -> bool {
        let mut cur = self.nodes[id as usize].parent;
        while cur != NONE {
            if cur == folder {
                return true;
            }
            cur = self.nodes[cur as usize].parent;
        }
        false
    }
}

//! The scanned tree, owned by the host process.
//!
//! The frontend never receives this. It asks for the handful of rows it is
//! about to draw and refers to nodes by [`NodeId`]. That arrangement is what
//! removes the previous design's three worst properties at once:
//!
//!   - a multi-megabyte JSON tree crossing the IPC boundary on every scan
//!   - `JSON.parse(JSON.stringify(tree))` deep-cloning ~56,000 nodes on every
//!     single delete
//!   - byte-exact Android paths having to survive a round trip through
//!     JavaScript strings in order to come back as a delete target
//!
//! # Shape
//!
//! A flat `Vec<Node>` with child links held as indices rather than a nested
//! `Vec<Node>` per directory. Nodes are never moved or reallocated individually,
//! ids stay stable for the life of a scan, and walking to a parent is an array
//! index rather than a pointer chase.
//!
//! Only the node's own `name` is stored. Full paths are rebuilt by walking
//! parents on the rare occasions one is needed, which is why the daemon does not
//! need to send an absolute path per node.

use std::collections::HashMap;

use serde::Serialize;
use socketsweep_protocol::{Entry, EntryKind};

pub type NodeId = u32;

/// Sentinel for "no node". The root is always id 0, so 0 cannot mean absent.
pub const NONE: NodeId = u32::MAX;
pub const ROOT: NodeId = 0;

#[derive(Debug)]
struct Node {
    name: Box<[u8]>,
    parent: NodeId,
    first_child: NodeId,
    /// Children are a singly linked list so appending during a streaming scan
    /// never reallocates a per-directory vector.
    next_sibling: NodeId,
    last_child: NodeId,

    /// Subtree total. Climbs as frames arrive.
    size: u64,
    files: u32,
    dirs: u32,

    is_dir: bool,
    /// This directory's own frame has arrived, so its direct children are known.
    /// Descendants may still be filling in.
    listed: bool,
    removed: bool,
}

pub struct Arena {
    nodes: Vec<Node>,
    /// Directory path -> id. Only directories are indexed: a `Frame::Dir` is the
    /// only thing that arrives keyed by path, and there are ~1,600 directories
    /// against ~56,000 files on a typical device.
    dir_index: HashMap<Box<[u8]>, NodeId>,
    root_path: Box<[u8]>,
    /// Directories announced by a parent whose own frame has not yet arrived.
    pending_dirs: u32,
}

// ── Values handed to the frontend ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: NodeId,
    /// Lossy UTF-8 for display only. Deletes travel by `id`, so a name that is
    /// not valid UTF-8 is still displayable without becoming unsafe to act on.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub files: u32,
    /// False while this subtree is still being walked, so the UI can mark a
    /// number as still settling rather than presenting it as final.
    pub complete: bool,
    /// Containing directory, for views that cross the tree — a search hit or a
    /// largest-file entry is meaningless without knowing where it lives.
    /// Omitted inside a single directory listing, where it would be the same
    /// string on every row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Id of that folder, so "go to it" stays an id lookup rather than needing a
    /// path-to-node search command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<NodeId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: NodeId,
    pub path: String,
    pub size: u64,
    pub rows: Vec<Row>,
    /// Rows beyond the requested limit, so the UI can say "and 412 more".
    pub hidden: usize,
    pub complete: bool,
}

/// A node plus a bounded slice of its descendants, for the nested treemap.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreemapNode {
    pub id: NodeId,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<TreemapNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub id: NodeId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub size: u64,
    pub files: u32,
    pub dirs: u32,
    pub scanning: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ArenaError {
    UnknownNode(NodeId),
    /// A `Frame::Dir` arrived for a path no parent had announced.
    OrphanFrame(String),
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::UnknownNode(id) => write!(f, "no node with id {id}"),
            ArenaError::OrphanFrame(p) => {
                write!(
                    f,
                    "received a directory frame for {p}, which no parent announced"
                )
            }
        }
    }
}

impl Arena {
    pub fn new(root_path: &[u8]) -> Self {
        let name = basename(root_path).to_vec().into_boxed_slice();
        let root = Node {
            name,
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            last_child: NONE,
            size: 0,
            files: 0,
            dirs: 0,
            is_dir: true,
            listed: false,
            removed: false,
        };

        let mut dir_index = HashMap::new();
        let root_path: Box<[u8]> = root_path.to_vec().into_boxed_slice();
        dir_index.insert(root_path.clone(), ROOT);

        Arena {
            nodes: vec![root],
            dir_index,
            root_path,
            pending_dirs: 1,
        }
    }

    // ── Ingest ──────────────────────────────────────────────────────────────

    /// Fold one directory frame into the tree.
    ///
    /// Relies on the daemon's ordering guarantee: a directory is discovered while
    /// reading its parent, so by the time its own frame arrives a node already
    /// exists for it. `crates/scanner` asserts that property under test.
    pub fn apply_dir(&mut self, path: &[u8], entries: &[Entry]) -> Result<(), ArenaError> {
        let dir_id = *self
            .dir_index
            .get(path)
            .ok_or_else(|| ArenaError::OrphanFrame(String::from_utf8_lossy(path).into_owned()))?;

        if self.nodes[dir_id as usize].listed {
            // A duplicate frame would double-count every byte in it.
            return Ok(());
        }

        let mut added_size = 0u64;
        let mut added_files = 0u32;
        let mut added_dirs = 0u32;

        for entry in entries {
            let is_dir = entry.kind == EntryKind::Dir;
            let child = self.push_node(dir_id, &entry.name, is_dir, entry.size);

            if is_dir {
                added_dirs += 1;
                self.pending_dirs += 1;
                let mut child_path = path.to_vec();
                if child_path.last() != Some(&b'/') {
                    child_path.push(b'/');
                }
                child_path.extend_from_slice(&entry.name);
                self.dir_index.insert(child_path.into_boxed_slice(), child);
            } else {
                added_files += 1;
                added_size += entry.size;
            }
        }

        self.nodes[dir_id as usize].listed = true;
        self.pending_dirs = self.pending_dirs.saturating_sub(1);

        // Directory entries contribute 0 bytes here; their contents arrive in
        // their own frames and propagate up through this same path.
        self.add_to_ancestry(dir_id, added_size, added_files, added_dirs);
        Ok(())
    }

    fn push_node(&mut self, parent: NodeId, name: &[u8], is_dir: bool, size: u64) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            name: name.to_vec().into_boxed_slice(),
            parent,
            first_child: NONE,
            next_sibling: NONE,
            last_child: NONE,
            size: if is_dir { 0 } else { size },
            files: u32::from(!is_dir),
            dirs: 0,
            is_dir,
            listed: false,
            removed: false,
        });

        // Append via last_child so building a 5,000-entry directory stays linear.
        let p = &mut self.nodes[parent as usize];
        if p.first_child == NONE {
            p.first_child = id;
        } else {
            let last = p.last_child;
            self.nodes[last as usize].next_sibling = id;
        }
        self.nodes[parent as usize].last_child = id;
        id
    }

    /// Add a delta to `from` and every ancestor. O(depth).
    fn add_to_ancestry(&mut self, from: NodeId, size: u64, files: u32, dirs: u32) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.size += size;
            n.files += files;
            n.dirs += dirs;
            cur = n.parent;
        }
    }

    fn sub_from_ancestry(&mut self, from: NodeId, size: u64, files: u32, dirs: u32) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            // Saturating rather than wrapping: an accounting slip should show a
            // slightly wrong number, not a 16-exabyte one.
            n.size = n.size.saturating_sub(size);
            n.files = n.files.saturating_sub(files);
            n.dirs = n.dirs.saturating_sub(dirs);
            cur = n.parent;
        }
    }

    /// Detach a subtree and discount it from every ancestor.
    pub fn remove(&mut self, id: NodeId) -> Result<(), ArenaError> {
        if id == ROOT {
            return Err(ArenaError::UnknownNode(id));
        }
        let node = self
            .nodes
            .get(id as usize)
            .ok_or(ArenaError::UnknownNode(id))?;
        if node.removed {
            return Ok(());
        }

        let (size, files, dirs, parent) = (node.size, node.files, node.dirs, node.parent);
        let self_dir = u32::from(node.is_dir);

        self.unlink(parent, id);
        // Mark the whole subtree, not just the node. `largest_files` and
        // `search` scan the flat node vector, so a descendant left unmarked
        // would keep showing up in results after its parent was deleted.
        self.mark_removed(id);
        self.sub_from_ancestry(parent, size, files, dirs + self_dir);
        Ok(())
    }

    fn mark_removed(&mut self, root: NodeId) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let n = &mut self.nodes[id as usize];
            if n.removed {
                continue;
            }
            n.removed = true;

            let mut child = n.first_child;
            while child != NONE {
                stack.push(child);
                child = self.nodes[child as usize].next_sibling;
            }
        }
    }

    fn unlink(&mut self, parent: NodeId, id: NodeId) {
        let mut cur = self.nodes[parent as usize].first_child;
        if cur == id {
            let next = self.nodes[id as usize].next_sibling;
            self.nodes[parent as usize].first_child = next;
            if self.nodes[parent as usize].last_child == id {
                self.nodes[parent as usize].last_child = next;
            }
            return;
        }
        while cur != NONE {
            let next = self.nodes[cur as usize].next_sibling;
            if next == id {
                let after = self.nodes[id as usize].next_sibling;
                self.nodes[cur as usize].next_sibling = after;
                if self.nodes[parent as usize].last_child == id {
                    self.nodes[parent as usize].last_child = cur;
                }
                return;
            }
            cur = next;
        }
    }

    // ── Queries ─────────────────────────────────────────────────────────────

    pub fn scanning(&self) -> bool {
        self.pending_dirs > 0
    }

    pub fn stats(&self) -> Stats {
        let root = &self.nodes[ROOT as usize];
        Stats {
            size: root.size,
            files: root.files,
            dirs: root.dirs,
            scanning: self.scanning(),
        }
    }

    pub fn exists(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(|n| !n.removed)
    }

    pub fn is_dir(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(|n| n.is_dir)
    }

    /// Rebuild an absolute path by walking to the root. Byte-exact, so the
    /// result is safe to hand to the daemon as a delete target.
    pub fn path_of(&self, id: NodeId) -> Result<Vec<u8>, ArenaError> {
        if self.nodes.get(id as usize).is_none() {
            return Err(ArenaError::UnknownNode(id));
        }

        let mut parts = Vec::new();
        let mut cur = id;
        while cur != ROOT {
            let n = &self.nodes[cur as usize];
            parts.push(&n.name);
            cur = n.parent;
        }

        let mut out = self.root_path.to_vec();
        for name in parts.iter().rev() {
            if out.last() != Some(&b'/') {
                out.push(b'/');
            }
            out.extend_from_slice(name);
        }
        Ok(out)
    }

    fn row(&self, id: NodeId) -> Row {
        let n = &self.nodes[id as usize];
        Row {
            id,
            name: String::from_utf8_lossy(&n.name).into_owned(),
            size: n.size,
            is_dir: n.is_dir,
            files: n.files,
            complete: self.subtree_complete(id),
            parent: None,
            parent_id: None,
        }
    }

    /// A row carrying its containing directory, for results that span the tree.
    fn row_with_parent(&self, id: NodeId) -> Row {
        let mut row = self.row(id);
        let parent = self.nodes[id as usize].parent;
        if parent != NONE {
            row.parent = self
                .path_of(parent)
                .ok()
                .map(|p| String::from_utf8_lossy(&p).into_owned());
            row.parent_id = Some(parent);
        }
        row
    }

    /// A file is always complete; a directory is complete once it and everything
    /// under it has been listed. Approximated by "no directories are pending"
    /// during the final phase, and by its own `listed` flag before that.
    fn subtree_complete(&self, id: NodeId) -> bool {
        let n = &self.nodes[id as usize];
        if !n.is_dir {
            return true;
        }
        !self.scanning() || (n.listed && n.dirs == 0)
    }

    fn children_of(&self, id: NodeId) -> Vec<NodeId> {
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

    /// The rows for one directory, largest first, capped at `limit`.
    pub fn view(&self, id: NodeId, limit: usize) -> Result<View, ArenaError> {
        if !self.exists(id) {
            return Err(ArenaError::UnknownNode(id));
        }

        let mut kids = self.children_of(id);
        kids.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            // Name is the tiebreaker so equal-sized rows do not swap places
            // between refreshes while a scan is still running.
            y.size.cmp(&x.size).then_with(|| x.name.cmp(&y.name))
        });

        let hidden = kids.len().saturating_sub(limit);
        let rows = kids.iter().take(limit).map(|c| self.row(*c)).collect();

        Ok(View {
            id,
            path: String::from_utf8_lossy(&self.path_of(id)?).into_owned(),
            size: self.nodes[id as usize].size,
            rows,
            hidden,
            complete: self.subtree_complete(id),
        })
    }

    pub fn breadcrumbs(&self, id: NodeId) -> Result<Vec<Crumb>, ArenaError> {
        if self.nodes.get(id as usize).is_none() {
            return Err(ArenaError::UnknownNode(id));
        }
        let mut out = Vec::new();
        let mut cur = id;
        loop {
            let n = &self.nodes[cur as usize];
            out.push(Crumb {
                id: cur,
                name: String::from_utf8_lossy(&n.name).into_owned(),
            });
            if cur == ROOT {
                break;
            }
            cur = n.parent;
        }
        out.reverse();
        Ok(out)
    }

    /// The largest files anywhere in the tree.
    ///
    /// This is the question the app exists to answer, and the previous UI could
    /// only answer it by expanding folders one at a time.
    pub fn largest_files(&self, limit: usize) -> Vec<Row> {
        let mut files: Vec<NodeId> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.is_dir && !n.removed)
            .map(|(i, _)| i as NodeId)
            .collect();

        files.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            y.size.cmp(&x.size).then_with(|| x.name.cmp(&y.name))
        });
        files.truncate(limit);
        files.iter().map(|f| self.row_with_parent(*f)).collect()
    }

    /// A depth-limited slice of the tree for the treemap.
    ///
    /// The treemap draws children inside their parent tile, so it needs more
    /// than one level — but not the whole tree. Per-level caps keep the payload
    /// bounded: anything past the cap is too small to be a legible tile anyway.
    pub fn treemap(&self, id: NodeId, depth: usize) -> Result<TreemapNode, ArenaError> {
        if !self.exists(id) {
            return Err(ArenaError::UnknownNode(id));
        }
        /// Fan-out per level. Deeper tiles are smaller, so fewer are readable.
        const CAPS: [usize; 3] = [48, 12, 6];
        Ok(self.treemap_at(id, depth.min(CAPS.len()), &CAPS, 0))
    }

    fn treemap_at(
        &self,
        id: NodeId,
        remaining: usize,
        caps: &[usize],
        level: usize,
    ) -> TreemapNode {
        let n = &self.nodes[id as usize];
        let mut node = TreemapNode {
            id,
            name: String::from_utf8_lossy(&n.name).into_owned(),
            size: n.size,
            is_dir: n.is_dir,
            children: Vec::new(),
        };

        if remaining == 0 || !n.is_dir {
            return node;
        }

        let mut kids = self.children_of(id);
        kids.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            y.size.cmp(&x.size).then_with(|| x.name.cmp(&y.name))
        });
        kids.truncate(caps.get(level).copied().unwrap_or(0));

        node.children = kids
            .into_iter()
            .filter(|k| self.nodes[*k as usize].size > 0)
            .map(|k| self.treemap_at(k, remaining - 1, caps, level + 1))
            .collect();
        node
    }

    /// Total bytes per broad file category, largest first.
    ///
    /// A single pass over the flat node array — the shape that makes this cheap
    /// is the same one that makes `largest_files` cheap.
    pub fn type_breakdown(&self) -> Vec<TypeGroup> {
        let mut sizes = [0u64; CATEGORIES.len()];
        let mut counts = [0u32; CATEGORIES.len()];

        for n in self.nodes.iter().filter(|n| !n.is_dir && !n.removed) {
            let c = category_of(&n.name);
            sizes[c] += n.size;
            counts[c] += 1;
        }

        let mut groups: Vec<TypeGroup> = CATEGORIES
            .iter()
            .enumerate()
            .map(|(i, label)| TypeGroup {
                label,
                size: sizes[i],
                files: counts[i],
            })
            .filter(|g| g.files > 0)
            .collect();

        groups.sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.label.cmp(b.label)));
        groups
    }

    /// Case-insensitive substring match on names, largest first.
    pub fn search(&self, needle: &str, limit: usize) -> Vec<Row> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle = needle.to_lowercase();

        let mut hits: Vec<NodeId> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                !n.removed
                    && String::from_utf8_lossy(&n.name)
                        .to_lowercase()
                        .contains(&needle)
            })
            .map(|(i, _)| i as NodeId)
            .collect();

        hits.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            y.size.cmp(&x.size).then_with(|| x.name.cmp(&y.name))
        });
        hits.truncate(limit);
        hits.iter().map(|h| self.row_with_parent(*h)).collect()
    }
}

// ── File-type classification ────────────────────────────────────────────────

/// Broad categories, in the order they are presented.
///
/// Deliberately coarse. "You have 24GB of video" is an answer someone can act
/// on; a list of forty extensions is the same data with the conclusion removed.
pub const CATEGORIES: [&str; 7] = [
    "Photos",
    "Video",
    "Audio",
    "Apps",
    "Documents",
    "Archives",
    "Other",
];

const PHOTOS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "bmp", "tiff", "tif", "dng", "raw",
    "avif", "svg",
];
const VIDEO: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "3gp", "webm", "flv", "m4v", "wmv", "ts", "mpg", "mpeg",
];
const AUDIO: &[&str] = &[
    "mp3", "aac", "flac", "wav", "ogg", "m4a", "opus", "wma", "amr", "mid",
];
const APPS: &[&str] = &["apk", "obb", "xapk", "apks", "aab"];
const DOCS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "epub", "mobi", "csv", "rtf",
    "odt",
];
const ARCHIVES: &[&str] = &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img"];

fn category_of(name: &[u8]) -> usize {
    let Some(dot) = name.iter().rposition(|b| *b == b'.') else {
        return 6; // no extension
    };
    // A leading dot is a hidden file, not an extension: ".gitignore".
    if dot == 0 || dot + 1 >= name.len() {
        return 6;
    }

    let ext = String::from_utf8_lossy(&name[dot + 1..]).to_lowercase();
    let ext = ext.as_str();

    for (idx, list) in [PHOTOS, VIDEO, AUDIO, APPS, DOCS, ARCHIVES]
        .iter()
        .enumerate()
    {
        if list.contains(&ext) {
            return idx;
        }
    }
    6
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeGroup {
    pub label: &'static str,
    pub size: u64,
    pub files: u32,
}

fn basename(path: &[u8]) -> &[u8] {
    let trimmed = match path.iter().rposition(|b| *b != b'/') {
        Some(end) => &path[..=end],
        None => return path, // "/" or ""
    };
    match trimmed.iter().rposition(|b| *b == b'/') {
        Some(slash) => &trimmed[slash + 1..],
        None => trimmed,
    }
}

#[cfg(test)]
mod tests;

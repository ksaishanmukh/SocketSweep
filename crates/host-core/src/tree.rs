//! The scanned tree, built straight from the daemon's event stream.
//!
//! # Shape
//!
//! A flat `Vec<Node>` with child links held as indices, as in v3: nodes never
//! move, a [`NodeId`] is a position, and walking to a parent is an index. Files
//! are nodes too. A second dense vector maps each wire [`DirId`] to its node,
//! so an event costs one index, not a path lookup.
//!
//! # Sizes
//!
//! Every node has its **own** size (its `lstat`, from its parent's listing or
//! a later `Stats`), and a directory also has **sub** totals: everything
//! strictly below it, subdirectories' own sizes included, which is exactly the
//! wire [`Totals`] convention. What the GUI sees is `own + sub`, so a folder's
//! rows always add up to the folder.
//!
//! Sums climb as `Listing` and `Stats` arrive, each delta applied to every
//! ancestor (`Partial`). `SubtreeDone` replaces a directory's sub totals with
//! the daemon's and applies the difference up the chain (`Final`): the
//! daemon's figure wins. See `docs/architecture.md`, section 4.
//!
//! # Validation
//!
//! The ordering guarantees G1–G3 are checked as events arrive. A violation is
//! a [`TreeError::Protocol`], and the caller fails the scan: a tree built from
//! a stream it does not understand would show wrong numbers as if they were
//! right.
//!
//! # Deletes
//!
//! A subtree can disappear two ways: a `Pruned` event in the scan stream, or
//! [`Tree::remove`] once the phone confirms a delete. Whichever comes first
//! removes it; the other finds it gone and does nothing. The daemon's control
//! frames overtake its bulk queue, so events about directories inside a removed
//! subtree can still arrive afterwards; those directories are remembered as
//! gone and their events are dropped rather than treated as violations.

use std::collections::HashMap;

use socketsweep_gui_contract::{
    self as gui, AppUsage, Crumb, EntryKind, Measure, NodeId, NodeRef, Row, SessionId, Size,
    SizeMetric, TreemapNode, TypeGroup, View,
};
use socketsweep_wire::{DirId, Entry, Kind, Outcome, ScanEvent, Stat, Totals, ROOT_DIR};

/// The scan root's node.
pub const ROOT: NodeId = 0;

/// "No node" in a link. The root is 0, so 0 cannot mean absent.
const NONE: u32 = u32::MAX;

/// Why a tree operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// A node from another session's tree.
    Stale,
    /// The node existed but has been deleted.
    Removed,
    /// No such node in this tree.
    UnknownNode(NodeId),
    /// The scan root cannot be deleted.
    RootNotDeletable,
    /// The event stream broke an ordering guarantee or named something that
    /// does not exist. The scan must be failed.
    Protocol(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::Stale => write!(f, "These results are out of date. Rescan."),
            TreeError::Removed => write!(f, "That item has already been deleted."),
            TreeError::UnknownNode(id) => write!(f, "There is no item {id} in this scan."),
            TreeError::RootNotDeletable => write!(f, "The scan root cannot be deleted."),
            TreeError::Protocol(m) => write!(f, "The phone sent an invalid scan: {m}"),
        }
    }
}

impl std::error::Error for TreeError {}

fn protocol(m: impl Into<String>) -> TreeError {
    TreeError::Protocol(m.into())
}

#[derive(Debug)]
struct Node {
    name: Box<[u8]>,
    parent: NodeId,
    first_child: NodeId,
    last_child: NodeId,
    next_sibling: NodeId,
    kind: Kind,
    /// The walked directory's id, or [`NONE`].
    dir: DirId,
    /// This entry's own `lstat` sizes.
    own: Size,
    /// Strictly below; zero for anything but a directory.
    sub: Totals,
    measure: Measure,
    /// Its own `lstat` has arrived (or failed).
    stat_known: bool,
    /// Its `lstat` failed, which counts as one error in its parent.
    stat_failed: bool,
    removed: bool,
}

impl Node {
    fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    /// What the GUI shows: own plus everything below.
    fn size(&self) -> Size {
        Size {
            apparent: self.own.apparent.saturating_add(self.sub.apparent),
            disk: self.own.disk.saturating_add(self.sub.disk),
        }
    }

    /// Everything this node contributes to its parent's sub totals.
    fn contribution(&self) -> Totals {
        let mut t = self.sub;
        t.apparent = t.apparent.saturating_add(self.own.apparent);
        t.disk = t.disk.saturating_add(self.own.disk);
        if self.is_dir() {
            t.dirs = t.dirs.saturating_add(1);
        } else {
            t.files = t.files.saturating_add(1);
        }
        if self.stat_failed {
            t.errors = t.errors.saturating_add(1);
        }
        t
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirState {
    /// Introduced by its parent's listing (or `Started`), no listing yet.
    Introduced,
    /// Some listing parts have arrived.
    Listing,
    /// Every listing part has arrived.
    Listed,
    /// `SubtreeDone` arrived.
    Done,
    /// Deleted or pruned: later events about it are dropped.
    Gone,
}

#[derive(Debug)]
struct DirRec {
    node: NodeId,
    state: DirState,
    next_part: u16,
    /// Child nodes in listing order, for `Stats` positions. Dropped once done.
    entries: Vec<NodeId>,
    /// Walked child directories not yet done or gone, to check G2.
    open: u32,
}

/// Facts from the stream the tree does not keep in nodes, returned by
/// [`Tree::ingest`] for the caller to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingested {
    /// The event only changed the tree.
    Tree,
    SkeletonReady {
        listed: u32,
        frontier: u32,
        elapsed_ms: u32,
    },
    /// `node` is `None` when the directory has since been removed.
    FocusState {
        seq: u32,
        node: Option<NodeId>,
        settled: bool,
    },
    Progress {
        listed: u32,
        known: u32,
    },
    Finished {
        outcome: Outcome,
        totals: Totals,
        elapsed_ms: u32,
    },
}

/// One scan's tree. See the [module documentation](self).
pub struct Tree {
    session: SessionId,
    nodes: Vec<Node>,
    /// Indexed by [`DirId`]; `None` for an id not introduced yet.
    dirs: Vec<Option<DirRec>>,
    /// The root the daemon resolved: what wire paths are built from.
    root_path: Vec<u8>,
    /// The root as the user knows it (`/sdcard`): what display paths are
    /// built from.
    root_label: String,
    finished: Option<Outcome>,
}

impl Tree {
    /// A tree for a scan whose `Started` named `resolved_root`. `label` is the
    /// root as the user asked for it.
    pub fn new(session: SessionId, resolved_root: &[u8], label: &str) -> Self {
        let root = Node {
            name: basename(label.as_bytes()).to_vec().into_boxed_slice(),
            parent: NONE,
            first_child: NONE,
            last_child: NONE,
            next_sibling: NONE,
            kind: Kind::Dir,
            dir: ROOT_DIR,
            own: Size::default(),
            sub: Totals::default(),
            measure: Measure::Unknown,
            stat_known: true,
            stat_failed: false,
            removed: false,
        };
        Tree {
            session,
            nodes: vec![root],
            dirs: vec![Some(DirRec {
                node: ROOT,
                state: DirState::Introduced,
                next_part: 0,
                entries: Vec::new(),
                open: 0,
            })],
            root_path: resolved_root.to_vec(),
            root_label: label.to_string(),
            finished: None,
        }
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The scan has ended, and how.
    pub fn finished(&self) -> Option<Outcome> {
        self.finished
    }

    /// The root as the user knows it.
    pub fn root_label(&self) -> &str {
        &self.root_label
    }

    // ── Ingest ──────────────────────────────────────────────────────────────

    /// Apply one event. `Started` is not accepted: it creates the tree (see
    /// [`Tree::new`]), and a second one is a protocol error.
    pub fn ingest(&mut self, event: &ScanEvent) -> Result<Ingested, TreeError> {
        if self.finished.is_some() {
            return Err(protocol("an event arrived after the scan finished"));
        }
        match event {
            ScanEvent::Started { .. } => Err(protocol("a second Started event")),
            ScanEvent::Listing {
                dir,
                part,
                last,
                entries,
            } => self
                .listing(*dir, *part, *last, entries)
                .map(|_| Ingested::Tree),
            ScanEvent::Stats { dir, start, stats } => {
                self.stats(*dir, *start, stats).map(|_| Ingested::Tree)
            }
            ScanEvent::DirFailed { dir, .. } => self.dir_failed(*dir).map(|_| Ingested::Tree),
            ScanEvent::SubtreeDone { dir, totals } => {
                self.subtree_done(*dir, totals).map(|_| Ingested::Tree)
            }
            ScanEvent::Pruned { dir } => self.pruned(*dir).map(|_| Ingested::Tree),
            ScanEvent::SkeletonReady {
                listed,
                frontier,
                elapsed_ms,
            } => Ok(Ingested::SkeletonReady {
                listed: *listed,
                frontier: *frontier,
                elapsed_ms: *elapsed_ms,
            }),
            ScanEvent::FocusState { seq, dir, settled } => {
                let rec = self.rec(*dir)?;
                let node = (rec.state != DirState::Gone).then_some(rec.node);
                Ok(Ingested::FocusState {
                    seq: *seq,
                    node,
                    settled: *settled,
                })
            }
            ScanEvent::Progress { listed, known, .. } => Ok(Ingested::Progress {
                listed: *listed,
                known: *known,
            }),
            ScanEvent::Finished {
                outcome,
                totals,
                elapsed_ms,
            } => {
                self.finished = Some(*outcome);
                Ok(Ingested::Finished {
                    outcome: *outcome,
                    totals: *totals,
                    elapsed_ms: *elapsed_ms,
                })
            }
        }
    }

    /// The record for `dir`, which must have been introduced (G1).
    fn rec(&self, dir: DirId) -> Result<&DirRec, TreeError> {
        self.dirs
            .get(dir as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| {
                protocol(format!(
                    "directory {dir} was referred to before it was introduced"
                ))
            })
    }

    fn rec_mut(&mut self, dir: DirId) -> Result<&mut DirRec, TreeError> {
        self.dirs
            .get_mut(dir as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| {
                protocol(format!(
                    "directory {dir} was referred to before it was introduced"
                ))
            })
    }

    /// Record that `dir` now exists, as `node` (or as gone).
    fn introduce(&mut self, dir: DirId, node: NodeId, state: DirState) -> Result<(), TreeError> {
        let i = dir as usize;
        if i >= self.dirs.len() {
            self.dirs.resize_with(i + 1, || None);
        }
        if self.dirs[i].is_some() {
            return Err(protocol(format!("directory {dir} was introduced twice")));
        }
        self.dirs[i] = Some(DirRec {
            node,
            state,
            next_part: 0,
            entries: Vec::new(),
            open: 0,
        });
        Ok(())
    }

    fn listing(
        &mut self,
        dir: DirId,
        part: u16,
        last: bool,
        entries: &[Entry],
    ) -> Result<(), TreeError> {
        let rec = self.rec(dir)?;
        if rec.state == DirState::Gone {
            // Its subtree is gone, but the ids it introduces may still be
            // referred to by events already on their way.
            for id in entries.iter().filter_map(|e| e.id) {
                self.introduce(id, NONE, DirState::Gone)?;
            }
            return Ok(());
        }
        match rec.state {
            DirState::Introduced | DirState::Listing if part == rec.next_part => {}
            DirState::Introduced | DirState::Listing => {
                return Err(protocol(format!(
                    "listing part {part} of directory {dir} arrived when part {} was expected",
                    rec.next_part
                )))
            }
            _ => return Err(protocol(format!("directory {dir} was listed twice"))),
        }
        let parent = rec.node;

        let mut delta = Totals::default();
        let mut added = Vec::with_capacity(entries.len());
        let mut opened = 0u32;
        for entry in entries {
            let walked = entry.id.is_some();
            if walked && entry.kind != Kind::Dir {
                return Err(protocol(format!(
                    "an entry of directory {dir} that is not a directory carries an id"
                )));
            }
            let measure = match (entry.kind, walked, entry.stat) {
                (Kind::Dir, true, _) => Measure::Unknown,
                // A directory that will not be walked has nothing more coming.
                (Kind::Dir, false, _) => Measure::Final,
                (_, _, Some(_)) => Measure::Final,
                (_, _, None) => Measure::Unknown,
            };
            let own = entry.stat.as_ref().map(size_of).unwrap_or_default();
            let id = self.push_node(parent, entry, own, measure);
            if let Some(child_dir) = entry.id {
                self.introduce(child_dir, id, DirState::Introduced)?;
                self.nodes[id as usize].dir = child_dir;
                opened += 1;
            }
            if entry.kind == Kind::Dir {
                delta.dirs += 1;
            } else {
                delta.files += 1;
            }
            delta.apparent += own.apparent;
            delta.disk += own.disk;
            added.push(id);
        }

        let rec = self.rec_mut(dir)?;
        rec.entries.extend(added);
        rec.open += opened;
        rec.next_part = part.saturating_add(1);
        rec.state = if last {
            DirState::Listed
        } else {
            DirState::Listing
        };

        let n = &mut self.nodes[parent as usize];
        if n.measure == Measure::Unknown {
            n.measure = Measure::Partial;
        }
        self.add_up(parent, &delta);
        Ok(())
    }

    fn push_node(&mut self, parent: NodeId, entry: &Entry, own: Size, measure: Measure) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            name: entry.name.clone().into_boxed_slice(),
            parent,
            first_child: NONE,
            last_child: NONE,
            next_sibling: NONE,
            kind: entry.kind,
            dir: NONE,
            own,
            sub: Totals::default(),
            measure,
            stat_known: entry.stat.is_some(),
            stat_failed: false,
            removed: false,
        });
        let last = self.nodes[parent as usize].last_child;
        if last == NONE {
            self.nodes[parent as usize].first_child = id;
        } else {
            self.nodes[last as usize].next_sibling = id;
        }
        self.nodes[parent as usize].last_child = id;
        id
    }

    fn stats(&mut self, dir: DirId, start: u32, stats: &[Option<Stat>]) -> Result<(), TreeError> {
        let rec = self.rec(dir)?;
        match rec.state {
            DirState::Gone => return Ok(()),
            DirState::Listed => {}
            DirState::Done => {
                return Err(protocol(format!(
                    "stats for directory {dir} arrived after it was finished"
                )))
            }
            _ => {
                return Err(protocol(format!(
                    "stats for directory {dir} arrived before its listing was complete"
                )))
            }
        }
        let parent = rec.node;
        let end = start as usize + stats.len();
        if end > rec.entries.len() {
            return Err(protocol(format!(
                "stats for entries {start}..{end} of directory {dir}, which has {}",
                rec.entries.len()
            )));
        }
        let targets: Vec<NodeId> = rec.entries[start as usize..end].to_vec();

        let mut delta = Totals::default();
        for (node, stat) in targets.into_iter().zip(stats) {
            let n = &mut self.nodes[node as usize];
            if n.stat_known {
                return Err(protocol(format!(
                    "a second stat for an entry of directory {dir}"
                )));
            }
            n.stat_known = true;
            if !n.is_dir() {
                n.measure = Measure::Final;
            }
            if n.removed {
                continue;
            }
            match stat {
                Some(stat) => {
                    n.own = size_of(stat);
                    delta.apparent += n.own.apparent;
                    delta.disk += n.own.disk;
                }
                None => {
                    n.stat_failed = true;
                    delta.errors += 1;
                }
            }
        }
        self.add_up(parent, &delta);
        Ok(())
    }

    fn dir_failed(&mut self, dir: DirId) -> Result<(), TreeError> {
        let rec = self.rec(dir)?;
        match rec.state {
            DirState::Gone => Ok(()),
            DirState::Done => Err(protocol(format!(
                "directory {dir} failed after it was finished"
            ))),
            _ => {
                let node = rec.node;
                let delta = Totals {
                    flags: Totals::UNREADABLE,
                    ..Totals::default()
                };
                self.add_up(node, &delta);
                Ok(())
            }
        }
    }

    fn subtree_done(&mut self, dir: DirId, totals: &Totals) -> Result<(), TreeError> {
        let rec = self.rec(dir)?;
        match rec.state {
            DirState::Gone => return Ok(()),
            DirState::Done => return Err(protocol(format!("directory {dir} was finished twice"))),
            DirState::Listing => {
                return Err(protocol(format!(
                    "directory {dir} was finished part-way through its listing"
                )))
            }
            DirState::Introduced | DirState::Listed => {}
        }
        if rec.open > 0 {
            return Err(protocol(format!(
                "directory {dir} was finished before {} of its subdirectories",
                rec.open
            )));
        }
        let node = rec.node;

        let rec = self.rec_mut(dir)?;
        rec.state = DirState::Done;
        rec.entries = Vec::new();

        let n = &mut self.nodes[node as usize];
        let old = n.sub;
        n.sub = *totals;
        n.measure = Measure::Final;
        let parent = n.parent;
        self.replace_up(parent, &old, totals);
        self.close_in_parent(node);
        Ok(())
    }

    fn pruned(&mut self, dir: DirId) -> Result<(), TreeError> {
        let rec = self.rec(dir)?;
        if rec.state == DirState::Gone {
            return Ok(());
        }
        if dir == ROOT_DIR {
            return Err(protocol("the scan root was pruned"));
        }
        let node = rec.node;
        self.remove_subtree(node);
        Ok(())
    }

    /// Mark `node`'s directory as no longer open in its parent's record.
    fn close_in_parent(&mut self, node: NodeId) {
        let parent = self.nodes[node as usize].parent;
        if parent == NONE {
            return;
        }
        let pdir = self.nodes[parent as usize].dir;
        if let Some(Some(prec)) = self.dirs.get_mut(pdir as usize) {
            prec.open = prec.open.saturating_sub(1);
        }
    }

    /// Add `delta` to the sub totals of `from` and every ancestor.
    fn add_up(&mut self, from: NodeId, delta: &Totals) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.sub.add(delta);
            cur = n.parent;
        }
    }

    /// Subtract `delta` from the sub totals of `from` and every ancestor.
    fn sub_up(&mut self, from: NodeId, delta: &Totals) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.sub.subtract(delta);
            cur = n.parent;
        }
    }

    /// Replace `old` by `new` in the sub totals of `from` and every ancestor.
    fn replace_up(&mut self, from: NodeId, old: &Totals, new: &Totals) {
        let mut cur = from;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.sub.subtract(old);
            n.sub.add(new);
            cur = n.parent;
        }
    }

    /// Detach `node` and everything below it, discounting it from every
    /// ancestor. Directories inside it are remembered as gone.
    fn remove_subtree(&mut self, node: NodeId) {
        if self.nodes[node as usize].removed {
            return;
        }
        let contribution = self.nodes[node as usize].contribution();
        let parent = self.nodes[node as usize].parent;
        let was_open = self.nodes[node as usize].dir != NONE
            && self
                .dirs
                .get(self.nodes[node as usize].dir as usize)
                .and_then(Option::as_ref)
                .is_some_and(|r| r.state != DirState::Done);

        self.unlink(parent, node);
        let mut stack = vec![node];
        while let Some(id) = stack.pop() {
            let n = &mut self.nodes[id as usize];
            if n.removed {
                continue;
            }
            n.removed = true;
            let dir = n.dir;
            let mut child = n.first_child;
            while child != NONE {
                stack.push(child);
                child = self.nodes[child as usize].next_sibling;
            }
            if dir != NONE {
                if let Some(Some(rec)) = self.dirs.get_mut(dir as usize) {
                    rec.state = DirState::Gone;
                    rec.entries = Vec::new();
                }
            }
        }
        self.sub_up(parent, &contribution);
        if was_open {
            self.close_in_parent(node);
        }
    }

    fn unlink(&mut self, parent: NodeId, id: NodeId) {
        let next = self.nodes[id as usize].next_sibling;
        if self.nodes[parent as usize].first_child == id {
            self.nodes[parent as usize].first_child = next;
            if self.nodes[parent as usize].last_child == id {
                self.nodes[parent as usize].last_child = NONE;
            }
            return;
        }
        let mut cur = self.nodes[parent as usize].first_child;
        while cur != NONE {
            let after = self.nodes[cur as usize].next_sibling;
            if after == id {
                self.nodes[cur as usize].next_sibling = next;
                if self.nodes[parent as usize].last_child == id {
                    self.nodes[parent as usize].last_child = cur;
                }
                return;
            }
            cur = after;
        }
    }

    // ── Nodes and identity ──────────────────────────────────────────────────

    /// The node a [`NodeRef`] names, if it is from this tree and still in it.
    pub fn resolve(&self, node: NodeRef) -> Result<NodeId, TreeError> {
        if node.session != self.session {
            return Err(TreeError::Stale);
        }
        self.live(node.id)?;
        Ok(node.id)
    }

    fn live(&self, id: NodeId) -> Result<&Node, TreeError> {
        match self.nodes.get(id as usize) {
            None => Err(TreeError::UnknownNode(id)),
            Some(n) if n.removed => Err(TreeError::Removed),
            Some(n) => Ok(n),
        }
    }

    pub fn node_ref(&self, id: NodeId) -> NodeRef {
        NodeRef {
            session: self.session,
            id,
        }
    }

    pub fn exists(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(|n| !n.removed)
    }

    pub fn is_dir(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(Node::is_dir)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes.get(id as usize)?.parent;
        (p != NONE).then_some(p)
    }

    /// The node of a walked directory, if it is still in the tree.
    pub fn dir_node(&self, dir: DirId) -> Option<NodeId> {
        let rec = self.dirs.get(dir as usize)?.as_ref()?;
        (rec.state != DirState::Gone).then_some(rec.node)
    }

    /// What to focus for `id`: its own [`DirId`] if it is a walked directory,
    /// otherwise that of its nearest walked ancestor (a file means its
    /// folder). The root is [`ROOT_DIR`].
    pub fn focus_dir(&self, id: NodeId) -> Result<(NodeId, DirId), TreeError> {
        self.live(id)?;
        let mut cur = id;
        loop {
            let n = &self.nodes[cur as usize];
            if n.dir != NONE {
                return Ok((cur, n.dir));
            }
            cur = n.parent;
            if cur == NONE {
                return Ok((ROOT, ROOT_DIR));
            }
        }
    }

    /// The walked directory `id` is, if any.
    pub fn dir_of(&self, id: NodeId) -> Option<DirId> {
        let n = self.nodes.get(id as usize)?;
        (n.dir != NONE && !n.removed).then_some(n.dir)
    }

    /// `id` if it is still in the tree, otherwise its nearest ancestor that
    /// is. Where the GUI lands after the folder it showed is deleted.
    pub fn nearest_live(&self, id: NodeId) -> NodeId {
        if id as usize >= self.nodes.len() {
            return ROOT;
        }
        let mut cur = id;
        while cur != NONE && self.nodes[cur as usize].removed {
            cur = self.nodes[cur as usize].parent;
        }
        if cur == NONE {
            ROOT
        } else {
            cur
        }
    }

    /// The byte-exact phone path of `id`, built from the resolved root.
    pub fn path_of(&self, id: NodeId) -> Result<Vec<u8>, TreeError> {
        self.live(id)?;
        let mut out = self.root_path.clone();
        for name in self.names_from_root(id) {
            if out.last() != Some(&b'/') {
                out.push(b'/');
            }
            out.extend_from_slice(name);
        }
        Ok(out)
    }

    /// The path to delete for `id`: a live node of this tree, not the root.
    pub fn delete_target(&self, node: NodeRef) -> Result<Vec<u8>, TreeError> {
        let id = self.resolve(node)?;
        if id == ROOT {
            return Err(TreeError::RootNotDeletable);
        }
        self.path_of(id)
    }

    /// The path as the user knows it, from the root label, lossily.
    pub fn display_path(&self, id: NodeId) -> String {
        let mut out = self.root_label.clone();
        for name in self.names_from_root(id) {
            if !out.ends_with('/') {
                out.push('/');
            }
            out.push_str(&String::from_utf8_lossy(name));
        }
        out
    }

    fn names_from_root(&self, id: NodeId) -> Vec<&[u8]> {
        let mut names = Vec::new();
        let mut cur = id;
        while cur != ROOT && cur != NONE {
            let n = &self.nodes[cur as usize];
            names.push(&*n.name);
            cur = n.parent;
        }
        names.reverse();
        names
    }

    /// Remove `id` and everything below it, once the phone confirms the
    /// delete. Already removed (by a `Pruned`) is not an error.
    pub fn remove(&mut self, id: NodeId) -> Result<(), TreeError> {
        if id == ROOT {
            return Err(TreeError::RootNotDeletable);
        }
        if id as usize >= self.nodes.len() {
            return Err(TreeError::UnknownNode(id));
        }
        self.remove_subtree(id);
        Ok(())
    }

    // ── Queries ─────────────────────────────────────────────────────────────

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

    fn files_of(n: &Node) -> u32 {
        if n.is_dir() {
            n.sub.files
        } else {
            1
        }
    }

    fn row(&self, id: NodeId) -> Row {
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

    /// Largest first by `by`, then by name so equal rows do not swap places
    /// between refreshes.
    fn sort(&self, ids: &mut [NodeId], by: SizeMetric) {
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

    /// Total size per broad file category, largest first.
    pub fn type_breakdown(&self, by: SizeMetric) -> Vec<TypeGroup> {
        let mut sizes = [Size::default(); CATEGORIES.len()];
        let mut counts = [0u32; CATEGORIES.len()];
        for n in self.nodes.iter().filter(|n| !n.is_dir() && !n.removed) {
            let c = category_of(&n.name);
            sizes[c].apparent += n.own.apparent;
            sizes[c].disk += n.own.disk;
            counts[c] += 1;
        }
        let mut groups: Vec<TypeGroup> = CATEGORIES
            .iter()
            .enumerate()
            .filter(|(i, _)| counts[*i] > 0)
            .map(|(i, label)| TypeGroup {
                label: (*label).to_string(),
                size: sizes[i],
                files: counts[i],
            })
            .collect();
        groups.sort_by(|a, b| {
            metric(b.size, by)
                .cmp(&metric(a.size, by))
                .then_with(|| a.label.cmp(&b.label))
        });
        groups
    }

    fn child_named(&self, parent: NodeId, name: &[u8]) -> Option<NodeId> {
        let mut cur = self.nodes.get(parent as usize)?.first_child;
        while cur != NONE {
            let n = &self.nodes[cur as usize];
            if !n.removed && &*n.name == name {
                return Some(cur);
            }
            cur = n.next_sibling;
        }
        None
    }

    /// Storage per app package, from `Android/{data,obb,media}/<package>`.
    /// Location identifies the owner where no extension could.
    pub fn app_breakdown(&self, limit: u32, by: SizeMetric) -> Vec<AppUsage> {
        const AREAS: [&[u8]; 3] = [b"data", b"obb", b"media"];
        let Some(android) = self.child_named(ROOT, b"Android") else {
            return Vec::new();
        };
        // package -> (size, files, largest folder, its size)
        let mut totals: HashMap<&[u8], (Size, u32, NodeId, u64)> = HashMap::new();
        for area in AREAS {
            let Some(area_id) = self.child_named(android, area) else {
                continue;
            };
            for pkg in self.children_of(area_id) {
                let n = &self.nodes[pkg as usize];
                if !n.is_dir() {
                    continue;
                }
                let size = n.size();
                let slot = totals
                    .entry(&n.name)
                    .or_insert((Size::default(), 0, pkg, 0));
                slot.0.apparent += size.apparent;
                slot.0.disk += size.disk;
                slot.1 += n.sub.files;
                if metric(size, by) > slot.3 {
                    slot.2 = pkg;
                    slot.3 = metric(size, by);
                }
            }
        }
        let mut rows: Vec<AppUsage> = totals
            .into_iter()
            .filter(|(_, (size, ..))| metric(*size, by) > 0)
            .map(|(name, (size, files, node, _))| AppUsage {
                package: String::from_utf8_lossy(name).into_owned(),
                size,
                files,
                node: self.node_ref(node),
            })
            .collect();
        rows.sort_by(|a, b| {
            metric(b.size, by)
                .cmp(&metric(a.size, by))
                .then_with(|| a.package.cmp(&b.package))
        });
        rows.truncate(limit as usize);
        rows
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

fn size_of(stat: &Stat) -> Size {
    Size {
        apparent: stat.apparent,
        disk: stat.disk(),
    }
}

/// The figure a [`SizeMetric`] selects.
pub fn metric(size: Size, by: SizeMetric) -> u64 {
    match by {
        SizeMetric::Disk => size.disk,
        SizeMetric::Apparent => size.apparent,
    }
}

fn entry_kind(kind: Kind) -> EntryKind {
    match kind {
        Kind::File => EntryKind::File,
        Kind::Dir => EntryKind::Dir,
        Kind::Symlink => EntryKind::Symlink,
        Kind::Other => EntryKind::Other,
    }
}

/// The last component of `path`, ignoring trailing slashes.
pub(crate) fn basename(path: &[u8]) -> &[u8] {
    let trimmed = match path.iter().rposition(|b| *b != b'/') {
        Some(end) => &path[..=end],
        None => return path,
    };
    match trimmed.iter().rposition(|b| *b == b'/') {
        Some(slash) => &trimmed[slash + 1..],
        None => trimmed,
    }
}

// ── File-type classification ────────────────────────────────────────────────

/// Broad categories, deliberately coarse: "24 GB of video" is actionable.
const CATEGORIES: [&str; 7] = [
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
    let other = CATEGORIES.len() - 1;
    let Some(dot) = name.iter().rposition(|b| *b == b'.') else {
        return other;
    };
    if dot == 0 || dot + 1 >= name.len() {
        return other;
    }
    let ext = String::from_utf8_lossy(&name[dot + 1..]).to_lowercase();
    [PHOTOS, VIDEO, AUDIO, APPS, DOCS, ARCHIVES]
        .iter()
        .position(|list| list.contains(&ext.as_str()))
        .unwrap_or(other)
}

#[cfg(test)]
mod tests;

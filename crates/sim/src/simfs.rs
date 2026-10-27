//! `SimFs`: a seeded, in-memory file tree that implements the engine's
//! [`Fs`], counts every listing and stat, supports deletes during a scan,
//! and knows its own ground-truth totals.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use socketsweep_engine::{
    errno, DirRead, Fs, FsEntry, FsError, FsStat, Interrupt, RootInfo, StatResult,
};
use socketsweep_wire::{FsOp, Kind, Stat, Totals};

use crate::rng::Rng;

pub type NodeId = u32;

/// One file system object.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: Vec<u8>,
    pub parent: NodeId,
    pub kind: Kind,
    pub apparent: u64,
    pub blocks: u64,
    pub mtime: i64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    /// `getdents` reports `DT_UNKNOWN`, so a names listing must stat it.
    pub unknown_dtype: bool,
    /// Opening this directory fails with this errno.
    pub fail_open: Option<i32>,
    /// `lstat` of this entry fails with this errno.
    pub fail_stat: Option<i32>,
    /// Sorted by name once built.
    pub children: Vec<NodeId>,
    /// The order `getdents` returns the children in: by a hash of the name,
    /// as on ext4 (htree) and the FUSE layer above it.
    pub readdir: Vec<NodeId>,
}

/// Builds a [`SimFs`].
#[derive(Debug)]
pub struct TreeBuilder {
    nodes: Vec<Node>,
}

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeBuilder {
    /// A tree with just its root directory (node 0, device 1).
    pub fn new() -> Self {
        let mut b = TreeBuilder { nodes: Vec::new() };
        b.push(0, Vec::new(), Kind::Dir, 4096);
        b
    }

    fn push(&mut self, parent: NodeId, name: Vec<u8>, kind: Kind, size: u64) -> NodeId {
        let id = self.nodes.len() as NodeId;
        let dev = if id == 0 {
            1
        } else {
            self.nodes[parent as usize].dev
        };
        self.nodes.push(Node {
            name,
            parent,
            kind,
            apparent: size,
            blocks: size.div_ceil(4096) * 8,
            mtime: 1_700_000_000 + i64::from(id),
            dev,
            ino: u64::from(id) + 2,
            nlink: 1,
            unknown_dtype: false,
            fail_open: None,
            fail_stat: None,
            children: Vec::new(),
            readdir: Vec::new(),
        });
        if id != 0 {
            self.nodes[parent as usize].children.push(id);
        }
        id
    }

    pub fn dir(&mut self, parent: NodeId, name: impl Into<Vec<u8>>) -> NodeId {
        self.push(parent, name.into(), Kind::Dir, 4096)
    }

    pub fn file(&mut self, parent: NodeId, name: impl Into<Vec<u8>>, size: u64) -> NodeId {
        self.push(parent, name.into(), Kind::File, size)
    }

    pub fn symlink(&mut self, parent: NodeId, name: impl Into<Vec<u8>>, target_len: u64) -> NodeId {
        let id = self.push(parent, name.into(), Kind::Symlink, target_len);
        self.nodes[id as usize].blocks = 0;
        id
    }

    /// Another link to file `of`.
    pub fn hardlink(&mut self, parent: NodeId, name: impl Into<Vec<u8>>, of: NodeId) -> NodeId {
        let src = self.nodes[of as usize].clone();
        let id = self.push(parent, name.into(), Kind::File, src.apparent);
        let n = &mut self.nodes[id as usize];
        n.ino = src.ino;
        n.dev = src.dev;
        n.blocks = src.blocks;
        id
    }

    /// Make `d` a mount point of another file system (it and everything
    /// below it get device `dev`).
    pub fn mount(&mut self, d: NodeId, dev: u64) {
        let mut stack = vec![d];
        while let Some(x) = stack.pop() {
            self.nodes[x as usize].dev = dev;
            stack.extend(self.nodes[x as usize].children.iter().copied());
        }
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn build(mut self, root_path: &[u8]) -> SimFs {
        let mut links: HashMap<(u64, u64), u64> = HashMap::new();
        for n in &self.nodes {
            *links.entry((n.dev, n.ino)).or_default() += 1;
        }
        for n in &mut self.nodes {
            n.nlink = links[&(n.dev, n.ino)];
        }
        let names: Vec<Vec<u8>> = self.nodes.iter().map(|n| n.name.clone()).collect();
        for n in &mut self.nodes {
            n.readdir = n.children.clone();
            n.readdir.sort_by_key(|&c| name_hash(&names[c as usize]));
            n.children
                .sort_by(|a, b| names[*a as usize].cmp(&names[*b as usize]));
            debug_assert!(
                n.children
                    .windows(2)
                    .all(|w| names[w[0] as usize] != names[w[1] as usize]),
                "duplicate name in a directory"
            );
        }
        let n = self.nodes.len();
        let mut fs = SimFs {
            root_path: root_path.to_vec(),
            nodes: self.nodes,
            paths: HashMap::new(),
            deleted: (0..n).map(|_| AtomicBool::new(false)).collect(),
            lists: (0..n).map(|_| AtomicU32::new(0)).collect(),
            stats: (0..n).map(|_| AtomicU32::new(0)).collect(),
        };
        for id in 0..n as NodeId {
            if fs.nodes[id as usize].kind == Kind::Dir {
                let p = fs.path_of(id);
                fs.paths.insert(p, id);
            }
        }
        fs
    }
}

/// Ground truth for one set of scan options.
#[derive(Debug, Clone)]
pub struct Truth {
    /// Totals of each directory the engine walks (`None` otherwise).
    pub totals: Vec<Option<Totals>>,
    /// The engine must read this directory exactly once.
    pub listed: Vec<bool>,
    /// The engine must stat this entry exactly once.
    pub stated: Vec<bool>,
    /// Entries of a directory on another file system: the engine reads that
    /// directory once before it can see its device, so they may have been
    /// stat'ed once (or not at all).
    pub maybe: Vec<bool>,
}

impl Truth {
    pub fn root(&self) -> Totals {
        self.totals[0].expect("root is walked")
    }
}

/// See the module docs.
#[derive(Debug)]
pub struct SimFs {
    root_path: Vec<u8>,
    nodes: Vec<Node>,
    paths: HashMap<Vec<u8>, NodeId>,
    deleted: Vec<AtomicBool>,
    lists: Vec<AtomicU32>,
    stats: Vec<AtomicU32>,
}

impl SimFs {
    pub fn root_path(&self) -> &[u8] {
        &self.root_path
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn is_deleted(&self, id: NodeId) -> bool {
        self.deleted[id as usize].load(Ordering::Relaxed)
    }

    /// Live children of a directory.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id as usize]
            .children
            .iter()
            .copied()
            .filter(|&c| !self.is_deleted(c))
    }

    pub fn path_of(&self, id: NodeId) -> Vec<u8> {
        let mut parts = Vec::new();
        let mut x = id;
        while x != 0 {
            parts.push(&self.nodes[x as usize].name);
            x = self.nodes[x as usize].parent;
        }
        let mut p = self.root_path.clone();
        for n in parts.iter().rev() {
            if p.last() != Some(&b'/') {
                p.push(b'/');
            }
            p.extend_from_slice(n);
        }
        p
    }

    /// A live directory by path.
    pub fn lookup(&self, path: &[u8]) -> Option<NodeId> {
        self.paths
            .get(path)
            .copied()
            .filter(|&d| !self.is_deleted(d))
    }

    /// A live node of any kind by path.
    pub fn find(&self, path: &[u8]) -> Option<NodeId> {
        if let Some(d) = self.lookup(path) {
            return Some(d);
        }
        let slash = path.iter().rposition(|&b| b == b'/')?;
        let (parent, name) = (&path[..slash], &path[slash + 1..]);
        let parent = if parent.is_empty() {
            b"/" as &[u8]
        } else {
            parent
        };
        let p = self.lookup(parent)?;
        let kids = &self.nodes[p as usize].children;
        kids.binary_search_by(|&c| self.nodes[c as usize].name.as_slice().cmp(name))
            .ok()
            .map(|i| kids[i])
            .filter(|&c| !self.is_deleted(c))
    }

    /// Delete a file, or a directory and everything below it. Returns
    /// whether it existed. The root is never deleted.
    pub fn delete(&self, path: &[u8]) -> bool {
        let Some(d) = self.find(path) else {
            return false;
        };
        if d == 0 {
            return false;
        }
        let mut stack = vec![d];
        while let Some(x) = stack.pop() {
            self.deleted[x as usize].store(true, Ordering::Relaxed);
            stack.extend(self.nodes[x as usize].children.iter().copied());
        }
        true
    }

    /// Times each node was read as a directory.
    pub fn lists(&self, id: NodeId) -> u32 {
        self.lists[id as usize].load(Ordering::Relaxed)
    }

    /// Times each node was stat'ed.
    pub fn stats(&self, id: NodeId) -> u32 {
        self.stats[id as usize].load(Ordering::Relaxed)
    }

    pub fn reset_counters(&self) {
        for c in self.lists.iter().chain(&self.stats) {
            c.store(0, Ordering::Relaxed);
        }
    }

    /// Directories in a live subtree, itself included.
    pub fn subtree_dirs(&self, id: NodeId) -> u64 {
        let mut n = 0;
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            n += 1;
            stack.extend(
                self.children(x)
                    .filter(|&c| self.nodes[c as usize].kind == Kind::Dir),
            );
        }
        n
    }

    /// Disk usage of a live subtree, itself excluded.
    pub fn subtree_disk(&self, id: NodeId) -> u64 {
        let mut n = 0;
        let mut stack: Vec<NodeId> = self.children(id).collect();
        while let Some(x) = stack.pop() {
            n += self.nodes[x as usize].blocks * 512;
            if self.nodes[x as usize].kind == Kind::Dir {
                stack.extend(self.children(x));
            }
        }
        n
    }

    /// Check the counters against `truth`: every directory the engine had to
    /// read was read exactly once and every entry it had to stat was stat'ed
    /// exactly once, and nothing else at all. Nodes deleted during the scan
    /// may have been touched at most once.
    pub fn check_counters(&self, truth: &Truth) -> Result<(), String> {
        for id in 0..self.nodes.len() {
            let (l, s) = (self.lists(id as NodeId), self.stats(id as NodeId));
            let path = || String::from_utf8_lossy(&self.path_of(id as NodeId)).into_owned();
            if self.is_deleted(id as NodeId) || truth.maybe[id] {
                // Deleted during the scan, or below a mount point: touched at
                // most once.
                if l > 1 || s > 1 {
                    return Err(format!(
                        "{} (deleted): listed {l}, stat'ed {s} times",
                        path()
                    ));
                }
                continue;
            }
            if l != u32::from(truth.listed[id]) {
                return Err(format!(
                    "{}: listed {l} times (expected {})",
                    path(),
                    u8::from(truth.listed[id])
                ));
            }
            if s != u32::from(truth.stated[id]) {
                return Err(format!(
                    "{}: stat'ed {s} times (expected {})",
                    path(),
                    u8::from(truth.stated[id])
                ));
            }
        }
        Ok(())
    }

    /// Ground truth over the live tree for a scan with these options.
    /// Hardlinks are counted once each in a depth-first order, so per-directory
    /// totals of trees with hardlinks are only meaningful at the root.
    pub fn truth(&self, max_depth: u16, one_filesystem: bool) -> Truth {
        let n = self.nodes.len();
        let mut t = Truth {
            totals: vec![None; n],
            listed: vec![false; n],
            stated: vec![false; n],
            maybe: vec![false; n],
        };
        let mut links = HashSet::new();
        let root_dev = self.nodes[0].dev;
        self.truth_dir(
            0,
            0,
            max_depth,
            one_filesystem,
            root_dev,
            &mut links,
            &mut t,
        );
        t
    }

    #[allow(clippy::too_many_arguments)]
    fn truth_dir(
        &self,
        d: NodeId,
        depth: u16,
        max_depth: u16,
        one_fs: bool,
        root_dev: u64,
        links: &mut HashSet<(u64, u64)>,
        t: &mut Truth,
    ) -> Totals {
        let node = &self.nodes[d as usize];
        t.listed[d as usize] = true;
        let mut tot = Totals::default();
        if node.fail_open.is_some() {
            tot.errors = 1;
            tot.flags = Totals::UNREADABLE;
        } else if one_fs && d != 0 && node.dev != root_dev {
            tot.flags = Totals::OTHER_FS;
            for c in self.children(d) {
                t.maybe[c as usize] = true;
            }
        } else {
            for c in self.children(d) {
                let cn = &self.nodes[c as usize];
                t.stated[c as usize] = true;
                let is_dir = cn.kind == Kind::Dir && !(cn.unknown_dtype && cn.fail_stat.is_some());
                if is_dir {
                    tot.dirs += 1;
                } else {
                    tot.files += 1;
                }
                if cn.fail_stat.is_some() {
                    tot.errors += 1;
                } else if cn.kind == Kind::Dir || cn.nlink <= 1 || links.insert((cn.dev, cn.ino)) {
                    tot.apparent += cn.apparent;
                    tot.disk += cn.blocks * 512;
                }
                if is_dir {
                    if depth + 1 > max_depth {
                        tot.flags |= Totals::DEPTH_LIMIT;
                    } else {
                        let sub =
                            self.truth_dir(c, depth + 1, max_depth, one_fs, root_dev, links, t);
                        tot.add(&sub);
                    }
                }
            }
        }
        t.totals[d as usize] = Some(tot);
        tot
    }

    fn stat_node(&self, id: NodeId) -> StatResult {
        self.stats[id as usize].fetch_add(1, Ordering::Relaxed);
        let n = &self.nodes[id as usize];
        match n.fail_stat {
            Some(e) => Err(e),
            None => Ok(FsStat {
                kind: n.kind,
                stat: Stat {
                    apparent: n.apparent,
                    blocks: n.blocks,
                    mtime: n.mtime,
                },
                dev: n.dev,
                ino: n.ino,
                nlink: n.nlink,
            }),
        }
    }

    fn open(&self, path: &[u8]) -> Result<NodeId, FsError> {
        let d = self.lookup(path).ok_or(FsError::Failed {
            op: FsOp::Open,
            errno: errno::ENOENT,
        })?;
        Ok(d)
    }

    fn read(&self, path: &[u8], full: bool, intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        let d = self.open(path)?;
        self.lists[d as usize].fetch_add(1, Ordering::Relaxed);
        let node = &self.nodes[d as usize];
        if let Some(e) = node.fail_open {
            return Err(FsError::Failed {
                op: FsOp::Open,
                errno: e,
            });
        }
        let mut entries = Vec::with_capacity(node.children.len());
        let live = node
            .readdir
            .iter()
            .copied()
            .filter(|&c| !self.is_deleted(c));
        for (i, c) in live.enumerate() {
            intr.poll(i)?;
            let cn = &self.nodes[c as usize];
            let (kind, stat) = if full || cn.unknown_dtype {
                let st = self.stat_node(c);
                let kind = match (&st, cn.unknown_dtype) {
                    (Ok(s), _) => s.kind,
                    (Err(_), true) => Kind::Other,
                    (Err(_), false) => cn.kind,
                };
                (kind, Some(st))
            } else {
                (cn.kind, None)
            };
            entries.push(FsEntry {
                name: cn.name.clone(),
                kind,
                stat,
            });
        }
        Ok(DirRead {
            dev: node.dev,
            entries,
        })
    }
}

impl Fs for SimFs {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        let d = self.open(path)?;
        Ok(RootInfo {
            path: path.to_vec(),
            dev: self.nodes[d as usize].dev,
        })
    }

    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        self.read(dir, false, intr)
    }

    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError> {
        let d = self.open(dir)?;
        let kids = &self.nodes[d as usize].children;
        let mut out = Vec::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            intr.poll(i)?;
            let found = kids
                .binary_search_by(|&c| self.nodes[c as usize].name.as_slice().cmp(name))
                .ok()
                .map(|i| kids[i])
                .filter(|&c| !self.is_deleted(c));
            out.push(match found {
                Some(c) => self.stat_node(c),
                None => Err(errno::ENOENT),
            });
        }
        Ok(out)
    }

    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        self.read(dir, true, intr)
    }
}

// ── Generators ──────────────────────────────────────────────────────────────

/// Shape of [`SimFs::random`] trees.
#[derive(Debug, Clone, Copy)]
pub struct RandomShape {
    pub max_dirs: u32,
    pub max_files_per_dir: u32,
    pub max_depth: u32,
    /// Inject unreadable directories, failing stats, `DT_UNKNOWN`, symlinks
    /// and a mount point.
    pub faults: bool,
    /// Add hardlinks (root totals only are then comparable).
    pub hardlinks: bool,
}

impl Default for RandomShape {
    fn default() -> Self {
        RandomShape {
            max_dirs: 200,
            max_files_per_dir: 20,
            max_depth: 8,
            faults: true,
            hardlinks: false,
        }
    }
}

impl SimFs {
    /// A random tree, for property tests.
    pub fn random(seed: u64, shape: RandomShape) -> SimFs {
        let mut rng = Rng::new(seed);
        let mut b = TreeBuilder::new();
        let mut frontier = vec![(0 as NodeId, 0u32)];
        let mut dirs = 1u32;
        let mut files_made: Vec<NodeId> = Vec::new();
        let mut i = 0;
        while i < frontier.len() {
            let (d, depth) = frontier[i];
            i += 1;
            let nf = rng.below(u64::from(shape.max_files_per_dir) + 1);
            for k in 0..nf {
                let f = if shape.faults && rng.chance(0.03) {
                    b.symlink(d, format!("l{k}"), rng.range(5, 60))
                } else {
                    let size = rng.size(20_000, 50_000_000);
                    b.file(d, format!("f{k}"), size)
                };
                if shape.faults {
                    if rng.chance(0.03) {
                        b.node_mut(f).unknown_dtype = true;
                    }
                    if rng.chance(0.02) {
                        b.node_mut(f).fail_stat = Some(errno::EACCES);
                    }
                }
                if b.node(f).kind == Kind::File && b.node(f).fail_stat.is_none() {
                    files_made.push(f);
                }
            }
            if shape.hardlinks && !files_made.is_empty() && rng.chance(0.2) {
                let of = files_made[rng.below(files_made.len() as u64) as usize];
                b.hardlink(d, "hl", of);
            }
            if depth < shape.max_depth {
                let fan = if depth == 0 {
                    rng.range(2, 8)
                } else {
                    rng.below(5)
                };
                for k in 0..fan {
                    if dirs >= shape.max_dirs {
                        break;
                    }
                    dirs += 1;
                    let c = b.dir(d, format!("d{k}"));
                    if shape.faults {
                        if rng.chance(0.03) {
                            b.node_mut(c).fail_open = Some(errno::EACCES);
                        } else if rng.chance(0.03) {
                            b.node_mut(c).unknown_dtype = true;
                        } else if rng.chance(0.02) {
                            b.node_mut(c).fail_stat = Some(errno::EACCES);
                        }
                    }
                    frontier.push((c, depth + 1));
                }
            }
        }
        if shape.faults && frontier.len() > 3 {
            let (d, _) = frontier[1 + rng.below(frontier.len() as u64 - 1) as usize];
            if b.node(d).fail_open.is_none() {
                b.mount(d, 2);
            }
        }
        b.build(b"/sdcard")
    }

    /// The standard `/sdcard`-shaped tree (spec section 5): 300 packages
    /// under `Android/data`, a 5,000-file camera folder, one 60,000-file
    /// folder (`Movies/.thumbcache`), a 40-level chain under
    /// `Documents/deep`, 3,000 empty directories (`.leftovers`), and the
    /// usual media folders (WhatsApp, Telegram, Pictures, Music,
    /// Download, Documents). By disk usage `Android` is the largest top-level folder
    /// (game data) and `Android/data` its largest child.
    pub fn standard(seed: u64) -> SimFs {
        Self::sdcard(seed, true)
    }

    /// [`SimFs::standard`], optionally without the 60,000-file folder (so the
    /// total is not one directory's stat time).
    pub fn sdcard(seed: u64, huge_folder: bool) -> SimFs {
        let mut rng = Rng::new(seed);
        let mut b = TreeBuilder::new();
        let root = 0;
        let android = b.dir(root, "Android");
        let data = b.dir(android, "data");
        for p in 0..300 {
            let pkg = b.dir(data, format!("com.example.app{p:03}"));
            let cache = b.dir(pkg, "cache");
            for f in 0..rng.below(30) {
                let s = rng.size(30_000, 5_000_000);
                b.file(cache, format!("c{f}"), s);
            }
            if rng.chance(0.2) {
                let sub = b.dir(cache, "image_manager_disk_cache");
                for f in 0..rng.below(100) {
                    let s = rng.size(50_000, 2_000_000);
                    b.file(sub, format!("{f:x}.0"), s);
                }
            }
            let files = b.dir(pkg, "files");
            random_subtree(&mut b, &mut rng, files, 2, 16);
            if p % 15 == 0 {
                // Games keep their assets here.
                b.file(files, "assets.pak", rng.range(300_000_000, 1_500_000_000));
            }
            if rng.chance(0.5) {
                b.dir(pkg, "no_backup");
            }
        }
        let obb = b.dir(android, "obb");
        for g in 0..10 {
            let d = b.dir(obb, format!("com.example.game{g:02}"));
            let s = rng.range(200_000_000, 1_000_000_000);
            b.file(d, format!("main.{g}.obb"), s);
        }
        let media = b.dir(android, "media");
        for p in 0..10 {
            let d = b.dir(media, format!("com.example.media{p}"));
            for f in 0..rng.below(30) {
                b.file(d, format!("m{f}"), rng.size(500_000, 20_000_000));
            }
        }
        // Leftovers of uninstalled apps: thousands of empty directories.
        let hoard = b.dir(root, ".leftovers");
        for e in 0..3000 {
            b.dir(hoard, format!("e{e:04}"));
        }

        let wa = b.dir(root, "WhatsApp");
        let wam = b.dir(wa, "Media");
        for (name, n, typ) in [
            ("WhatsApp Images", 5_000, 200_000),
            ("WhatsApp Video", 600, 15_000_000),
            ("WhatsApp Documents", 1_500, 400_000),
            ("WhatsApp Audio", 3_000, 1_000_000),
            (".Statuses", 60, 1_000_000),
        ] {
            let d = b.dir(wam, name);
            for f in 0..n {
                let s = rng.size(typ, 500_000_000);
                b.file(d, format!("WA-{f:05}"), s);
            }
            let sent = b.dir(d, "Sent");
            for f in 0..n / 5 {
                let s = rng.size(typ, 500_000_000);
                b.file(sent, format!("WA-{f:05}"), s);
            }
        }
        let notes = b.dir(wam, "WhatsApp Voice Notes");
        for w in 0..200 {
            let d = b.dir(notes, format!("2024{w:03}"));
            for f in 0..rng.below(20) {
                b.file(d, format!("PTT-{f:03}.opus"), rng.size(30_000, 2_000_000));
            }
        }
        let backups = b.dir(wa, "Backups");
        b.file(backups, "chatsettingsbackup.db.crypt14", 2_000);
        let dbs = b.dir(wa, "Databases");
        for f in 0..10 {
            b.file(
                dbs,
                format!("msgstore-{f}.db.crypt14"),
                rng.range(50_000_000, 150_000_000),
            );
        }

        let dcim = b.dir(root, "DCIM");
        let cam = b.dir(dcim, "Camera");
        for f in 0..5000 {
            let s = rng.range(1_500_000, 5_000_000);
            b.file(cam, format!("IMG_{f:05}.jpg"), s);
        }
        let th = b.dir(dcim, ".thumbnails");
        for f in 0..800 {
            b.file(th, format!("{f}.jpg"), rng.range(5_000, 30_000));
        }

        let pics = b.dir(root, "Pictures");
        let shots = b.dir(pics, "Screenshots");
        for f in 0..2500 {
            b.file(
                shots,
                format!("Screenshot_{f:04}.png"),
                rng.range(100_000, 900_000),
            );
        }
        for (name, n) in [("Instagram", 2000), ("Facebook", 1500), ("Twitter", 800)] {
            let d = b.dir(pics, name);
            for f in 0..n {
                b.file(d, format!("{name}_{f:05}.jpg"), rng.range(50_000, 600_000));
            }
        }
        for a in 0..40 {
            let d = b.dir(pics, format!("Album{a:02}"));
            for f in 0..rng.below(200) {
                b.file(d, format!("p{f}.jpg"), rng.range(200_000, 4_000_000));
            }
        }

        let tg = b.dir(root, "Telegram");
        for (name, n, typ) in [
            ("Telegram Images", 2500, 150_000),
            ("Telegram Video", 600, 10_000_000),
            ("Telegram Documents", 1500, 2_000_000),
            ("Telegram Audio", 1200, 1_500_000),
        ] {
            let d = b.dir(tg, name);
            for f in 0..n {
                b.file(d, format!("{f:05}"), rng.size(typ, 300_000_000));
            }
        }

        let movies = b.dir(root, "Movies");
        for f in 0..10 {
            b.file(
                movies,
                format!("movie{f}.mp4"),
                rng.range(100_000_000, 1_500_000_000),
            );
        }
        if huge_folder {
            let thumbs = b.dir(movies, ".thumbcache");
            for f in 0..60_000 {
                b.file(thumbs, format!("t{f:05}"), rng.range(2_000, 20_000));
            }
        }

        let dl = b.dir(root, "Download");
        for f in 0..2000 {
            let s = rng.size(1_000_000, 300_000_000);
            b.file(dl, format!("download{f:04}.bin"), s);
        }
        for d in 0..40 {
            let x = b.dir(dl, format!("folder{d}"));
            for f in 0..40 {
                b.file(x, format!("f{f}"), rng.size(100_000, 10_000_000));
            }
        }

        let docs = b.dir(root, "Documents");
        for f in 0..500 {
            b.file(
                docs,
                format!("doc{f:03}.pdf"),
                rng.size(300_000, 30_000_000),
            );
        }
        for p in 0..30 {
            let d = b.dir(docs, format!("Project{p:02}"));
            for f in 0..rng.below(120) {
                b.file(d, format!("file{f:03}"), rng.size(100_000, 10_000_000));
            }
        }
        let mut deep = b.dir(docs, "deep");
        for l in 0..40 {
            b.file(deep, format!("level{l}.txt"), 100 + l);
            deep = b.dir(deep, format!("l{l}"));
        }

        let music = b.dir(root, "Music");
        for a in 0..60 {
            let artist = b.dir(music, format!("Artist{a:02}"));
            for al in 0..3 {
                let album = b.dir(artist, format!("Album{al}"));
                for t in 0..12 {
                    b.file(
                        album,
                        format!("{t:02}.mp3"),
                        rng.range(3_000_000, 9_000_000),
                    );
                }
            }
        }
        for name in [
            "Alarms",
            "Notifications",
            "Ringtones",
            "Podcasts",
            "Audiobooks",
            "Recordings",
        ] {
            b.dir(root, name);
        }
        b.build(b"/sdcard")
    }
}

fn name_hash(name: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn random_subtree(b: &mut TreeBuilder, rng: &mut Rng, d: NodeId, depth: u32, files: u64) {
    for f in 0..rng.below(files + 1) {
        let s = rng.size(40_000, 20_000_000);
        b.file(d, format!("f{f}"), s);
    }
    if depth == 0 {
        return;
    }
    for k in 0..rng.below(3) {
        let c = b.dir(d, format!("d{k}"));
        random_subtree(b, rng, c, depth - 1, files / 2);
    }
}

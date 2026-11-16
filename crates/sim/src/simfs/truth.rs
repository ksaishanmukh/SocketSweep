//! Ground-truth totals over the live tree.

use std::collections::HashSet;

use socketsweep_engine::MAX_DEPTH;
use socketsweep_wire::{Kind, Totals};

use super::{NodeId, SimFs, Truth};

impl SimFs {
    /// Ground truth over the live tree for a scan of it.
    /// Hardlinks are counted once each in a depth-first order, so per-directory
    /// totals of trees with hardlinks are only meaningful at the root.
    pub fn truth(&self) -> Truth {
        let n = self.nodes.len();
        let mut walk = Walk {
            fs: self,
            root_dev: self.nodes[0].dev,
            links: HashSet::new(),
            t: Truth {
                totals: vec![None; n],
                listed: vec![false; n],
                stated: vec![false; n],
                maybe: vec![false; n],
            },
        };
        walk.dir(0, 0);
        walk.t
    }
}

struct Walk<'a> {
    fs: &'a SimFs,
    root_dev: u64,
    links: HashSet<(u64, u64)>,
    t: Truth,
}

impl Walk<'_> {
    fn dir(&mut self, d: NodeId, depth: u8) -> Totals {
        let fs = self.fs;
        let node = &fs.nodes[d as usize];
        self.t.listed[d as usize] = true;
        let mut tot = Totals::default();
        if node.fail_open.is_some() {
            tot.errors = 1;
            tot.flags = Totals::UNREADABLE;
        } else if d != 0 && node.dev != self.root_dev {
            tot.flags = Totals::OTHER_FS;
            for c in fs.children(d) {
                self.t.maybe[c as usize] = true;
            }
        } else {
            for c in fs.children(d) {
                self.entry(c, depth, &mut tot);
            }
        }
        self.t.totals[d as usize] = Some(tot);
        tot
    }

    /// Entry `c` of a directory at `depth`, counted into `tot`.
    fn entry(&mut self, c: NodeId, depth: u8, tot: &mut Totals) {
        let cn = &self.fs.nodes[c as usize];
        self.t.stated[c as usize] = true;
        let is_dir = cn.kind == Kind::Dir && !(cn.unknown_dtype && cn.fail_stat.is_some());
        if is_dir {
            tot.dirs += 1;
        } else {
            tot.files += 1;
        }
        if cn.fail_stat.is_some() {
            tot.errors += 1;
        } else {
            // Hardlink duplicates count here too: only bytes are
            // deduplicated, not the inode's mtime.
            tot.newest_mtime = tot.newest_mtime.max(cn.mtime);
            if cn.kind == Kind::Dir || cn.nlink <= 1 || self.links.insert((cn.dev, cn.ino)) {
                tot.apparent += cn.apparent;
                tot.disk += cn.blocks * 512;
            }
        }
        if !is_dir {
            return;
        }
        if depth >= MAX_DEPTH {
            tot.flags |= Totals::DEPTH_LIMIT;
        } else {
            let sub = self.dir(c, depth + 1);
            tot.add(&sub);
        }
    }
}

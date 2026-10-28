//! Ground-truth totals over the live tree.

use std::collections::HashSet;

use socketsweep_wire::{Kind, Totals};

use super::{NodeId, SimFs, Truth};

impl SimFs {
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
                if cn.fail_stat.is_none() {
                    // Hardlink duplicates count here too: only bytes are
                    // deduplicated, not the inode's mtime.
                    tot.newest_mtime = tot.newest_mtime.max(cn.mtime);
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
}

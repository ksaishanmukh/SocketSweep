//! [`SimFs`] as the engine's [`Fs`].

use std::sync::atomic::Ordering;

use socketsweep_engine::{
    errno, DirRead, Fs, FsEntry, FsError, FsStat, Interrupt, RootInfo, StatResult,
};
use socketsweep_wire::{FsOp, Kind, Stat};

use super::{NodeId, SimFs};

impl SimFs {
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

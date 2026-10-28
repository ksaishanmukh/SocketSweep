//! `Listing` and `Stats` events: entries arriving, and their sizes.

use socketsweep_gui_contract::{Measure, NodeId, Size};
use socketsweep_wire::{DirId, Entry, Kind, Stat, Totals};

use crate::tree::nodes::size_of;
use crate::tree::{protocol, DirState, Node, Tree, TreeError, NONE};

impl Tree {
    pub(super) fn listing(
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

        let (delta, added, opened) = self.add_entries(dir, parent, entries)?;

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

    /// Add a listing part's entries under `parent`, introducing the walked
    /// directories among them. Returns what they add to `parent`'s sums, the
    /// new nodes, and how many directories they open.
    fn add_entries(
        &mut self,
        dir: DirId,
        parent: NodeId,
        entries: &[Entry],
    ) -> Result<(Totals, Vec<NodeId>, u32), TreeError> {
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
            let mtime = entry.stat.map_or(0, |s| s.mtime);
            let id = self.push_node(parent, entry, (own, mtime), measure);
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
            delta.newest_mtime = delta.newest_mtime.max(mtime);
            added.push(id);
        }
        Ok((delta, added, opened))
    }

    fn push_node(
        &mut self,
        parent: NodeId,
        entry: &Entry,
        (own, mtime): (Size, i64),
        measure: Measure,
    ) -> NodeId {
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
            mtime,
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

    pub(super) fn stats(
        &mut self,
        dir: DirId,
        start: u32,
        stats: &[Option<Stat>],
    ) -> Result<(), TreeError> {
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
                    n.mtime = stat.mtime;
                    delta.apparent += n.own.apparent;
                    delta.disk += n.own.disk;
                    delta.newest_mtime = delta.newest_mtime.max(stat.mtime);
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
}

//! `DirFailed`, `SubtreeDone` and `Pruned` events: a directory ending.

use socketsweep_gui_contract::Measure;
use socketsweep_wire::{DirId, Totals, ROOT_DIR};

use crate::tree::{protocol, DirState, Tree, TreeError};

impl Tree {
    pub(super) fn dir_failed(&mut self, dir: DirId) -> Result<(), TreeError> {
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

    pub(super) fn subtree_done(&mut self, dir: DirId, totals: &Totals) -> Result<(), TreeError> {
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

    pub(super) fn pruned(&mut self, dir: DirId) -> Result<(), TreeError> {
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
}

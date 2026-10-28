//! Applying the scan event stream to a [`Tree`], checking the daemon's
//! ordering rules as it goes.

mod completion;
mod listing;

use socketsweep_gui_contract::NodeId;
use socketsweep_wire::{DirId, ScanEvent};

use super::{protocol, DirRec, DirState, Ingested, Tree, TreeError};

impl Tree {
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

    /// The record for `dir`, which must have been introduced.
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
}

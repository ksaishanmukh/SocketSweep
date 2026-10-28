//! Directory paths as a host would track them.

use std::collections::HashMap;

use socketsweep_wire::{DirId, ScanEvent};

/// Paths of introduced directories, as a host would track them.
#[derive(Debug, Default)]
pub(super) struct Ids {
    pub(super) path: Vec<Vec<u8>>,
    pub(super) by_path: HashMap<Vec<u8>, DirId>,
}

impl Ids {
    pub(super) fn see(&mut self, ev: &ScanEvent) {
        match ev {
            ScanEvent::Started { root, .. } => {
                self.path = vec![root.clone()];
                self.by_path.insert(root.clone(), 0);
            }
            ScanEvent::Listing { dir, entries, .. } => {
                let base = self.path[*dir as usize].clone();
                for e in entries {
                    if let Some(id) = e.id {
                        let mut p = base.clone();
                        if p.last() != Some(&b'/') {
                            p.push(b'/');
                        }
                        p.extend_from_slice(&e.name);
                        if self.path.len() <= id as usize {
                            self.path.resize(id as usize + 1, Vec::new());
                        }
                        self.path[id as usize] = p.clone();
                        self.by_path.insert(p, id);
                    }
                }
            }
            _ => {}
        }
    }
}

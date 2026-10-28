//! The real [`Actions`], over `std::fs`.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use socketsweep_wire::{CopyItem, Totals};

use super::{
    bytes_to_path, delete_tree, guard, manifest_tree, path_to_bytes, ActionError, Actions,
};

/// [`Actions`] over `std::fs`, confined to `root` (the daemon's `--root`).
#[derive(Debug, Clone)]
pub struct LocalActions {
    pub root: PathBuf,
}

impl LocalActions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalActions { root: root.into() }
    }
}

impl Actions for LocalActions {
    fn resolve_scan_root(&self, path: &[u8]) -> Result<Vec<u8>, ActionError> {
        let p = guard::resolve_at_or_under_root(&self.root, &bytes_to_path(path))?;
        Ok(path_to_bytes(&p))
    }

    fn resolve_delete(&self, path: &[u8]) -> Result<Vec<u8>, ActionError> {
        let p = guard::resolve_under_root(&self.root, &bytes_to_path(path))?;
        Ok(path_to_bytes(&p))
    }

    fn resolve_copy(&self, path: &[u8]) -> Result<Vec<u8>, ActionError> {
        let p = guard::resolve_for_copy(&self.root, &bytes_to_path(path))?;
        Ok(path_to_bytes(&p))
    }

    fn same_entry(&self, scan_path: &[u8], resolved: &[u8]) -> bool {
        if cfg!(unix) {
            scan_path == resolved
        } else {
            // Component-wise, so `/` and `\` compare equal.
            bytes_to_path(scan_path) == bytes_to_path(resolved)
        }
    }

    fn delete(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(u64, Totals), ActionError> {
        delete_tree(&bytes_to_path(resolved), cancel, progress)
    }

    fn manifest(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        emit: &mut dyn FnMut(CopyItem),
    ) -> Result<(), ActionError> {
        manifest_tree(&bytes_to_path(resolved), cancel, emit)
    }
}

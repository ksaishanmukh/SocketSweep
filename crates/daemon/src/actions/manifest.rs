//! Listing a tree for copying.

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use socketsweep_wire::{CopyItem, Kind};

use super::metadata::{kind_of, mode_of, mtime_of};
use super::paths::name_bytes;
use super::ActionError;

// ── Copy manifest ───────────────────────────────────────────────────────────

fn copy_item(rel: Vec<u8>, m: &Metadata) -> CopyItem {
    let kind = kind_of(m);
    CopyItem {
        rel,
        kind,
        size: if kind == Kind::File { m.len() } else { 0 },
        mode: mode_of(m),
        mtime: mtime_of(m),
    }
}

/// List `root` and everything below it for copying: the root first (empty
/// `rel`), each directory before its contents, links listed and never
/// followed. Stops at the first error or once `cancel` is set.
pub fn manifest_tree(
    root: &Path,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(CopyItem),
) -> Result<(), ActionError> {
    let meta = fs::symlink_metadata(root).map_err(|e| ActionError::io(&e, "stat", root))?;
    let is_dir = kind_of(&meta) == Kind::Dir;
    emit(copy_item(Vec::new(), &meta));
    if !is_dir {
        return Ok(());
    }
    let mut stack: Vec<(Vec<u8>, PathBuf)> = vec![(Vec::new(), root.to_path_buf())];
    while let Some((rel, dir)) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| ActionError::io(&e, "open", &dir))?;
        for (i, entry) in rd.enumerate() {
            if i % 256 == 0 && cancel.load(Ordering::Relaxed) {
                return Err(ActionError::cancelled());
            }
            let entry = entry.map_err(|e| ActionError::io(&e, "read", &dir))?;
            let path = entry.path();
            let m = entry
                .metadata()
                .map_err(|e| ActionError::io(&e, "stat", &path))?;
            let mut child_rel = rel.clone();
            if !child_rel.is_empty() {
                child_rel.push(b'/');
            }
            child_rel.extend_from_slice(&name_bytes(&entry.file_name()));
            if kind_of(&m) == Kind::Dir {
                stack.push((child_rel.clone(), path));
            }
            emit(copy_item(child_rel, &m));
        }
    }
    Ok(())
}

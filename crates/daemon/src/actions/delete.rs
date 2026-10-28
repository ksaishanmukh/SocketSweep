//! Deleting a tree without following symbolic links.

use std::fs::{self, Metadata};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(doc)]
use socketsweep_wire::ErrCode;
use socketsweep_wire::{Kind, Totals};

#[cfg(doc)]
use super::guard;
use super::metadata::{account, kind_of};
use super::ActionError;

// ── Delete ──────────────────────────────────────────────────────────────────

struct DeleteCtx<'a> {
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(u64),
    items: u64,
    totals: Totals,
}

/// Remove `path` and everything below it, never following a symbolic link:
/// every entry is judged by `lstat`, a link is unlinked, and only real
/// directories are descended into. `path` must come from
/// [`guard::resolve_under_root`].
///
/// Stops at the first error, or with [`ErrCode::Cancelled`] once `cancel` is
/// set; what was removed by then stays removed. Returns the items removed
/// and their totals (the entry itself included).
pub fn delete_tree(
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<(u64, Totals), ActionError> {
    let meta = fs::symlink_metadata(path).map_err(|e| ActionError::io(&e, "stat", path))?;
    let mut ctx = DeleteCtx {
        cancel,
        progress,
        items: 0,
        totals: Totals::default(),
    };
    remove_entry(path, &meta, &mut ctx)?;
    Ok((ctx.items, ctx.totals))
}

fn remove_entry(path: &Path, meta: &Metadata, ctx: &mut DeleteCtx<'_>) -> Result<(), ActionError> {
    if ctx.cancel.load(Ordering::Relaxed) {
        return Err(ActionError::cancelled());
    }
    if kind_of(meta) == Kind::Dir {
        let rd = fs::read_dir(path).map_err(|e| ActionError::io(&e, "open", path))?;
        for entry in rd {
            let entry = entry.map_err(|e| ActionError::io(&e, "read", path))?;
            let child = entry.path();
            // `DirEntry::metadata` does not follow links.
            let m = entry
                .metadata()
                .map_err(|e| ActionError::io(&e, "stat", &child))?;
            remove_entry(&child, &m, ctx)?;
        }
        fs::remove_dir(path).map_err(|e| ActionError::io(&e, "remove", path))?;
    } else {
        remove_non_dir(path, meta).map_err(|e| ActionError::io(&e, "remove", path))?;
    }
    account(&mut ctx.totals, meta);
    ctx.items += 1;
    (ctx.progress)(ctx.items);
    Ok(())
}

fn remove_non_dir(path: &Path, _meta: &Metadata) -> io::Result<()> {
    match fs::remove_file(path) {
        // A Windows directory symlink or junction is removed as a directory;
        // `remove_dir` on a link never touches what it points to.
        #[cfg(windows)]
        Err(e) if _meta.file_type().is_symlink() => fs::remove_dir(path).map_err(|_| e),
        other => other,
    }
}

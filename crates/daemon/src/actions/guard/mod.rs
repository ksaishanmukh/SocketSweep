//! Path validation for destructive operations.
//!
//! The only place a delete is authorised. What makes a delete safe is the
//! pairing of how a path is resolved here with how it is then removed
//! ([`delete_tree`](super::delete_tree), which never follows a symbolic
//! link), so the two live side by side in `actions`.
//!
//! It lives here, next to the filesystem, rather than on the desktop: a check on
//! the desktop is one that anything speaking to the socket directly never
//! reaches.
//!
//! Free of socket and Android specifics so it can be tested on any development
//! machine.

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum GuardError {
    /// The path does not exist, or a component of it is unreadable.
    NotFound(PathBuf),
    /// Resolved to somewhere outside the session root.
    OutsideRoot { target: PathBuf, root: PathBuf },
    /// Refusing to delete the directory the session is rooted at.
    IsRoot(PathBuf),
    /// The last component is empty (a trailing `/`), `.` or `..`. Each names a
    /// directory by reference rather than an entry in its parent, so there is
    /// no single entry to remove.
    NoFinalName(PathBuf),
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardError::NotFound(p) => write!(f, "no such path: {}", p.display()),
            GuardError::OutsideRoot { target, root } => {
                write!(
                    f,
                    "refusing to touch {}: outside the scan root {}",
                    target.display(),
                    root.display()
                )
            }
            GuardError::IsRoot(p) => {
                write!(
                    f,
                    "refusing to delete the scan root itself: {}",
                    p.display()
                )
            }
            GuardError::NoFinalName(p) => write!(
                f,
                "refusing to delete {}: it must end in a file or directory name",
                p.display()
            ),
        }
    }
}

/// [`Path::canonicalize`], minus the `\\?\` prefix Windows adds, so a
/// resolved path can still be joined with `/` the way the engine builds
/// paths. Exactly `canonicalize` anywhere else.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let p = path.canonicalize()?;
    #[cfg(windows)]
    {
        let s = p.as_os_str().to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            let b = rest.as_bytes();
            if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
                return Ok(PathBuf::from(rest));
            }
        }
    }
    Ok(p)
}

fn canonical_root(root: &Path) -> Result<PathBuf, GuardError> {
    canonicalize(root).map_err(|_| GuardError::NotFound(root.to_path_buf()))
}

/// Resolve a delete target and confirm it sits strictly beneath `root`.
///
/// Only the parent directory is canonicalised; the final component is kept as
/// given and never followed. The result therefore names the entry the request
/// pointed at, not whatever it links to:
///
/// - `..` segments and symlinks along the way are resolved, so a path that
///   climbs out of the root, or passes through a link that leads out of it, is
///   judged by where it really is and rejected.
/// - A symlink as the last component stays a symlink. Deleting it removes the
///   link alone. Following it instead would turn a delete of `/sdcard/foo`,
///   where `foo -> /sdcard/DCIM`, into a recursive delete of DCIM. A link inside
///   the root that points outside it is accepted for the same reason: removing
///   the link cannot touch its target.
///
/// Comparison uses [`Path::starts_with`], which matches whole path components —
/// a plain string prefix test would let `/sdcard/Downloads` pass a
/// `/sdcard/Down` root.
///
/// Pair with [`remove`], which deletes the returned path without following it.
///
/// # Known limitation
///
/// A component could be swapped for a symlink between this check and the delete.
/// Closing that window needs an `openat`/`O_NOFOLLOW` descent; on a single-user
/// device, where an attacker would already need shell-domain access, it is not
/// the weak link.
pub fn resolve_under_root(root: &Path, target: &Path) -> Result<PathBuf, GuardError> {
    let root = canonical_root(root)?;

    let name = final_name(target).ok_or_else(|| GuardError::NoFinalName(target.to_path_buf()))?;
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent = canonicalize(parent).map_err(|_| GuardError::NotFound(target.to_path_buf()))?;
    let resolved = parent.join(name);

    // The entry itself must exist, but is looked at rather than through.
    if std::fs::symlink_metadata(&resolved).is_err() {
        return Err(GuardError::NotFound(target.to_path_buf()));
    }

    if resolved == root {
        return Err(GuardError::IsRoot(resolved));
    }
    if !resolved.starts_with(&root) {
        return Err(GuardError::OutsideRoot {
            target: resolved,
            root,
        });
    }
    Ok(resolved)
}

/// Resolve a scan root and confirm it is `root` or beneath it.
///
/// Unlike [`resolve_under_root`] this canonicalises the whole path, final
/// component included: a scan only reads, and `/sdcard` itself is a symlink
/// chain to `/storage/emulated/0` on a typical device.
pub fn resolve_at_or_under_root(root: &Path, target: &Path) -> Result<PathBuf, GuardError> {
    let root = canonical_root(root)?;
    let resolved = canonicalize(target).map_err(|_| GuardError::NotFound(target.to_path_buf()))?;

    if !resolved.starts_with(&root) {
        return Err(GuardError::OutsideRoot {
            target: resolved,
            root,
        });
    }
    Ok(resolved)
}

/// Resolve a copy source: as [`resolve_under_root`], so a symbolic link as the
/// final component is listed as a link rather than followed, except that the
/// root itself may be copied (copying only reads).
pub fn resolve_for_copy(root: &Path, target: &Path) -> Result<PathBuf, GuardError> {
    match resolve_under_root(root, target) {
        Err(GuardError::IsRoot(p)) => Ok(p),
        // `/sdcard/` or `/sdcard/DCIM/..`: a directory named by reference.
        // Reading through it is harmless; judge it by where it leads.
        Err(GuardError::NoFinalName(_)) => resolve_at_or_under_root(root, target),
        other => other,
    }
}

/// The last component of `path` exactly as written, or `None` if it is empty,
/// `.` or `..`.
///
/// Read from the raw bytes because [`Path::components`] normalises a trailing
/// `/` and `.` away, and `link/` or `link/.` would then quietly become `link`.
fn final_name(path: &Path) -> Option<&std::ffi::OsStr> {
    let raw = path.as_os_str().as_encoded_bytes();
    let last = raw
        .iter()
        .rposition(|&b| std::path::is_separator(b as char))
        .map_or(raw, |i| &raw[i + 1..]);
    if last.is_empty() || last == b"." || last == b".." {
        return None;
    }
    // A non-empty, non-dot last segment is exactly what `file_name` returns.
    path.file_name()
}

#[cfg(test)]
mod tests;

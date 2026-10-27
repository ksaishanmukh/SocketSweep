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
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;

    /// Delete as the session does, returning the items removed.
    fn remove(path: &Path) -> std::io::Result<u64> {
        let cancel = AtomicBool::new(false);
        super::super::delete_tree(path, &cancel, &mut |_| {})
            .map(|(items, _)| items)
            .map_err(|e| std::io::Error::other(e.message))
    }

    #[test]
    fn a_copy_may_name_the_root_but_not_outside_it() {
        let f = fixture();
        let canonical = canonicalize(&f.root).unwrap();
        assert_eq!(resolve_for_copy(&f.root, &f.root), Ok(canonical.clone()));
        let with_slash = format!("{}/", f.root.to_string_lossy());
        assert_eq!(
            resolve_for_copy(&f.root, Path::new(&with_slash)),
            Ok(canonical)
        );
        assert!(resolve_for_copy(&f.root, &f.root.join("DCIM")).is_ok());
        assert!(matches!(
            resolve_for_copy(&f.root, &f.outside),
            Err(GuardError::OutsideRoot { .. })
        ));
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        let root = base.join("sdcard");
        fs::create_dir_all(root.join("DCIM/Camera")).unwrap();
        fs::create_dir_all(root.join("Download")).unwrap();
        fs::write(root.join("DCIM/Camera/IMG_0001.jpg"), b"x").unwrap();

        // A sibling whose name shares a prefix with a directory inside the root.
        fs::create_dir_all(root.join("Down")).unwrap();

        let outside = base.join("private");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secrets.txt"), b"x").unwrap();

        Fixture {
            _tmp: tmp,
            root,
            outside,
        }
    }

    #[test]
    fn allows_a_file_inside_the_root() {
        let f = fixture();
        let target = f.root.join("DCIM/Camera/IMG_0001.jpg");
        assert!(resolve_under_root(&f.root, &target).is_ok());
    }

    #[test]
    fn allows_a_directory_inside_the_root() {
        let f = fixture();
        assert!(resolve_under_root(&f.root, &f.root.join("DCIM")).is_ok());
    }

    #[test]
    fn rejects_the_root_itself() {
        let f = fixture();
        let root = f.root.clone();
        assert!(matches!(
            resolve_under_root(&root, &root),
            Err(GuardError::IsRoot(_))
        ));
    }

    #[test]
    fn rejects_the_root_reached_through_dotdot() {
        let f = fixture();
        assert!(matches!(
            resolve_under_root(&f.root, &f.root.join("DCIM/../../sdcard")),
            Err(GuardError::IsRoot(_))
        ));
    }

    #[test]
    fn rejects_a_sibling_of_the_root() {
        let f = fixture();
        let target = f.outside.join("secrets.txt");
        assert!(matches!(
            resolve_under_root(&f.root, &target),
            Err(GuardError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn rejects_dotdot_traversal_out_of_the_root() {
        let f = fixture();
        let target = f.root.join("DCIM/../../private/secrets.txt");
        assert!(matches!(
            resolve_under_root(&f.root, &target),
            Err(GuardError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn dotdot_that_stays_inside_is_fine() {
        let f = fixture();
        let target = f.root.join("DCIM/../Download");
        assert!(resolve_under_root(&f.root, &target).is_ok());
    }

    /// `DCIM/Camera/..` is DCIM, but a delete must name an entry, not a
    /// directory reached by reference.
    #[test]
    fn rejects_a_final_dotdot_dot_or_empty_component() {
        let f = fixture();
        let root = f.root.to_string_lossy().into_owned();
        for target in [
            format!("{root}/DCIM/Camera/.."),
            format!("{root}/DCIM/."),
            format!("{root}/DCIM/"),
        ] {
            assert!(
                matches!(
                    resolve_under_root(&f.root, Path::new(&target)),
                    Err(GuardError::NoFinalName(_))
                ),
                "{target} must be rejected"
            );
        }
    }

    /// Component-wise comparison matters here: "Downloads" starts with "Down".
    #[test]
    fn a_root_that_is_a_string_prefix_of_the_target_does_not_authorise_it() {
        let f = fixture();
        let narrow_root = f.root.join("Down");
        let target = f.root.join("Download");
        assert!(
            matches!(
                resolve_under_root(&narrow_root, &target),
                Err(GuardError::OutsideRoot { .. })
            ),
            "component-wise comparison must not treat Down as a parent of Download"
        );
    }

    #[test]
    fn rejects_a_path_that_does_not_exist() {
        let f = fixture();
        assert!(matches!(
            resolve_under_root(&f.root, &f.root.join("nope")),
            Err(GuardError::NotFound(_))
        ));
    }

    #[test]
    fn scan_variant_permits_the_root_but_still_rejects_outside() {
        let f = fixture();
        let canonical = canonicalize(&f.root).unwrap();
        assert_eq!(resolve_at_or_under_root(&f.root, &f.root), Ok(canonical));
        assert!(resolve_at_or_under_root(&f.root, &f.root.join("DCIM")).is_ok());
        assert!(matches!(
            resolve_at_or_under_root(&f.root, &f.outside),
            Err(GuardError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn removing_a_directory_counts_everything_in_it() {
        let f = fixture();
        let target = resolve_under_root(&f.root, &f.root.join("DCIM")).unwrap();
        assert_eq!(remove(&target).unwrap(), 3, "DCIM, Camera, IMG_0001.jpg");
        assert!(!f.root.join("DCIM").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_the_root_is_unlinked_not_followed() {
        let f = fixture();
        let link = f.root.join("shortcut");
        std::os::unix::fs::symlink(f.root.join("DCIM"), &link).unwrap();

        let resolved = resolve_under_root(&f.root, &link).unwrap();
        assert_eq!(resolved.file_name().unwrap(), "shortcut");
        assert_eq!(remove(&resolved).unwrap(), 1);

        assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
        assert!(
            f.root.join("DCIM/Camera/IMG_0001.jpg").exists(),
            "its target is untouched"
        );
    }

    /// Unlinking a link is harmless wherever it points, so a link sitting in the
    /// root is deletable even when its target is not.
    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_root_is_deletable_and_its_target_survives() {
        let f = fixture();
        let link = f.root.join("escape");
        std::os::unix::fs::symlink(&f.outside, &link).unwrap();

        let resolved = resolve_under_root(&f.root, &link).unwrap();
        assert_eq!(remove(&resolved).unwrap(), 1);

        assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
        assert!(
            f.outside.join("secrets.txt").exists(),
            "its target is untouched"
        );
    }

    /// A link in the path, rather than at its end, is still followed, so it
    /// cannot carry a delete out of the root.
    #[cfg(unix)]
    #[test]
    fn a_link_in_the_middle_of_the_path_is_judged_by_where_it_leads() {
        let f = fixture();
        let link = f.root.join("escape");
        std::os::unix::fs::symlink(&f.outside, &link).unwrap();
        assert!(matches!(
            resolve_under_root(&f.root, &link.join("secrets.txt")),
            Err(GuardError::OutsideRoot { .. })
        ));
    }

    /// A link that resolves to the root is just an entry inside it.
    #[cfg(unix)]
    #[test]
    fn a_link_to_the_root_removes_only_the_link() {
        let f = fixture();
        let link = f.root.join("self");
        std::os::unix::fs::symlink(&f.root, &link).unwrap();

        let resolved = resolve_under_root(&f.root, &link).unwrap();
        assert_eq!(remove(&resolved).unwrap(), 1);
        assert!(f.root.join("DCIM").exists());
    }

    /// The root travels as a path, not as display text, so a name that is not
    /// UTF-8 survives the at-root case of a scan.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_root_resolves_to_itself() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(OsStr::from_bytes(b"card\xFF"));
        fs::create_dir_all(&root).unwrap();

        let resolved = resolve_at_or_under_root(&root, &root).unwrap();
        assert_eq!(resolved, canonicalize(&root).unwrap());
        assert!(resolved.as_os_str().as_bytes().ends_with(b"card\xFF"));
    }
}

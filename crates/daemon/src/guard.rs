//! Path validation for destructive operations.
//!
//! The only place a delete is authorised.
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
    NotFound(String),
    /// Resolved to somewhere outside the session root.
    OutsideRoot { target: String, root: String },
    /// Refusing to delete the directory the session is rooted at.
    IsRoot(String),
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardError::NotFound(p) => write!(f, "no such path: {p}"),
            GuardError::OutsideRoot { target, root } => {
                write!(
                    f,
                    "refusing to touch {target}: outside the scan root {root}"
                )
            }
            GuardError::IsRoot(p) => write!(f, "refusing to delete the scan root itself: {p}"),
        }
    }
}

/// Resolve `target` and confirm it sits strictly beneath `root`.
///
/// Canonicalisation does the heavy lifting: it resolves `..` segments and
/// follows symlinks, so a link inside the root pointing outside it resolves to
/// its real location and is then rejected. Comparison uses [`Path::starts_with`],
/// which matches whole path components — a plain string prefix test would let
/// `/sdcard/Downloads` pass a `/sdcard/Down` root.
///
/// # Known limitation
///
/// A component could be swapped for a symlink between this check and the delete.
/// Closing that window needs an `openat`/`O_NOFOLLOW` descent; on a single-user
/// device, where an attacker would already need shell-domain access, it is not
/// the weak link.
pub fn resolve_under_root(root: &Path, target: &Path) -> Result<PathBuf, GuardError> {
    let root = root
        .canonicalize()
        .map_err(|_| GuardError::NotFound(root.display().to_string()))?;
    let resolved = target
        .canonicalize()
        .map_err(|_| GuardError::NotFound(target.display().to_string()))?;

    if resolved == root {
        return Err(GuardError::IsRoot(resolved.display().to_string()));
    }
    if !resolved.starts_with(&root) {
        return Err(GuardError::OutsideRoot {
            target: resolved.display().to_string(),
            root: root.display().to_string(),
        });
    }
    Ok(resolved)
}

/// Like [`resolve_under_root`] but permits the root itself, for read-only
/// operations such as starting a scan.
pub fn resolve_at_or_under_root(root: &Path, target: &Path) -> Result<PathBuf, GuardError> {
    match resolve_under_root(root, target) {
        Err(GuardError::IsRoot(p)) => Ok(PathBuf::from(p)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        assert!(resolve_at_or_under_root(&f.root, &f.root).is_ok());
        assert!(matches!(
            resolve_at_or_under_root(&f.root, &f.outside),
            Err(GuardError::OutsideRoot { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_inside_the_root_that_points_outside() {
        let f = fixture();
        let link = f.root.join("escape");
        std::os::unix::fs::symlink(&f.outside, &link).unwrap();
        assert!(
            matches!(
                resolve_under_root(&f.root, &link),
                Err(GuardError::OutsideRoot { .. })
            ),
            "a symlink must be judged by where it resolves to, not where it sits"
        );
    }
}

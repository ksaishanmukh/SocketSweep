//! `actions`: everything that touches files by path rather than by scan —
//! resolving request paths through the guard, delete, and the copy manifest.
//!
//! [`Actions`] is the contract the session uses. [`LocalActions`] is the real
//! one, over `std::fs` and confined to the daemon's `--root`; it is portable,
//! so the session's tests run it on any host. `socketsweep-sim` implements it
//! over `SimFs` for `FakeDaemon`.
//!
//! # Safety rules (all enforced in [`guard`] and [`delete_tree`])
//!
//! - A delete or copy names an entry: only its parent is canonicalised, and the
//!   final component is never followed. Deleting a symbolic link removes the
//!   link; nothing below a link is ever touched.
//! - Everything resolves to somewhere strictly beneath the root (a scan or a
//!   copy may also name the root itself); the root is never deleted.
//! - Within a deleted directory, entries are judged by `lstat`, so a link to a
//!   directory is unlinked, not descended into.

pub mod guard;

use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use socketsweep_engine::errno;
use socketsweep_wire::{CopyItem, ErrCode, Kind, ServerMsg, Totals, BLOCK_SIZE};

use guard::GuardError;

/// Why an action failed: the payload of `ServerMsg::Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionError {
    pub code: ErrCode,
    pub errno: Option<i32>,
    pub message: String,
}

impl ActionError {
    pub fn new(code: ErrCode, message: impl Into<String>) -> Self {
        ActionError {
            code,
            errno: None,
            message: message.into(),
        }
    }

    pub fn cancelled() -> Self {
        ActionError::new(ErrCode::Cancelled, "cancelled")
    }

    /// From a failed file system call on `path`.
    pub fn io(e: &io::Error, what: &str, path: &Path) -> Self {
        let errno = errno_of(e);
        let code = match errno {
            errno::ENOENT => ErrCode::NotFound,
            errno::EACCES | 1 /* EPERM */ => ErrCode::PermissionDenied,
            _ => ErrCode::Io,
        };
        ActionError {
            code,
            errno: Some(errno),
            message: format!("cannot {what} {}: {e}", path.display()),
        }
    }

    /// From an engine `errno` (a scan root that could not be opened).
    pub fn errno(errno: i32, message: impl Into<String>) -> Self {
        let code = match errno {
            errno::ENOENT => ErrCode::NotFound,
            errno::EACCES | 1 => ErrCode::PermissionDenied,
            _ => ErrCode::Io,
        };
        ActionError {
            code,
            errno: Some(errno),
            message: message.into(),
        }
    }

    pub fn into_msg(self) -> ServerMsg {
        ServerMsg::Failed {
            code: self.code,
            errno: self.errno,
            message: self.message,
        }
    }
}

impl From<GuardError> for ActionError {
    fn from(e: GuardError) -> Self {
        let code = match &e {
            GuardError::NotFound(_) => ErrCode::NotFound,
            GuardError::OutsideRoot { .. } => ErrCode::OutsideRoot,
            GuardError::IsRoot(_) => ErrCode::ProtectedRoot,
            GuardError::NoFinalName(_) => ErrCode::BadRequest,
        };
        let errno = matches!(e, GuardError::NotFound(_)).then_some(errno::ENOENT);
        ActionError {
            code,
            errno,
            message: e.to_string(),
        }
    }
}

/// The `errno` behind an I/O error: the raw code on Unix, a close equivalent
/// elsewhere.
pub fn errno_of(e: &io::Error) -> i32 {
    #[cfg(unix)]
    if let Some(n) = e.raw_os_error() {
        return n;
    }
    match e.kind() {
        io::ErrorKind::NotFound => errno::ENOENT,
        io::ErrorKind::PermissionDenied => errno::EACCES,
        _ => errno::EIO,
    }
}

/// What the session needs to act on paths. Paths are bytes; the `resolve_*`
/// results are what the other methods take, and what is reported back.
pub trait Actions: Send + Sync {
    /// `StartScan`: the root as the engine should walk it. Follows every
    /// link (`/sdcard` is a chain of them); must be the allowed root or
    /// beneath it.
    fn resolve_scan_root(&self, path: &[u8]) -> Result<Vec<u8>, ActionError>;

    /// `Delete`: an entry strictly beneath the root, final component not
    /// followed.
    fn resolve_delete(&self, path: &[u8]) -> Result<Vec<u8>, ActionError>;

    /// `PrepareCopy`: as [`Actions::resolve_delete`], but the root itself is
    /// allowed.
    fn resolve_copy(&self, path: &[u8]) -> Result<Vec<u8>, ActionError>;

    /// Whether `scan_path` (built by a scan from its root and listed names)
    /// and `resolved` (from a `resolve_*`) name the same entry. Byte
    /// equality, except where the platform spells separators two ways.
    fn same_entry(&self, scan_path: &[u8], resolved: &[u8]) -> bool {
        scan_path == resolved
    }

    /// Remove `resolved` and everything below it, never following a link.
    /// Calls `progress` with the running item count, and stops with
    /// [`ErrCode::Cancelled`] once `cancel` is set. Returns the items removed
    /// and their totals, the removed entry itself included.
    fn delete(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(u64, Totals), ActionError>;

    /// List `resolved` and everything below it, each directory before its
    /// contents, the root first with an empty `rel`. Links are listed, never
    /// followed.
    fn manifest(
        &self,
        resolved: &[u8],
        cancel: &AtomicBool,
        emit: &mut dyn FnMut(CopyItem),
    ) -> Result<(), ActionError>;
}

// ── Paths as bytes ──────────────────────────────────────────────────────────

#[cfg(unix)]
pub fn bytes_to_path(p: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(p))
}

#[cfg(not(unix))]
pub fn bytes_to_path(p: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(p).into_owned())
}

#[cfg(unix)]
pub fn path_to_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
pub fn path_to_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn name_bytes(n: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    n.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn name_bytes(n: &std::ffi::OsStr) -> Vec<u8> {
    n.to_string_lossy().into_owned().into_bytes()
}

// ── Metadata ────────────────────────────────────────────────────────────────

fn kind_of(m: &Metadata) -> Kind {
    let t = m.file_type();
    if t.is_symlink() {
        Kind::Symlink
    } else if t.is_dir() {
        Kind::Dir
    } else if t.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

#[cfg(unix)]
fn blocks_of(m: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks()
}

#[cfg(not(unix))]
fn blocks_of(m: &Metadata) -> u64 {
    m.len().div_ceil(BLOCK_SIZE)
}

#[cfg(unix)]
fn mode_of(m: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(m: &Metadata) -> u32 {
    let base = if m.is_dir() { 0o755 } else { 0o644 };
    if m.permissions().readonly() {
        base & 0o555
    } else {
        base
    }
}

#[cfg(unix)]
fn mtime_of(m: &Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    m.mtime()
}

#[cfg(not(unix))]
fn mtime_of(m: &Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Add one removed (or listed) entry to `t`, as the engine counts it.
fn account(t: &mut Totals, m: &Metadata) {
    if kind_of(m) == Kind::Dir {
        t.dirs = t.dirs.saturating_add(1);
    } else {
        t.files = t.files.saturating_add(1);
    }
    t.apparent = t.apparent.saturating_add(m.len());
    t.disk = t
        .disk
        .saturating_add(blocks_of(m).saturating_mul(BLOCK_SIZE));
}

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

// ── The real implementation ─────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        fs::create_dir_all(r.join("a/b/c")).unwrap();
        fs::write(r.join("a/one.bin"), vec![0u8; 1000]).unwrap();
        fs::write(r.join("a/b/two.bin"), vec![0u8; 5000]).unwrap();
        fs::write(r.join("a/b/c/three.bin"), b"xyz").unwrap();
        fs::write(r.join("top.txt"), b"hello").unwrap();
        tmp
    }

    #[test]
    fn delete_reports_items_and_totals_of_what_it_removed() {
        let tmp = tree();
        let target = tmp.path().join("a");
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let (items, totals) = delete_tree(&target, &cancel, &mut |n| seen.push(n)).unwrap();
        assert_eq!(items, 6, "a, b, c and three files");
        assert_eq!(seen, (1..=6).collect::<Vec<_>>());
        assert_eq!(totals.files, 3);
        assert_eq!(totals.dirs, 3);
        assert!(totals.apparent >= 6003);
        assert!(totals.disk >= 6003);
        assert!(!target.exists());
        assert!(tmp.path().join("top.txt").exists());
    }

    #[test]
    fn a_cancelled_delete_stops() {
        let tmp = tree();
        let cancel = AtomicBool::new(true);
        let err = delete_tree(&tmp.path().join("a"), &cancel, &mut |_| {}).unwrap_err();
        assert_eq!(err.code, ErrCode::Cancelled);
        assert!(tmp.path().join("a/b/c/three.bin").exists());
    }

    #[test]
    fn the_manifest_lists_parents_first_with_relative_names() {
        let tmp = tree();
        let cancel = AtomicBool::new(false);
        let mut items = Vec::new();
        manifest_tree(&tmp.path().join("a"), &cancel, &mut |i| items.push(i)).unwrap();
        assert_eq!(items[0].rel, b"");
        assert_eq!(items[0].kind, Kind::Dir);
        let pos = |rel: &[u8]| items.iter().position(|i| i.rel == rel).unwrap();
        assert!(pos(b"b") < pos(b"b/c"));
        assert!(pos(b"b/c") < pos(b"b/c/three.bin"));
        assert_eq!(items[pos(b"b/two.bin")].size, 5000);
        assert_eq!(items[pos(b"b")].size, 0);
        assert_eq!(items.len(), 6);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_listed_and_unlinked_never_followed() {
        let tmp = tree();
        let r = tmp.path();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep.txt"), b"k").unwrap();
        std::os::unix::fs::symlink(outside.path(), r.join("a/escape")).unwrap();

        let cancel = AtomicBool::new(false);
        let mut items = Vec::new();
        manifest_tree(&r.join("a"), &cancel, &mut |i| items.push(i)).unwrap();
        let link = items.iter().find(|i| i.rel == b"escape").unwrap();
        assert_eq!(link.kind, Kind::Symlink);
        assert!(!items.iter().any(|i| i.rel.starts_with(b"escape/")));

        let (items, totals) = delete_tree(&r.join("a"), &cancel, &mut |_| {}).unwrap();
        assert_eq!(items, 7);
        assert_eq!(totals.files, 4, "three files and the link");
        assert!(outside.path().join("keep.txt").exists());
    }

    #[test]
    fn local_actions_confine_requests_to_the_root() {
        let tmp = tree();
        let root = tmp.path().join("a");
        let acts = LocalActions::new(&root);
        let b = |p: PathBuf| path_to_bytes(&p);
        assert!(acts.resolve_scan_root(&b(root.clone())).is_ok());
        assert!(acts.resolve_scan_root(&b(root.join("b"))).is_ok());
        assert_eq!(
            acts.resolve_scan_root(&b(tmp.path().to_path_buf()))
                .unwrap_err()
                .code,
            ErrCode::OutsideRoot
        );
        assert_eq!(
            acts.resolve_delete(&b(root.clone())).unwrap_err().code,
            ErrCode::ProtectedRoot
        );
        assert_eq!(
            acts.resolve_delete(&b(tmp.path().join("top.txt")))
                .unwrap_err()
                .code,
            ErrCode::OutsideRoot
        );
        assert_eq!(
            acts.resolve_delete(&b(root.join("nope"))).unwrap_err().code,
            ErrCode::NotFound
        );
        assert!(acts.resolve_copy(&b(root.clone())).is_ok());
        let resolved = acts.resolve_delete(&b(root.join("b"))).unwrap();
        let scan_root = acts.resolve_scan_root(&b(root.clone())).unwrap();
        let mut scan_path = scan_root.clone();
        scan_path.extend_from_slice(b"/b");
        assert!(acts.same_entry(&scan_path, &resolved));
    }
}

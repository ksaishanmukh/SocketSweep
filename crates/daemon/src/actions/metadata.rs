//! What the actions read from file metadata: kind, size on disk, mode,
//! mtime, and an entry's share of the totals.

use std::fs::Metadata;

use socketsweep_wire::{Kind, Totals, BLOCK_SIZE};

pub(super) fn kind_of(m: &Metadata) -> Kind {
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
pub(super) fn blocks_of(m: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks()
}

#[cfg(not(unix))]
pub(super) fn blocks_of(m: &Metadata) -> u64 {
    m.len().div_ceil(BLOCK_SIZE)
}

#[cfg(unix)]
pub(super) fn mode_of(m: &Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
pub(super) fn mode_of(m: &Metadata) -> u32 {
    let base = if m.is_dir() { 0o755 } else { 0o644 };
    if m.permissions().readonly() {
        base & 0o555
    } else {
        base
    }
}

#[cfg(unix)]
pub(super) fn mtime_of(m: &Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    m.mtime()
}

#[cfg(not(unix))]
pub(super) fn mtime_of(m: &Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Add one removed (or listed) entry to `t`, as the engine counts it.
pub(super) fn account(t: &mut Totals, m: &Metadata) {
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

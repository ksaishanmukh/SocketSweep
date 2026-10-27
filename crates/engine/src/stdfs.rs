//! A portable [`Fs`] on `std::fs`, for tests and host-side tools. The phone
//! daemon uses its own rustix implementation (`openat`, `getdents64`,
//! `statat`), which can tell `DT_UNKNOWN` apart and reuses one directory
//! handle; this one goes through paths.
//!
//! On non-Unix hosts paths are converted through UTF-8 (lossily), `blocks`
//! is estimated from the size, and `dev`, `ino` are 0 with `nlink` 1.

use std::fs::{self, Metadata};
use std::io;
use std::path::PathBuf;

use socketsweep_wire::{FsOp, Kind, Stat};

use crate::fs::{errno, DirRead, Fs, FsEntry, FsError, FsStat, Interrupt, RootInfo, StatResult};

/// [`Fs`] over `std::fs`. Never follows symbolic links.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFs;

#[cfg(unix)]
fn to_path(p: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(p))
}

#[cfg(not(unix))]
fn to_path(p: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(p).into_owned())
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

fn join(dir: &[u8], name: &[u8]) -> PathBuf {
    let mut p = dir.to_vec();
    if p.last() != Some(&b'/') {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    to_path(&p)
}

fn errno_of(e: &io::Error) -> i32 {
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

fn kind_of(t: fs::FileType) -> Kind {
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
fn fs_stat(m: &Metadata) -> FsStat {
    use std::os::unix::fs::MetadataExt;
    FsStat {
        kind: kind_of(m.file_type()),
        stat: Stat {
            apparent: m.size(),
            blocks: m.blocks(),
            mtime: m.mtime(),
        },
        dev: m.dev(),
        ino: m.ino(),
        nlink: m.nlink(),
    }
}

#[cfg(not(unix))]
fn fs_stat(m: &Metadata) -> FsStat {
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    FsStat {
        kind: kind_of(m.file_type()),
        stat: Stat {
            apparent: m.len(),
            blocks: m.len().div_ceil(socketsweep_wire::BLOCK_SIZE),
            mtime,
        },
        dev: 0,
        ino: 0,
        nlink: 1,
    }
}

fn lstat(p: &std::path::Path) -> StatResult {
    fs::symlink_metadata(p)
        .map(|m| fs_stat(&m))
        .map_err(|e| errno_of(&e))
}

impl StdFs {
    fn read(&self, dir: &[u8], with_stats: bool, intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        let path = to_path(dir);
        let fail = |op, e: io::Error| FsError::Failed {
            op,
            errno: errno_of(&e),
        };
        let meta = fs::symlink_metadata(&path).map_err(|e| fail(FsOp::Open, e))?;
        if !meta.is_dir() {
            return Err(FsError::Failed {
                op: FsOp::Open,
                errno: errno::ENOTDIR,
            });
        }
        let dev = fs_stat(&meta).dev;
        let rd = fs::read_dir(&path).map_err(|e| fail(FsOp::Open, e))?;
        let mut entries = Vec::new();
        for (i, ent) in rd.enumerate() {
            intr.poll(i)?;
            let ent = ent.map_err(|e| fail(FsOp::ReadDir, e))?;
            let name = name_bytes(&ent.file_name());
            let (kind, stat) = if with_stats {
                let st = lstat(&ent.path());
                let kind = match (&st, ent.file_type()) {
                    (Ok(s), _) => s.kind,
                    (Err(_), Ok(t)) => kind_of(t),
                    (Err(_), Err(_)) => Kind::Other,
                };
                (kind, Some(st))
            } else {
                match ent.file_type() {
                    Ok(t) => (kind_of(t), None),
                    // The equivalent of DT_UNKNOWN: stat it now.
                    Err(_) => {
                        let st = lstat(&ent.path());
                        (st.as_ref().map_or(Kind::Other, |s| s.kind), Some(st))
                    }
                }
            };
            entries.push(FsEntry { name, kind, stat });
        }
        Ok(DirRead { dev, entries })
    }
}

impl Fs for StdFs {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        let m = fs::symlink_metadata(to_path(path)).map_err(|e| FsError::Failed {
            op: FsOp::Open,
            errno: errno_of(&e),
        })?;
        if !m.is_dir() {
            return Err(FsError::Failed {
                op: FsOp::Open,
                errno: errno::ENOTDIR,
            });
        }
        Ok(RootInfo {
            path: path.to_vec(),
            dev: fs_stat(&m).dev,
        })
    }

    fn read_names(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        self.read(dir, false, intr)
    }

    fn stat_all(
        &self,
        dir: &[u8],
        names: &[Vec<u8>],
        intr: Interrupt<'_>,
    ) -> Result<Vec<StatResult>, FsError> {
        let m = fs::symlink_metadata(to_path(dir)).map_err(|e| FsError::Failed {
            op: FsOp::Open,
            errno: errno_of(&e),
        })?;
        if !m.is_dir() {
            return Err(FsError::Failed {
                op: FsOp::Open,
                errno: errno::ENOTDIR,
            });
        }
        let mut out = Vec::with_capacity(names.len());
        for (i, n) in names.iter().enumerate() {
            intr.poll(i)?;
            out.push(lstat(&join(dir, n)));
        }
        Ok(out)
    }

    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        self.read(dir, true, intr)
    }
}

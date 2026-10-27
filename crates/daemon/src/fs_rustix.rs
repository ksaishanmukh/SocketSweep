//! `fswalk`: the engine's [`Fs`] on the phone, with rustix. Linux/Android
//! only.
//!
//! - Every directory is opened `openat(O_RDONLY | O_DIRECTORY | O_NOFOLLOW |
//!   O_CLOEXEC)`, so a directory swapped for a symbolic link is refused
//!   rather than followed, and its device comes from `fstat` of that handle.
//! - Names come from `getdents64` (rustix [`RawDir`]) with `d_type`; an entry
//!   whose type is `DT_UNKNOWN` is `lstat`ed at once, inside the names read,
//!   and never again.
//! - Stats are `statat(dirfd, name, AT_SYMLINK_NOFOLLOW)` through the one open
//!   handle, so a deferred stat batch costs one open.
//! - The [`Interrupt`] is polled every
//!   [`CANCEL_CHECK_ENTRIES`](socketsweep_wire::CANCEL_CHECK_ENTRIES) entries.

use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};

use rustix::fs::{fstat, openat, statat, AtFlags, FileType, Mode, OFlags, RawDir, CWD};
use socketsweep_engine::{DirRead, Fs, FsEntry, FsError, FsStat, Interrupt, RootInfo, StatResult};
use socketsweep_wire::{FsOp, Kind, Stat};

/// Bytes of `getdents64` buffer per directory read.
const DIRENT_BUF: usize = 32 * 1024;

/// The real [`Fs`]. Stateless; safe to share between workers.
#[derive(Debug, Clone, Copy, Default)]
pub struct RustixFs;

fn kind_of(t: FileType) -> Kind {
    match t {
        FileType::RegularFile => Kind::File,
        FileType::Directory => Kind::Dir,
        FileType::Symlink => Kind::Symlink,
        _ => Kind::Other,
    }
}

// The field types of `struct stat` differ between Linux (linux_raw) and
// Android (libc's `stat64`), so some of these casts are no-ops on one of them.
#[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
fn fs_stat(st: &rustix::fs::Stat) -> FsStat {
    FsStat {
        kind: kind_of(FileType::from_raw_mode(st.st_mode as _)),
        stat: Stat {
            apparent: st.st_size.max(0) as u64,
            blocks: st.st_blocks.max(0) as u64,
            mtime: st.st_mtime as i64,
        },
        dev: st.st_dev as u64,
        ino: st.st_ino as u64,
        nlink: st.st_nlink as u64,
    }
}

fn open_dir(path: &[u8]) -> Result<OwnedFd, FsError> {
    openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| FsError::Failed {
        op: FsOp::Open,
        errno: e.raw_os_error(),
    })
}

#[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
fn dev_of(fd: &OwnedFd) -> Result<u64, FsError> {
    fstat(fd)
        .map(|st| st.st_dev as u64)
        .map_err(|e| FsError::Failed {
            op: FsOp::Open,
            errno: e.raw_os_error(),
        })
}

fn lstat_at(fd: &OwnedFd, name: &[u8]) -> StatResult {
    statat(fd, name, AtFlags::SYMLINK_NOFOLLOW)
        .map(|st| fs_stat(&st))
        .map_err(|e| e.raw_os_error())
}

impl RustixFs {
    fn read(&self, dir: &[u8], full: bool, intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        let fd = open_dir(dir)?;
        let dev = dev_of(&fd)?;
        let mut buf = vec![MaybeUninit::<u8>::uninit(); DIRENT_BUF];
        let mut iter = RawDir::new(fd.as_fd(), &mut buf);
        let mut entries = Vec::new();
        while let Some(ent) = iter.next() {
            let ent = ent.map_err(|e| FsError::Failed {
                op: FsOp::ReadDir,
                errno: e.raw_os_error(),
            })?;
            let name = ent.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            intr.poll(entries.len())?;
            let ft = ent.file_type();
            let (kind, stat) = if full || ft == FileType::Unknown {
                let st = lstat_at(&fd, name);
                let kind = match (&st, ft) {
                    (Ok(s), _) => s.kind,
                    (Err(_), FileType::Unknown) => Kind::Other,
                    (Err(_), t) => kind_of(t),
                };
                (kind, Some(st))
            } else {
                (kind_of(ft), None)
            };
            entries.push(FsEntry {
                name: name.to_vec(),
                kind,
                stat,
            });
        }
        Ok(DirRead { dev, entries })
    }
}

impl Fs for RustixFs {
    fn root(&self, path: &[u8]) -> Result<RootInfo, FsError> {
        let fd = open_dir(path)?;
        Ok(RootInfo {
            path: path.to_vec(),
            dev: dev_of(&fd)?,
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
        let fd = open_dir(dir)?;
        let mut out = Vec::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            intr.poll(i)?;
            out.push(lstat_at(&fd, name));
        }
        Ok(out)
    }

    fn list_full(&self, dir: &[u8], intr: Interrupt<'_>) -> Result<DirRead, FsError> {
        self.read(dir, true, intr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::atomic::AtomicBool;

    use socketsweep_engine::StdFs;

    fn bytes(p: &std::path::Path) -> Vec<u8> {
        p.as_os_str().as_bytes().to_vec()
    }

    fn sorted(mut r: DirRead) -> DirRead {
        r.entries.sort_by(|a, b| a.name.cmp(&b.name));
        r
    }

    #[test]
    fn listings_and_stats_match_std() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        std::fs::create_dir(r.join("sub")).unwrap();
        std::fs::write(r.join("file"), vec![1u8; 5000]).unwrap();
        std::fs::write(r.join("name\u{e9}"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc", r.join("link")).unwrap();
        let dir = bytes(r);

        let full = sorted(RustixFs.list_full(&dir, Interrupt::NEVER).unwrap());
        let expected = sorted(StdFs.list_full(&dir, Interrupt::NEVER).unwrap());
        assert_eq!(full, expected);

        let names = sorted(RustixFs.read_names(&dir, Interrupt::NEVER).unwrap());
        let kinds: Vec<(Vec<u8>, Kind)> = names
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (b"file".to_vec(), Kind::File),
                (b"link".to_vec(), Kind::Symlink),
                ("name\u{e9}".as_bytes().to_vec(), Kind::File),
                (b"sub".to_vec(), Kind::Dir),
            ]
        );

        let list: Vec<Vec<u8>> = kinds.iter().map(|(n, _)| n.clone()).collect();
        let mut wanted = list.clone();
        wanted.push(b"missing".to_vec());
        let stats = RustixFs.stat_all(&dir, &wanted, Interrupt::NEVER).unwrap();
        assert_eq!(stats[0].unwrap().stat.apparent, 5000);
        assert_eq!(stats[1].unwrap().kind, Kind::Symlink);
        assert_eq!(stats[4], Err(socketsweep_engine::errno::ENOENT));

        assert_eq!(RustixFs.root(&dir).unwrap().dev, full.dev);
    }

    #[test]
    fn a_symlinked_directory_is_not_opened() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        std::fs::create_dir(r.join("real")).unwrap();
        std::os::unix::fs::symlink(r.join("real"), r.join("link")).unwrap();
        let err = RustixFs
            .read_names(&bytes(&r.join("link")), Interrupt::NEVER)
            .unwrap_err();
        assert!(matches!(err, FsError::Failed { op: FsOp::Open, .. }));
    }

    #[test]
    fn an_interrupt_stops_a_large_read() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..600 {
            std::fs::write(tmp.path().join(format!("f{i}")), b"").unwrap();
        }
        let flag = AtomicBool::new(true);
        let err = RustixFs
            .list_full(&bytes(tmp.path()), Interrupt::new(&flag))
            .unwrap_err();
        assert_eq!(err, FsError::Interrupted);
    }
}

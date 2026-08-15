//! Parallel filesystem scan engine.
//!
//! Platform-generic and free of socket or Android specifics, so the traversal
//! logic can be exercised against a fixture directory on any development
//! machine rather than only on a handset.
//!
//! # Why parallel
//!
//! On Android 11+ `/sdcard` is served through a FUSE emulation layer, so the
//! walk is bound by per-syscall latency rather than CPU. Issuing many `readdir`
//! and `stat` calls concurrently hides that latency. The best thread count is a
//! property of the handset, which is why [`ScanConfig::threads`] is tunable.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jwalk::{Parallelism, WalkDirGeneric};
use socketsweep_protocol::{write_msg, Entry, EntryKind, Frame, ProtocolError, ScanStats};

pub struct ScanConfig {
    pub root: PathBuf,
    /// 0 uses one thread per core. The FUSE layer does not necessarily scale
    /// with core count, so measure on the target device before pinning a value.
    pub threads: usize,
    /// Guards against pathological nesting and symlink loops that survive the
    /// symlink skip (bind mounts, for instance).
    pub max_depth: usize,
}

impl ScanConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            threads: 0,
            max_depth: 64,
        }
    }
}

#[derive(Debug)]
pub enum ScanError {
    /// The root does not exist, is not a directory, or cannot be stat'd.
    BadRoot {
        path: PathBuf,
        reason: String,
    },
    Protocol(ProtocolError),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::BadRoot { path, reason } => {
                write!(f, "cannot scan {}: {reason}", path.display())
            }
            ScanError::Protocol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<ProtocolError> for ScanError {
    fn from(e: ProtocolError) -> Self {
        ScanError::Protocol(e)
    }
}

#[derive(Default)]
struct Counters {
    files: AtomicU64,
    dirs: AtomicU64,
    total_size: AtomicU64,
    errors: AtomicU64,
}

/// Walk `cfg.root`, streaming one [`Frame::Dir`] per directory into `sink`.
///
/// The terminating [`Frame::ScanDone`] is the caller's to write, so it can
/// decide what to report if the walk fails part-way through.
///
/// `sink` is taken by value and must be `'static` because jwalk requires its
/// `process_read_dir` callback to be `'static`. Callers with a borrowed stream
/// should hand over an owned clone (`UnixStream::try_clone`, for instance).
pub fn scan<W: Write + Send + 'static>(cfg: &ScanConfig, sink: W) -> Result<ScanStats, ScanError> {
    let meta = std::fs::metadata(&cfg.root).map_err(|e| ScanError::BadRoot {
        path: cfg.root.clone(),
        reason: e.to_string(),
    })?;
    if !meta.is_dir() {
        return Err(ScanError::BadRoot {
            path: cfg.root.clone(),
            reason: "not a directory".into(),
        });
    }

    let started = Instant::now();

    let out = Arc::new(Mutex::new(BufWriter::with_capacity(256 * 1024, sink)));
    let counters = Arc::new(Counters::default());
    counters.dirs.store(1, Ordering::Relaxed); // the root itself
                                               // The walk closure cannot return an error, so the first write failure is
                                               // parked here and surfaced once the walk finishes.
    let write_failure: Arc<Mutex<Option<ProtocolError>>> = Arc::new(Mutex::new(None));

    let parallelism = match cfg.threads {
        0 => Parallelism::RayonDefaultPool {
            busy_timeout: std::time::Duration::from_secs(1),
        },
        n => Parallelism::RayonNewPool(n),
    };

    let walk_out = Arc::clone(&out);
    let walk_counters = Arc::clone(&counters);
    let walk_failure = Arc::clone(&write_failure);

    let walk = WalkDirGeneric::<((), ())>::new(&cfg.root)
        .parallelism(parallelism)
        .max_depth(cfg.max_depth)
        .skip_hidden(false) // dotfiles occupy real space and must be counted
        .follow_links(false)
        .process_read_dir(move |depth, dir_path, _state, children| {
            // jwalk opens with a synthetic read (depth None) whose only child is
            // the root entry itself. Emitting it would invent a frame for the
            // root's parent and double-count the root as a directory.
            if depth.is_none() {
                return;
            }

            let (out, counters, write_failure) = (&walk_out, &walk_counters, &walk_failure);
            let mut entries = Vec::with_capacity(children.len());

            for child in children.iter_mut() {
                let child = match child {
                    Ok(c) => c,
                    Err(_) => {
                        counters.errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };

                let file_type = child.file_type();

                // Skip rather than follow: symlinks double-count storage and
                // can form cycles.
                if file_type.is_symlink() {
                    child.read_children_path = None;
                    continue;
                }

                if file_type.is_dir() {
                    counters.dirs.fetch_add(1, Ordering::Relaxed);
                    entries.push(Entry {
                        name: os_str_bytes(child.file_name()),
                        size: 0,
                        kind: EntryKind::Dir,
                    });
                } else {
                    let size = match child.metadata() {
                        Ok(m) => m.len(),
                        Err(_) => {
                            counters.errors.fetch_add(1, Ordering::Relaxed);
                            0
                        }
                    };
                    counters.files.fetch_add(1, Ordering::Relaxed);
                    counters.total_size.fetch_add(size, Ordering::Relaxed);
                    entries.push(Entry {
                        name: os_str_bytes(child.file_name()),
                        size,
                        kind: EntryKind::File,
                    });
                }
            }

            let frame = Frame::Dir {
                path: path_bytes(dir_path),
                entries,
            };
            let mut guard = out.lock().expect("scan writer mutex poisoned");
            if let Err(e) = write_msg(&mut *guard, &frame) {
                let mut slot = write_failure.lock().expect("scan error mutex poisoned");
                if slot.is_none() {
                    *slot = Some(e);
                }
            }
        });

    for item in walk {
        if item.is_err() {
            // A directory that could not be opened at all. Its entry was still
            // reported by its parent, so the tree stays consistent.
            counters.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    if let Some(e) = write_failure
        .lock()
        .expect("scan error mutex poisoned")
        .take()
    {
        return Err(e.into());
    }

    out.lock()
        .expect("scan writer mutex poisoned")
        .flush()
        .map_err(|e| ScanError::Protocol(ProtocolError::Io(e)))?;

    Ok(ScanStats {
        files: counters.files.load(Ordering::Relaxed),
        dirs: counters.dirs.load(Ordering::Relaxed),
        total_size: counters.total_size.load(Ordering::Relaxed),
        errors: counters.errors.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

// Android paths are arbitrary bytes. On a Windows or macOS test host there is no
// byte view of an `OsStr`, but fixtures there are ASCII, so a lossy conversion
// is exact in practice. The divergence is confined to these two functions.

#[cfg(unix)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn os_str_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_str_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

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
use std::time::{Duration, Instant};

use jwalk::{Parallelism, WalkDirGeneric};
use socketsweep_protocol::{
    write_msg, Entry, EntryKind, Frame, ProtocolError, ScanStats, KEEPALIVE_INTERVAL,
};

pub struct ScanConfig {
    pub root: PathBuf,
    /// 0 uses one thread per core. The FUSE layer does not necessarily scale
    /// with core count, so measure on the target device before pinning a value.
    pub threads: usize,
    /// Guards against pathological nesting and symlink loops that survive the
    /// symlink skip (bind mounts, for instance). The root is depth 0. A
    /// directory too deep to read still appears as an entry in its parent, gets
    /// no frame of its own, and is counted in [`ScanStats::errors`].
    pub max_depth: usize,
    /// How long the scan may go without sending anything before it sends a
    /// [`Frame::Keepalive`]. Checked every [`KEEPALIVE_CHECK_EVERY`] entries
    /// while a directory is being read, so a single huge directory, whose frame
    /// cannot go out until every file in it has been stat'ed, does not look to
    /// the host like a daemon that has died. Defaults to the protocol's
    /// [`KEEPALIVE_INTERVAL`].
    pub keepalive: Duration,
}

impl ScanConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            threads: 0,
            max_depth: 64,
            keepalive: KEEPALIVE_INTERVAL,
        }
    }
}

/// How many entries of a directory are processed between checks on whether a
/// keepalive is due. Large enough that the check costs nothing next to the
/// stats in between, small enough that even at FUSE speeds it comes round every
/// second or so.
pub const KEEPALIVE_CHECK_EVERY: usize = 1024;

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
/// decide what to report if the walk fails part-way through. The first failed
/// write stops the walk descending further and is returned once it has wound
/// down.
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

    let clock = Arc::new(FlushClock::new());
    let out = Arc::new(Mutex::new(Output {
        writer: BufWriter::with_capacity(256 * 1024, sink),
        clock: Arc::clone(&clock),
    }));
    let counters = Arc::new(Counters::default());
    counters.dirs.store(1, Ordering::Relaxed); // the root itself

    // The walk closure cannot return an error, so the first write failure is
    // parked here and surfaced once the walk finishes. Its presence also tells
    // the walk to stop: the usual cause is a host that has disconnected.
    let write_failure: Arc<Mutex<Option<ProtocolError>>> = Arc::new(Mutex::new(None));
    let max_depth = cfg.max_depth;
    let keepalive = cfg.keepalive;

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
            let Some(depth) = depth else {
                if max_depth == 0 {
                    // Not even the root is read, so it goes unreported.
                    walk_counters.errors.fetch_add(1, Ordering::Relaxed);
                }
                return;
            };

            let (out, counters, write_failure) = (&walk_out, &walk_counters, &walk_failure);

            // Record the first failed write, and stop jwalk descending further.
            let fail = |e: ProtocolError, children: &mut Vec<_>| {
                let mut slot = write_failure.lock().expect("scan error mutex poisoned");
                if slot.is_none() {
                    *slot = Some(e);
                }
                drop(slot);
                prune(children);
            };

            // Once a frame has failed to send, nothing more can reach the host.
            // Pruning every child stops jwalk descending any further, so the
            // walk winds down instead of reading the rest of the tree.
            if write_failure
                .lock()
                .expect("scan error mutex poisoned")
                .is_some()
            {
                prune(children);
                return;
            }

            // jwalk reads a directory's contents only while they sit within
            // `max_depth`, so a subdirectory listed here at the limit is never
            // read and never gets a frame of its own. It is counted as an error
            // so the stats admit to the gap rather than silently under-reporting.
            let children_are_cut_off = depth + 1 >= max_depth;

            let mut entries = Vec::with_capacity(children.len());
            let mut keepalive_failed = None;

            for (i, child) in children.iter_mut().enumerate() {
                // The clock is read without the lock; the lock is taken only
                // when a keepalive looks due, and `keepalive` re-checks under
                // it in case another thread has sent something meanwhile.
                if i % KEEPALIVE_CHECK_EVERY == KEEPALIVE_CHECK_EVERY - 1
                    && clock.quiet_for() >= keepalive
                {
                    let sent = out
                        .lock()
                        .expect("scan writer mutex poisoned")
                        .keepalive(keepalive);
                    if let Err(e) = sent {
                        keepalive_failed = Some(e);
                        break;
                    }
                }

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
                    if children_are_cut_off {
                        child.read_children_path = None;
                        counters.errors.fetch_add(1, Ordering::Relaxed);
                    }
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

            if let Some(e) = keepalive_failed {
                fail(e, children);
                return;
            }

            let frame = Frame::Dir {
                path: path_bytes(dir_path),
                entries,
            };
            let written = out.lock().expect("scan writer mutex poisoned").send(&frame);
            if let Err(e) = written {
                fail(e, children);
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
        .writer
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

/// How often buffered frames are pushed to the host. Without it the buffer
/// drains only when full, and on a slow stretch of FUSE filling it can take
/// long enough that the host's read timeout fires on a scan that is still
/// making progress.
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// The shared sink. Batches frames into large writes, and flushes after any
/// frame written [`FLUSH_INTERVAL`] or more since the last flush.
struct Output<W: Write> {
    writer: BufWriter<W>,
    clock: Arc<FlushClock>,
}

impl<W: Write> Output<W> {
    fn send(&mut self, frame: &Frame) -> Result<(), ProtocolError> {
        write_msg(&mut self.writer, frame)?;
        if self.clock.quiet_for() >= FLUSH_INTERVAL {
            self.flush()?;
        }
        Ok(())
    }

    /// Send a [`Frame::Keepalive`] if nothing has been flushed for `after`.
    ///
    /// Measured from the last flush rather than the last write, because a frame
    /// still sitting in the buffer has not reached the host either; the flush
    /// here pushes it out along with the keepalive.
    fn keepalive(&mut self, after: Duration) -> Result<(), ProtocolError> {
        if self.clock.quiet_for() < after {
            return Ok(());
        }
        write_msg(&mut self.writer, &Frame::Keepalive)?;
        self.flush()
    }

    fn flush(&mut self) -> Result<(), ProtocolError> {
        self.writer.flush()?;
        self.clock.mark();
        Ok(())
    }
}

/// When [`Output`] last flushed, readable without its lock, so the walk threads
/// can check whether a keepalive is due without contending for the writer.
struct FlushClock {
    origin: Instant,
    /// Milliseconds after `origin`.
    last_flush_ms: AtomicU64,
}

impl FlushClock {
    fn new() -> Self {
        FlushClock {
            origin: Instant::now(),
            last_flush_ms: AtomicU64::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }

    fn mark(&self) {
        self.last_flush_ms.store(self.now_ms(), Ordering::Relaxed);
    }

    /// How long since the last flush, to the millisecond.
    fn quiet_for(&self) -> Duration {
        let last = self.last_flush_ms.load(Ordering::Relaxed);
        Duration::from_millis(self.now_ms().saturating_sub(last))
    }
}

/// Stop jwalk descending into any of `children`.
fn prune<E>(children: &mut [Result<jwalk::DirEntry<((), ())>, E>]) {
    for child in children.iter_mut().flatten() {
        child.read_children_path = None;
    }
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

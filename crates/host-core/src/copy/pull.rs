//! Creating a plan's directories and pulling its files on worker threads.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use socketsweep_transport::Transport;

use super::{CopyError, CopyPlan, CopyTick, PlannedItem, COPY_WORKERS};
use crate::lock;

/// Create the plan's directories and pull its files from `serial`.
/// `progress` is called from the worker threads.
pub fn execute(
    transport: &dyn Transport,
    serial: &str,
    plan: &CopyPlan,
    cancel: &AtomicBool,
    progress: &(dyn Fn(CopyTick) + Sync),
) -> Result<CopyTick, CopyError> {
    for dir in &plan.dirs {
        std::fs::create_dir_all(&dir.local).map_err(local_err(&dir.local))?;
    }

    let pull = Pull {
        transport,
        serial,
        plan,
        cancel,
        progress,
        next: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
        failure: Mutex::new(None),
        tick: Mutex::new(CopyTick::default()),
    };

    std::thread::scope(|scope| {
        for _ in 0..COPY_WORKERS.min(plan.files.len()) {
            scope.spawn(|| pull.worker());
        }
    });

    if let Some(e) = lock(&pull.failure).take() {
        return Err(e);
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(CopyError::Cancelled);
    }
    let mut done = lock(&pull.tick).clone();
    done.current = None;
    Ok(done)
}

/// What the worker threads of one [`execute`] share.
struct Pull<'a> {
    transport: &'a dyn Transport,
    serial: &'a str,
    plan: &'a CopyPlan,
    cancel: &'a AtomicBool,
    progress: &'a (dyn Fn(CopyTick) + Sync),
    next: AtomicUsize,
    stop: AtomicBool,
    failure: Mutex<Option<CopyError>>,
    tick: Mutex<CopyTick>,
}

impl Pull<'_> {
    /// Take files until none are left or the copy stops.
    fn worker(&self) {
        loop {
            if self.stop.load(Ordering::SeqCst) || self.cancel.load(Ordering::SeqCst) {
                return;
            }
            let Some(file) = self
                .plan
                .files
                .get(self.next.fetch_add(1, Ordering::SeqCst))
            else {
                return;
            };
            if !self.pull_file(file) {
                return;
            }
        }
    }

    /// Pull one file. `false` when it failed and the copy stops.
    fn pull_file(&self, file: &PlannedItem) -> bool {
        let (stop, failure, tick, progress) =
            (&self.stop, &self.failure, &self.tick, self.progress);
        if let Some(parent) = file.local.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                stop.store(true, Ordering::SeqCst);
                lock(failure).get_or_insert(local_err(parent)(e));
                return false;
            }
        }
        {
            let mut t = lock(tick);
            t.current = Some(file.rel.clone());
            progress(t.clone());
        }
        let mut last = 0u64;
        let pulled = self
            .transport
            .pull(self.serial, &file.remote, &file.local, &mut |done| {
                let delta = done.saturating_sub(last);
                last = done;
                let mut t = lock(tick);
                t.bytes_done += delta;
                progress(t.clone());
            });
        match pulled {
            Ok(_) => {
                set_mtime(&file.local, file.mtime);
                let mut t = lock(tick);
                t.files_done += 1;
                progress(t.clone());
                true
            }
            Err(e) => {
                let _ = std::fs::remove_file(&file.local);
                stop.store(true, Ordering::SeqCst);
                lock(failure).get_or_insert(CopyError::Transport(e));
                false
            }
        }
    }
}

fn local_err(path: &Path) -> impl FnOnce(std::io::Error) -> CopyError {
    let path = path.to_path_buf();
    move |source| CopyError::Local { path, source }
}

/// Best effort: keep the phone's modification time.
fn set_mtime(path: &Path, mtime: i64) {
    let Ok(secs) = u64::try_from(mtime) else {
        return;
    };
    let when = UNIX_EPOCH + Duration::from_secs(secs);
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(when.min(SystemTime::now()));
    }
}

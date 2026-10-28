//! The work a request starts on a thread of its own: a scan, a delete or a
//! copy manifest.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use socketsweep_engine::{FsError, RootInfo, Scan};
use socketsweep_wire::{CopyItem, DirId, ReqId, ScanOpts, ServerMsg, MAX_LISTING_PART};

use super::{lock, Active, Shared};
use crate::actions::ActionError;
use crate::log;
use crate::outbox::ScanSink;

impl Shared {
    fn spawn(self: &Arc<Self>, name: &str, f: impl FnOnce(Arc<Shared>) + Send + 'static) {
        let me = Arc::clone(self);
        match std::thread::Builder::new()
            .name(name.into())
            .spawn(move || f(me))
        {
            Ok(t) => {
                let mut threads = lock(&self.threads);
                threads.retain(|t| !t.is_finished());
                threads.push(t);
            }
            Err(e) => log!("cannot start a {name} thread: {e}"),
        }
    }

    pub(super) fn start_scan(self: &Arc<Self>, id: ReqId, root: Vec<u8>, mut opts: ScanOpts) {
        let Some((resolved, info)) = self.open_scan_root(id, &root) else {
            return;
        };
        let limits = self.cfg.limits;
        opts.max_depth = opts.max_depth.min(limits.max_depth);
        let workers = opts
            .workers
            .unwrap_or(self.cfg.workers)
            .clamp(1, limits.max_workers);
        let sink = ScanSink {
            id,
            outbox: Arc::clone(&self.outbox),
        };
        let scan = Scan::with_workers(info, &opts, workers, sink);
        lock(&self.active).insert(id, Active::Scan(scan.clone()));
        log!(
            "scan {id} of {} with {workers} workers, {:?} mode",
            String::from_utf8_lossy(&resolved),
            opts.mode
        );
        self.spawn("ssd-scan", move |me| {
            let started = Instant::now();
            let outcome = scan.run(&*me.services.fs);
            let totals = scan.totals();
            lock(&me.active).remove(&id);
            log!(
                "scan {id}: {outcome:?} after {} ms, {} files, {} dirs, {} bytes on disk, {} errors",
                started.elapsed().as_millis(),
                totals.files,
                totals.dirs,
                totals.disk,
                totals.errors
            );
        });
    }

    /// Resolve a scan's root through the guard and open it. On failure,
    /// answers `Failed` and returns `None`.
    fn open_scan_root(&self, id: ReqId, root: &[u8]) -> Option<(Vec<u8>, RootInfo)> {
        let resolved = match self.services.actions.resolve_scan_root(root) {
            Ok(r) => r,
            Err(e) => {
                log!("scan {id} refused: {}", e.message);
                self.fail(id, e);
                return None;
            }
        };
        let info = match self.services.fs.root(&resolved) {
            Ok(info) => info,
            Err(FsError::Failed { errno, .. }) => {
                let e = ActionError::errno(
                    errno,
                    format!(
                        "cannot open {}: errno {errno}",
                        String::from_utf8_lossy(&resolved)
                    ),
                );
                log!("scan {id} failed: {}", e.message);
                self.fail(id, e);
                return None;
            }
            Err(FsError::Interrupted) => {
                self.fail(id, ActionError::cancelled());
                return None;
            }
        };
        Some((resolved, info))
    }

    pub(super) fn start_delete(
        self: &Arc<Self>,
        id: ReqId,
        path: Vec<u8>,
        hint: Option<(ReqId, DirId)>,
    ) {
        let flag = Arc::new(AtomicBool::new(false));
        lock(&self.active).insert(id, Active::Job(Arc::clone(&flag)));
        self.spawn("ssd-delete", move |me| {
            let result = me.delete(id, &path, hint, &flag);
            lock(&me.active).remove(&id);
            match result {
                Ok((items, totals)) => {
                    log!("delete {id}: removed {items} items");
                    me.outbox.reply(id, ServerMsg::Deleted { items, totals });
                }
                Err(e) => {
                    log!("delete {id} failed: {}", e.message);
                    me.fail(id, e);
                }
            }
        });
    }

    fn delete(
        &self,
        id: ReqId,
        path: &[u8],
        hint: Option<(ReqId, DirId)>,
        cancel: &AtomicBool,
    ) -> Result<(u64, socketsweep_wire::Totals), ActionError> {
        let actions = &self.services.actions;
        let resolved = actions.resolve_delete(path)?;
        if let Some((scan_id, dir)) = hint {
            match self.scan(scan_id) {
                Some(scan) => match scan.path_of(dir) {
                    // Prune before touching the files, so the scan never
                    // reports half-deleted directories.
                    Some(p) if actions.same_entry(&p, &resolved) => {
                        scan.prune(dir);
                    }
                    _ => log!(
                        "delete {id}: hint ({scan_id}, {dir}) does not match the path; ignored"
                    ),
                },
                None => log!("delete {id}: hinted scan {scan_id} is not running"),
            }
        }
        let every = self.cfg.progress_every;
        let mut last = Instant::now();
        let outbox = &self.outbox;
        actions.delete(&resolved, cancel, &mut |items| {
            if last.elapsed() >= every {
                last = Instant::now();
                outbox.reply(id, ServerMsg::DeleteProgress { items });
            }
        })
    }

    pub(super) fn start_copy(self: &Arc<Self>, id: ReqId, path: Vec<u8>) {
        let flag = Arc::new(AtomicBool::new(false));
        lock(&self.active).insert(id, Active::Job(Arc::clone(&flag)));
        self.spawn("ssd-copy", move |me| {
            let result = me.copy(id, &path, &flag);
            lock(&me.active).remove(&id);
            if let Err(e) = result {
                log!("copy manifest {id} failed: {}", e.message);
                me.fail(id, e);
            }
        });
    }

    fn copy(&self, id: ReqId, path: &[u8], cancel: &AtomicBool) -> Result<(), ActionError> {
        let actions = &self.services.actions;
        let root = actions.resolve_copy(path)?;
        let mut part: Vec<CopyItem> = Vec::new();
        actions.manifest(&root, cancel, &mut |item| {
            part.push(item);
            if part.len() >= MAX_LISTING_PART {
                self.outbox.reply(
                    id,
                    ServerMsg::CopyManifest {
                        root: root.clone(),
                        files: std::mem::take(&mut part),
                        last: false,
                    },
                );
            }
        })?;
        self.outbox.reply(
            id,
            ServerMsg::CopyManifest {
                root,
                files: part,
                last: true,
            },
        );
        Ok(())
    }
}

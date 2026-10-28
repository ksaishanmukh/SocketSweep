//! The ingest thread: folding the scan's replies into the tree, and the
//! progress it reports.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Instant;

use socketsweep_gui_contract::{
    self as gui, Measure, ScanFinished, ScanOutcome, ScanPhase, ScanProgress, SkeletonReady,
};
use socketsweep_wire::{Outcome, ScanEvent, ServerMsg};

use crate::app::state::{Inner, Scan};
use crate::app::HostEvent;
use crate::lock;
use crate::tree::{Ingested, TreeError, ROOT};

pub(super) fn spawn_ingest(inner: &Arc<Inner>, scan: Arc<Scan>, rx: Receiver<ServerMsg>) {
    let inner = Arc::clone(inner);
    let s = Arc::clone(&scan);
    let spawned = std::thread::Builder::new()
        .name("socketsweep-ingest".into())
        .spawn(move || ingest_loop(&inner, &s, rx));
    if let Err(e) = spawned {
        scan.fail(format!("cannot start the scan thread: {e}"));
        scan.cancel();
        scan.mark_done();
    }
}

fn ingest_loop(inner: &Inner, scan: &Scan, rx: Receiver<ServerMsg>) {
    let interval = inner.config.progress_interval;
    let mut next_emit = Instant::now() + interval;
    loop {
        let wait = next_emit.saturating_duration_since(Instant::now());
        let ended = match rx.recv_timeout(wait) {
            Ok(ServerMsg::Scan(events)) => match scan.apply(&events) {
                Ok(ended) => ended,
                Err(e) => {
                    scan.fail(e.to_string());
                    let _ = scan.client.cancel(scan.req);
                    true
                }
            },
            Ok(ServerMsg::Failed { message, .. }) => {
                scan.fail(message);
                true
            }
            Ok(other) => {
                scan.fail(format!("unexpected reply to the scan: {other:?}"));
                let _ = scan.client.cancel(scan.req);
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => {
                scan.fail(
                    scan.client
                        .closed_reason()
                        .unwrap_or_else(|| "the scan stopped without finishing".into()),
                );
                true
            }
        };
        if ended {
            break;
        }
        if Instant::now() >= next_emit {
            inner.emit(HostEvent::ScanProgress(scan.progress()));
            next_emit = Instant::now() + interval;
        }
    }
    scan.focus.stop();
    let progress = scan.progress();
    if progress
        .finished
        .as_ref()
        .is_some_and(|f| f.outcome == ScanOutcome::Complete)
    {
        if let Some(history) = &inner.history {
            history.record(&scan.serial, &scan.root, &progress.totals);
        }
    }
    inner.emit(HostEvent::ScanProgress(progress));
    scan.mark_done();
}

impl Scan {
    /// Fold a batch into the tree. `Ok(true)` once the scan has finished.
    pub(super) fn apply(&self, events: &[ScanEvent]) -> Result<bool, TreeError> {
        let mut tree = lock(&self.tree);
        let mut meta = lock(&self.meta);
        for event in events {
            match tree.ingest(event)? {
                Ingested::Tree => {}
                Ingested::SkeletonReady {
                    listed,
                    frontier,
                    elapsed_ms,
                } => {
                    meta.skeleton = Some(SkeletonReady {
                        listed,
                        frontier,
                        elapsed_ms,
                    });
                    if meta.phase == ScanPhase::Skeleton {
                        meta.phase = ScanPhase::Measuring;
                    }
                }
                Ingested::FocusState { seq, node, settled } => {
                    if meta.daemon_focus.is_none_or(|(s, ..)| seq >= s) {
                        meta.daemon_focus = Some((seq, node, settled));
                    }
                }
                Ingested::Progress { listed, known } => {
                    meta.listed = listed;
                    meta.known = known;
                }
                Ingested::Finished {
                    outcome,
                    elapsed_ms,
                    ..
                } => {
                    let (outcome, message) = match outcome {
                        Outcome::Complete => (ScanOutcome::Complete, None),
                        Outcome::Cancelled => (ScanOutcome::Cancelled, None),
                        Outcome::Aborted { code } => (
                            ScanOutcome::Failed,
                            Some(format!("The phone stopped the scan ({code:?}).")),
                        ),
                    };
                    meta.finished = Some(ScanFinished {
                        outcome,
                        elapsed_ms,
                        message,
                    });
                    meta.phase = ScanPhase::Finished;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub(super) fn progress(&self) -> ScanProgress {
        let tree = lock(&self.tree);
        let meta = lock(&self.meta);
        let watching = tree.nearest_live(meta.focus.unwrap_or(ROOT));
        let focus = meta.focus.map(|f| {
            let node = tree.nearest_live(f);
            let daemon_settled = matches!(meta.daemon_focus, Some((_, Some(n), true)) if n == node);
            gui::FocusState {
                node: tree.node_ref(node),
                settled: daemon_settled || tree.measure(node) == Some(Measure::Final),
            }
        });
        ScanProgress {
            session: self.session,
            phase: meta.phase,
            totals: tree.totals(),
            listed: meta.listed,
            known: meta.known,
            elapsed_ms: meta
                .finished
                .as_ref()
                .map_or_else(|| Self::elapsed_ms(&meta), |f| f.elapsed_ms),
            skeleton: meta.skeleton,
            focus,
            finished: meta.finished.clone(),
            view: tree.view(watching, meta.view_limit, meta.view_by).ok(),
        }
    }
}

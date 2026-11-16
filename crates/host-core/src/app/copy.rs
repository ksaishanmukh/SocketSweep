//! Copying nodes to the desktop: the manifests from the phone, then the
//! files, as one copy with one progress stream.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use socketsweep_gui_contract::{CopyProgress, CopyResult, ErrorCode, NodeId, NodeRef};
use socketsweep_transport::Transport;
use socketsweep_wire::{CopyItem, Kind, ReqId, ServerMsg};

use super::errors::{client_err, copy_err, err, tree_err, CmdResult};
use super::state::{Busy, CopyCtl, Inner};
use super::HostEvent;
use crate::client::{Client, ClientError};
use crate::copy::{self, CopyError, CopyPlan, CopyTick};
use crate::lock;
use crate::tree::Tree;

/// What every progress event of one copy names.
struct Job<'a> {
    inner: &'a Inner,
    ctl: &'a CopyCtl,
    nodes: Vec<NodeRef>,
}

pub(super) fn copy_many(
    inner: &Inner,
    nodes: Vec<NodeRef>,
    destination: Option<String>,
) -> CmdResult<CopyResult> {
    let ctl = Arc::new(CopyCtl::default());
    let _busy = inner.claim(Busy::Copying(Arc::clone(&ctl)))?;
    let conn = inner.conn()?;
    let scan = inner.scan()?;
    let transport = inner.transport()?;
    let paths = {
        let tree = lock(&scan.tree);
        sources(&tree, &nodes)?
    };
    if paths.is_empty() {
        return Err(err(ErrorCode::Invalid, "Choose something to copy."));
    }
    let dest = destination
        .map(PathBuf::from)
        .or_else(|| inner.config.default_destination.clone())
        .ok_or_else(|| err(ErrorCode::Invalid, "Choose where to copy to."))?;
    std::fs::create_dir_all(&dest)
        .map_err(|e| err(ErrorCode::Io, format!("Cannot use {}: {e}", dest.display())))?;

    let job = Job {
        inner,
        ctl: &ctl,
        nodes,
    };
    let client = conn.daemon.client.clone();
    let destination = dest.display().to_string();
    let mut manifests = Vec::with_capacity(paths.len());
    let mut listed = (0, 0);
    for path in paths {
        let (root, items) = job.fetch_manifest(&client, path, &destination, listed)?;
        listed.0 += files_in(&items);
        listed.1 += items.iter().map(|i| i.size).sum::<u64>();
        manifests.push((root, items));
        if ctl.cancel.load(Ordering::SeqCst) {
            return Err(copy_err(CopyError::Cancelled));
        }
    }

    let plan = copy::plan_many(&manifests, &dest, &|p| p.exists());
    job.pull_files(transport.as_ref(), &conn.daemon.serial, &plan)
}

pub(super) fn cancel_copy(inner: &Inner) -> CmdResult<()> {
    let ctl = match lock(&inner.busy).as_ref() {
        Some(Busy::Copying(ctl)) => Arc::clone(ctl),
        _ => return Ok(()),
    };
    ctl.cancel.store(true, Ordering::SeqCst);
    if let Some((client, req)) = lock(&ctl.prepare).clone() {
        let _ = client.cancel(req);
    }
    Ok(())
}

/// The phone path of every node, refusing stale ones, without nodes inside
/// another one asked for: copying the outer one brings them along.
fn sources(tree: &Tree, nodes: &[NodeRef]) -> CmdResult<Vec<Vec<u8>>> {
    let ids = nodes
        .iter()
        .map(|n| tree.resolve(*n).map_err(tree_err))
        .collect::<CmdResult<Vec<NodeId>>>()?;
    tree.outermost(&ids)
        .into_iter()
        .map(|id| tree.path_of(id).map_err(tree_err))
        .collect()
}

fn files_in(items: &[CopyItem]) -> u32 {
    items.iter().filter(|i| i.kind == Kind::File).count() as u32
}

impl Job<'_> {
    /// Ask the phone for the manifest of `path`, cancellable through the
    /// copy's control while it is being listed. `before` is what earlier
    /// manifests of this copy listed, as (files, bytes).
    fn fetch_manifest(
        &self,
        client: &Client,
        path: Vec<u8>,
        destination: &str,
        before: (u32, u64),
    ) -> CmdResult<(Vec<u8>, Vec<CopyItem>)> {
        let (req, rx) = client.prepare_copy(path).map_err(client_err)?;
        *lock(&self.ctl.prepare) = Some((client.clone(), req));
        if self.ctl.cancel.load(Ordering::SeqCst) {
            let _ = client.cancel(req);
        }
        let manifest = Self::receive_manifest(client, req, &rx, |items| {
            let totals = (
                before.0 + files_in(items),
                before.1 + items.iter().map(|i| i.size).sum::<u64>(),
            );
            self.emit(destination, &CopyTick::default(), totals, false);
        });
        *lock(&self.ctl.prepare) = None;
        manifest
    }

    /// Collect a manifest's parts, calling `progress` with the items so far
    /// after each.
    fn receive_manifest(
        client: &Client,
        req: ReqId,
        rx: &Receiver<ServerMsg>,
        progress: impl Fn(&[CopyItem]),
    ) -> CmdResult<(Vec<u8>, Vec<CopyItem>)> {
        let mut items = Vec::new();
        loop {
            let msg = rx.recv().map_err(|_| {
                client_err(ClientError::Closed(
                    client.closed_reason().unwrap_or_default(),
                ))
            })?;
            let (root, files, last) = match msg {
                ServerMsg::CopyManifest { root, files, last } => (root, files, last),
                ServerMsg::Failed {
                    code,
                    errno,
                    message,
                } => {
                    return Err(client_err(ClientError::Failed {
                        code,
                        errno,
                        message,
                    }));
                }
                other => {
                    let _ = client.cancel(req);
                    return Err(err(
                        ErrorCode::Daemon,
                        format!("Unexpected reply to the copy: {other:?}"),
                    ));
                }
            };
            items.extend(files);
            progress(&items);
            if last {
                return Ok((root, items));
            }
        }
    }

    /// Pull the plan's files, reporting progress at most every progress
    /// interval.
    fn pull_files(
        &self,
        transport: &dyn Transport,
        serial: &str,
        plan: &CopyPlan,
    ) -> CmdResult<CopyResult> {
        let totals = (plan.files.len() as u32, plan.bytes);
        let destination = plan.top.display().to_string();
        self.emit(&destination, &CopyTick::default(), totals, true);
        let last_emit = Mutex::new(Instant::now());
        let interval = self.inner.config.progress_interval;
        let done = copy::execute(transport, serial, plan, &self.ctl.cancel, &|tick| {
            let mut last = lock(&last_emit);
            if last.elapsed() >= interval || tick.files_done == totals.0 {
                *last = Instant::now();
                drop(last);
                self.emit(&destination, &tick, totals, true);
            }
        })
        .map_err(copy_err)?;
        self.emit(&destination, &done, totals, true);
        Ok(CopyResult {
            destination,
            files: done.files_done,
            bytes: done.bytes_done,
            skipped: plan.skipped,
            renamed: plan.renamed,
        })
    }

    /// `totals` is (files, bytes) of the whole copy, as far as known.
    fn emit(&self, destination: &str, tick: &CopyTick, totals: (u32, u64), known: bool) {
        self.inner.emit(HostEvent::CopyProgress(CopyProgress {
            nodes: self.nodes.clone(),
            destination: destination.to_string(),
            files_done: tick.files_done,
            files_total: totals.0,
            bytes_done: tick.bytes_done,
            bytes_total: totals.1,
            totals_known: known,
            current: tick.current.clone(),
        }));
    }
}

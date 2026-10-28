//! Copying a node to the desktop: the manifest from the phone, then the files.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use socketsweep_gui_contract::{CopyProgress, CopyResult, ErrorCode, NodeRef};
use socketsweep_transport::Transport;
use socketsweep_wire::{CopyItem, ReqId, ServerMsg};

use super::errors::{client_err, copy_err, err, tree_err, CmdResult};
use super::state::{Busy, CopyCtl, Inner};
use super::HostEvent;
use crate::client::{Client, ClientError};
use crate::copy::{self, CopyError, CopyPlan, CopyTick};
use crate::lock;

pub(super) fn copy_to_desktop(
    inner: &Inner,
    node: NodeRef,
    destination: Option<String>,
) -> CmdResult<CopyResult> {
    let ctl = Arc::new(CopyCtl::default());
    let _busy = inner.claim(Busy::Copying(Arc::clone(&ctl)))?;
    let conn = inner.conn()?;
    let scan = inner.scan()?;
    let transport = inner.transport()?;
    let path = {
        let tree = lock(&scan.tree);
        let id = tree.resolve(node).map_err(tree_err)?;
        tree.path_of(id).map_err(tree_err)?
    };
    let dest = destination
        .map(PathBuf::from)
        .or_else(|| inner.config.default_destination.clone())
        .ok_or_else(|| err(ErrorCode::Invalid, "Choose where to copy to."))?;
    std::fs::create_dir_all(&dest)
        .map_err(|e| err(ErrorCode::Io, format!("Cannot use {}: {e}", dest.display())))?;
    let destination = dest.display().to_string();

    let client = conn.daemon.client.clone();
    let (root, items) = fetch_manifest(inner, &ctl, &client, path, node, &destination)?;
    if ctl.cancel.load(Ordering::SeqCst) {
        return Err(copy_err(CopyError::Cancelled));
    }

    let plan = copy::plan(&root, &items, &dest, &|p| p.exists());
    pull_files(
        inner,
        &ctl,
        transport.as_ref(),
        &conn.daemon.serial,
        node,
        &plan,
    )
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

/// Ask the phone for the manifest of `path`, cancellable through `ctl` while
/// it is being listed.
fn fetch_manifest(
    inner: &Inner,
    ctl: &CopyCtl,
    client: &Client,
    path: Vec<u8>,
    node: NodeRef,
    destination: &str,
) -> CmdResult<(Vec<u8>, Vec<CopyItem>)> {
    let (req, rx) = client.prepare_copy(path).map_err(client_err)?;
    *lock(&ctl.prepare) = Some((client.clone(), req));
    if ctl.cancel.load(Ordering::SeqCst) {
        let _ = client.cancel(req);
    }
    let manifest = receive_manifest(inner, client, req, &rx, node, destination);
    *lock(&ctl.prepare) = None;
    manifest
}

/// Collect the manifest's parts, reporting the totals so far after each.
fn receive_manifest(
    inner: &Inner,
    client: &Client,
    req: ReqId,
    rx: &Receiver<ServerMsg>,
    node: NodeRef,
    destination: &str,
) -> CmdResult<(Vec<u8>, Vec<CopyItem>)> {
    let mut items = Vec::new();
    let root = loop {
        match rx.recv() {
            Ok(ServerMsg::CopyManifest {
                root: r,
                files,
                last,
            }) => {
                items.extend(files);
                let files_total = items
                    .iter()
                    .filter(|i| i.kind == socketsweep_wire::Kind::File)
                    .count() as u32;
                let bytes_total = items.iter().map(|i| i.size).sum();
                inner.emit(progress_event(
                    node,
                    destination,
                    &CopyTick::default(),
                    files_total,
                    bytes_total,
                    false,
                ));
                if last {
                    break r;
                }
            }
            Ok(ServerMsg::Failed {
                code,
                errno,
                message,
            }) => {
                return Err(client_err(ClientError::Failed {
                    code,
                    errno,
                    message,
                }));
            }
            Ok(other) => {
                let _ = client.cancel(req);
                return Err(err(
                    ErrorCode::Daemon,
                    format!("Unexpected reply to the copy: {other:?}"),
                ));
            }
            Err(_) => {
                return Err(client_err(ClientError::Closed(
                    client.closed_reason().unwrap_or_default(),
                )));
            }
        }
    };
    Ok((root, items))
}

/// Pull the plan's files, reporting progress at most every progress interval.
fn pull_files(
    inner: &Inner,
    ctl: &CopyCtl,
    transport: &dyn Transport,
    serial: &str,
    node: NodeRef,
    plan: &CopyPlan,
) -> CmdResult<CopyResult> {
    let files_total = plan.files.len() as u32;
    let bytes_total = plan.bytes;
    let destination = plan.top.display().to_string();
    let progress = |tick: &CopyTick, known: bool| {
        progress_event(node, &destination, tick, files_total, bytes_total, known)
    };
    inner.emit(progress(&CopyTick::default(), true));
    let last_emit = Mutex::new(Instant::now());
    let interval = inner.config.progress_interval;
    let done = copy::execute(transport, serial, plan, &ctl.cancel, &|tick| {
        let mut last = lock(&last_emit);
        if last.elapsed() >= interval || tick.files_done == files_total {
            *last = Instant::now();
            drop(last);
            inner.emit(progress(&tick, true));
        }
    })
    .map_err(copy_err)?;
    inner.emit(progress(&done, true));
    Ok(CopyResult {
        destination,
        files: done.files_done,
        bytes: done.bytes_done,
        skipped: plan.skipped,
        renamed: plan.renamed,
    })
}

fn progress_event(
    node: NodeRef,
    destination: &str,
    tick: &CopyTick,
    files_total: u32,
    bytes_total: u64,
    known: bool,
) -> HostEvent {
    HostEvent::CopyProgress(CopyProgress {
        node,
        destination: destination.to_string(),
        files_done: tick.files_done,
        files_total,
        bytes_done: tick.bytes_done,
        bytes_total,
        totals_known: known,
        current: tick.current.clone(),
    })
}

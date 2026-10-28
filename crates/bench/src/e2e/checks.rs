//! Waiting for progress and comparing what the phone and the copy hold.

use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use socketsweep_gui_contract::{EntryKind, Measure, NodeRef, ScanProgress, View};
use socketsweep_host_core::HostEvent;
use socketsweep_sim::SimFs;
use socketsweep_transport::Transport;
use socketsweep_wire::Kind;

use super::ensure;
use crate::fixture::{self, Expected};

/// The next `scan-progress` that satisfies `pred`.
pub(super) fn wait_progress(
    rx: &Receiver<HostEvent>,
    timeout: Duration,
    what: &str,
    mut pred: impl FnMut(&ScanProgress) -> bool,
) -> Result<ScanProgress, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(HostEvent::ScanProgress(p)) if pred(&p) => return Ok(p),
            Ok(HostEvent::ConnectionLost(l)) => {
                return Err(format!("connection lost waiting for {what}: {}", l.message))
            }
            Ok(_) => {}
            Err(_) => return Err(format!("timed out waiting for {what}")),
        }
    }
}

pub(super) fn row(view: &View, name: &str) -> Result<NodeRef, String> {
    view.rows
        .iter()
        .find(|r| r.name == name && r.kind == EntryKind::Dir)
        .map(|r| r.node)
        .ok_or_else(|| format!("no folder {name:?} in {}", view.path))
}

pub(super) fn check_view(
    view: &View,
    want: Expected,
    apparent: u64,
    what: &str,
) -> Result<(), String> {
    ensure(view.measure == Measure::Final, || {
        format!("{what}: measure {:?}, not final", view.measure)
    })?;
    ensure(
        (view.files, view.dirs, view.size.apparent) == (want.files, want.dirs, apparent),
        || {
            format!(
                "{what}: files/dirs/bytes {}/{}/{} but the phone has {}/{}/{}",
                view.files, view.dirs, view.size.apparent, want.files, want.dirs, apparent
            )
        },
    )?;
    ensure(!view.unreadable, || format!("{what}: unreadable"))
}

/// Compare the copy at `local` with the fixture subtree at `id`, byte for
/// byte, and check nothing else is there.
pub(super) fn compare_copy(fs: &SimFs, seed: u64, id: u32, local: &Path) -> Result<u64, String> {
    let mut checked = 0u64;
    let mut stack = vec![(id, local.to_path_buf())];
    while let Some((x, path)) = stack.pop() {
        let n = fs.node(x);
        match n.kind {
            Kind::Dir => {
                ensure(path.is_dir(), || {
                    format!("{} is not a folder", path.display())
                })?;
                let mut want: Vec<String> = Vec::new();
                for c in fs.children(x) {
                    let name = String::from_utf8_lossy(&fs.node(c).name).into_owned();
                    stack.push((c, path.join(&name)));
                    want.push(name);
                }
                let mut have: Vec<String> = std::fs::read_dir(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?
                    .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect::<Result<_, _>>()
                    .map_err(|e| e.to_string())?;
                want.sort();
                have.sort();
                ensure(want == have, || {
                    format!("{}: entries differ from the fixture", path.display())
                })?;
            }
            _ => {
                let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let rel = fixture::rel_path(fs, x);
                ensure(data == fixture::content(seed, &rel, n.apparent), || {
                    format!("{}: content differs", path.display())
                })?;
                checked += 1;
            }
        }
    }
    Ok(checked)
}

/// The directory's own `st_size`.
pub(super) fn dir_size(transport: &dyn Transport, serial: &str, path: &str) -> Result<u64, String> {
    let out = transport
        .shell(serial, &format!("stat -c %s '{path}'"))
        .map_err(|e| e.to_string())?;
    out.stdout_text()
        .trim()
        .parse()
        .map_err(|_| format!("stat {path}: {:?}", out.stdout_text()))
}

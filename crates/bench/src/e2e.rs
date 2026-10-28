//! The emulator end-to-end test (`docs/architecture.md`, section 5), through
//! host-core's [`App`], the same `Commands` the GUI calls, over real adb and
//! the real daemon:
//!
//! connect → scan → `SkeletonReady` → focus the fixture → its size final and
//! equal to what is on the phone → delete a folder (gone on the phone, totals
//! reduced) → copy a folder and compare it byte for byte → disconnect (the
//! daemon stopped and removed).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use socketsweep_gui_contract::{
    Commands, EntryKind, Measure, NodeRef, ScanOutcome, ScanProgress, View,
};
use socketsweep_host_core::{App, AppConfig, HostEvent};
use socketsweep_sim::SimFs;
use socketsweep_transport::Transport;
use socketsweep_wire::Kind;

use crate::fixture::{self, Expected, FIXTURE_DIR};

/// How to run the end-to-end test.
#[derive(Debug, Clone)]
pub struct Config {
    pub serial: String,
    pub seed: u64,
    /// The storage root, which is also where the fixture lives.
    pub parent: String,
    /// Build and push the fixture first (otherwise it must already be there,
    /// from `fixture` with the same seed).
    pub push: bool,
    pub daemon: Option<PathBuf>,
    /// Scratch space on this computer (the archive, the copy).
    pub work: PathBuf,
    /// Longest any one step may take.
    pub timeout: Duration,
}

fn ensure(cond: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(what())
    }
}

fn cmd<T>(what: &str, r: Result<T, socketsweep_gui_contract::CommandError>) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {:?}: {}", e.code, e.message))
}

/// The next `scan-progress` that satisfies `pred`.
fn wait_progress(
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

fn row(view: &View, name: &str) -> Result<NodeRef, String> {
    view.rows
        .iter()
        .find(|r| r.name == name && r.kind == EntryKind::Dir)
        .map(|r| r.node)
        .ok_or_else(|| format!("no folder {name:?} in {}", view.path))
}

fn check_view(view: &View, want: Expected, apparent: u64, what: &str) -> Result<(), String> {
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
fn compare_copy(fs: &SimFs, seed: u64, id: u32, local: &Path) -> Result<u64, String> {
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

/// Run the test. `log` gets a line per step.
pub fn run(
    transport: Arc<dyn Transport>,
    cfg: &Config,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let root = format!("{}/{FIXTURE_DIR}", cfg.parent);
    std::fs::create_dir_all(&cfg.work).map_err(|e| format!("{}: {e}", cfg.work.display()))?;

    // ── The fixture ──────────────────────────────────────────────────────────
    let fs = if cfg.push {
        let started = Instant::now();
        let (fs, size) = fixture::push(
            transport.as_ref(),
            &cfg.serial,
            cfg.seed,
            &cfg.parent,
            &cfg.work.join("ssfixture.tar"),
        )?;
        log(&format!(
            "fixture: pushed and extracted {} MiB in {:.1} s",
            size >> 20,
            started.elapsed().as_secs_f64()
        ));
        fs
    } else {
        fixture::build(cfg.seed, root.as_bytes())
    };
    let want = fixture::expected(&fs);
    let (files, dirs, apparent, file_bytes) =
        fixture::measure_on_device(transport.as_ref(), &cfg.serial, &root)?;
    ensure(
        (files, dirs, file_bytes) == (want.files, want.dirs, want.file_bytes),
        || {
            format!(
                "the phone's {root} has {files} files, {dirs} dirs, {file_bytes} bytes; \
                 the fixture for seed {} has {want:?} (push it with `fixture --seed {}`)",
                cfg.seed, cfg.seed
            )
        },
    )?;
    log(&format!(
        "fixture on the phone: {files} files, {dirs} dirs, {apparent} bytes"
    ));
    // A view's size counts the folder's own entry as well as everything below
    // it, which `find -mindepth 1` leaves out.
    let apparent = apparent + dir_size(transport.as_ref(), &cfg.serial, &root)?;

    // ── Connect and scan ─────────────────────────────────────────────────────
    let (tx, rx) = mpsc::channel::<HostEvent>();
    let config = AppConfig {
        daemon: crate::device::binaries(cfg.daemon.clone()),
        default_destination: Some(cfg.work.join("copies")),
        ..AppConfig::default()
    };
    let app = App::new(Arc::clone(&transport), config, move |e| {
        let _ = tx.send(e);
    });
    let connected = cmd(
        "connect",
        app.connect(Some(cfg.serial.clone()), Some(cfg.parent.clone())),
    )?;
    log(&format!(
        "connected to {} ({}), root {}",
        connected.model, connected.serial, connected.root
    ));
    let result = scan_and_act(
        &app,
        transport.as_ref(),
        &rx,
        &fs,
        cfg,
        &root,
        want,
        apparent,
        log,
    );
    // Disconnect whatever happened, then check the phone is clean.
    let disconnected = cmd("disconnect", app.disconnect());
    app.shutdown();
    result?;
    disconnected?;
    let alive = transport
        .shell(&cfg.serial, "pidof socketsweep-daemon")
        .map_err(|e| e.to_string())?;
    ensure(alive.stdout_text().trim().is_empty(), || {
        format!("the daemon still runs: pid {}", alive.stdout_text().trim())
    })?;
    let left = transport
        .shell(&cfg.serial, "test -e /data/local/tmp/socketsweep-daemon")
        .map_err(|e| e.to_string())?;
    ensure(!left.success(), || {
        "the daemon binary was left on the phone".into()
    })?;
    log("disconnected: the daemon is stopped and removed");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_and_act(
    app: &App,
    transport: &dyn Transport,
    rx: &Receiver<HostEvent>,
    fs: &SimFs,
    cfg: &Config,
    root: &str,
    want: Expected,
    apparent: u64,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let t = cfg.timeout;
    let started = cmd("start_scan", app.start_scan(None))?;
    log(&format!("scan started: {}", started.path));
    let p = wait_progress(rx, t, "SkeletonReady", |p| {
        p.skeleton.is_some() || p.finished.is_some()
    })?;
    let sk = p
        .skeleton
        .ok_or("the scan finished without SkeletonReady")?;
    ensure(sk.listed >= 2, || {
        format!("skeleton listed only {}", sk.listed)
    })?;
    log(&format!(
        "skeleton ready at {} ms: {} folders listed, {} in the frontier",
        sk.elapsed_ms, sk.listed, sk.frontier
    ));

    // The fixture folder is in the root's listing, which the skeleton covers.
    let top = cmd("get_view", app.get_view(None, Some(1000), None))?;
    let node = row(&top, FIXTURE_DIR)?;
    cmd("focus", app.focus(Some(node), None, None))?;
    let p = wait_progress(rx, t, "the fixture to settle", |p| {
        p.focus.is_some_and(|f| f.node == node && f.settled) || p.finished.is_some()
    })?;
    log(&format!(
        "focus on {FIXTURE_DIR} settled at {} ms ({:?})",
        p.elapsed_ms, p.phase
    ));
    let view = p.view.filter(|v| v.node == node);
    if let Some(v) = &view {
        check_view(v, want, apparent, "the focused fixture (scan-progress)")?;
    }
    let p = wait_progress(rx, t, "the end of the scan", |p| p.finished.is_some())?;
    let fin = p.finished.expect("finished");
    ensure(fin.outcome == ScanOutcome::Complete, || {
        format!("the scan ended {:?}: {:?}", fin.outcome, fin.message)
    })?;
    log(&format!("scan complete in {} ms", fin.elapsed_ms));
    let view = cmd("get_view", app.get_view(Some(node), Some(1000), None))?;
    check_view(&view, want, apparent, "the fixture")?;
    log(&format!(
        "fixture sizes final and exact: {} files, {} dirs, {} bytes",
        view.files, view.dirs, view.size.apparent
    ));

    // ── Delete ───────────────────────────────────────────────────────────────
    let victim = "Download";
    let vpath = format!("{root}/{victim}");
    let vid = fs
        .lookup(vpath.as_bytes())
        .ok_or("no Download in the fixture")?;
    let gone = fixture::expected_below(fs, vid);
    let (_, _, v_apparent, _) = fixture::measure_on_device(transport, &cfg.serial, &vpath)?;
    let v_own = dir_size(transport, &cfg.serial, &vpath)?;
    let deleted = cmd("delete", app.delete(row(&view, victim)?))?;
    ensure(
        deleted.items == u64::from(gone.files + gone.dirs + 1),
        || {
            format!(
                "delete removed {} items, expected {}",
                deleted.items,
                gone.files + gone.dirs + 1
            )
        },
    )?;
    let still = transport
        .shell(&cfg.serial, &format!("test -e '{vpath}'"))
        .map_err(|e| e.to_string())?;
    ensure(!still.success(), || {
        format!("{vpath} is still on the phone")
    })?;
    fs.delete(vpath.as_bytes());
    let after = fixture::expected(fs);
    let view = cmd("get_view", app.get_view(Some(node), Some(1000), None))?;
    check_view(
        &view,
        after,
        apparent - v_apparent - v_own,
        "the fixture after the delete",
    )?;
    log(&format!(
        "deleted {victim}: {} items gone from the phone and the totals",
        deleted.items
    ));

    // ── Copy ─────────────────────────────────────────────────────────────────
    let source = "Music";
    let sid = fs
        .lookup(format!("{root}/{source}").as_bytes())
        .ok_or("no Music in the fixture")?;
    let sw = fixture::expected_below(fs, sid);
    let dest = cfg.work.join("copies");
    let _ = std::fs::remove_dir_all(&dest);
    let copied = cmd(
        "copy_to_desktop",
        app.copy_to_desktop(row(&view, source)?, Some(dest.display().to_string())),
    )?;
    ensure(
        (copied.files, copied.bytes, copied.skipped, copied.renamed)
            == (sw.files, sw.file_bytes, 0, 0),
        || format!("copy result {copied:?}, expected {sw:?}"),
    )?;
    let checked = compare_copy(fs, cfg.seed, sid, Path::new(&copied.destination))?;
    log(&format!(
        "copied {source}: {checked} files identical to the fixture"
    ));
    let _ = std::fs::remove_dir_all(&dest);
    Ok(())
}

/// The directory's own `st_size`.
fn dir_size(transport: &dyn Transport, serial: &str, path: &str) -> Result<u64, String> {
    let out = transport
        .shell(serial, &format!("stat -c %s '{path}'"))
        .map_err(|e| e.to_string())?;
    out.stdout_text()
        .trim()
        .parse()
        .map_err(|_| format!("stat {path}: {:?}", out.stdout_text()))
}

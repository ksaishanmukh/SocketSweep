//! The emulator end-to-end test, through
//! host-core's [`App`], the same `Commands` the GUI calls, over real adb and
//! the real daemon:
//!
//! connect → scan → `SkeletonReady` → focus the fixture → its size final and
//! equal to what is on the phone → delete a folder (gone on the phone, totals
//! reduced) → copy a folder and compare it byte for byte → disconnect (the
//! daemon stopped and removed).

mod checks;
mod scenario;

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use socketsweep_gui_contract::Commands;
use socketsweep_host_core::{App, AppConfig, HostEvent};
use socketsweep_sim::SimFs;
use socketsweep_transport::Transport;

use crate::fixture::{self, Expected, FIXTURE_DIR};
use checks::dir_size;
use scenario::Scenario;

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

/// What the fixture on the phone is expected to be, checked.
struct Fixture {
    fs: SimFs,
    want: Expected,
    /// Apparent bytes of everything below the fixture root, plus the root's
    /// own entry (as a view counts it).
    apparent: u64,
}

/// Push (or rebuild) the fixture and check the phone holds exactly it.
fn prepare_fixture(
    transport: &dyn Transport,
    cfg: &Config,
    root: &str,
    log: &mut dyn FnMut(&str),
) -> Result<Fixture, String> {
    let fs = if cfg.push {
        let started = Instant::now();
        let (fs, size) = fixture::push(
            transport,
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
        fixture::measure_on_device(transport, &cfg.serial, root)?;
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
    let apparent = apparent + dir_size(transport, &cfg.serial, root)?;
    Ok(Fixture { fs, want, apparent })
}

/// The daemon is stopped and its binary removed.
fn check_phone_is_clean(transport: &dyn Transport, serial: &str) -> Result<(), String> {
    let alive = transport
        .shell(serial, "pidof socketsweep-daemon")
        .map_err(|e| e.to_string())?;
    ensure(alive.stdout_text().trim().is_empty(), || {
        format!("the daemon still runs: pid {}", alive.stdout_text().trim())
    })?;
    let left = transport
        .shell(serial, "test -e /data/local/tmp/socketsweep-daemon")
        .map_err(|e| e.to_string())?;
    ensure(!left.success(), || {
        "the daemon binary was left on the phone".into()
    })
}

/// Run the test. `log` gets a line per step.
pub fn run(
    transport: Arc<dyn Transport>,
    cfg: &Config,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let root = format!("{}/{FIXTURE_DIR}", cfg.parent);
    std::fs::create_dir_all(&cfg.work).map_err(|e| format!("{}: {e}", cfg.work.display()))?;

    let fixture = prepare_fixture(transport.as_ref(), cfg, &root, log)?;

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
    let scenario = Scenario {
        app: &app,
        transport: transport.as_ref(),
        rx: &rx,
        fixture: &fixture,
        cfg,
        root: &root,
    };
    let result = scenario.run(log);
    // Disconnect whatever happened, then check the phone is clean.
    let disconnected = cmd("disconnect", app.disconnect());
    app.shutdown();
    result?;
    disconnected?;
    check_phone_is_clean(transport.as_ref(), &cfg.serial)?;
    log("disconnected: the daemon is stopped and removed");
    Ok(())
}

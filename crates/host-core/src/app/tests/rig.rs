//! The app wired to a fake transport and daemon, and sample values.

use super::super::scan::NEXT_SESSION;
use super::super::*;
use crate::apps::helper::HelperOptions;
use crate::client::ClientConfig;
use crate::lock;
use crate::testkit::FakeDaemon;
use socketsweep_gui_contract::{NodeRef, ScanProgress, View};
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_wire::{Entry, Kind, Stat, Totals};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Instant;

pub(super) const RESOLVED: &[u8] = b"/storage/emulated/0";

pub(super) struct Rig {
    pub(super) app: App,
    pub(super) transport: Arc<FakeTransport>,
    pub(super) daemon: FakeDaemon,
    pub(super) events: Mutex<mpsc::Receiver<HostEvent>>,
    pub(super) dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    pub(super) fn new(daemon: FakeDaemon) -> Rig {
        Self::build(daemon, |_, _| {})
    }

    /// A rig whose app bundles the app helper, with a short time limit and
    /// one package per icon run.
    pub(super) fn with_helper(daemon: FakeDaemon) -> Rig {
        Self::build(daemon, |config, dir| {
            let dex = dir.join("socketsweep-helper.dex");
            std::fs::write(
                &dex, b"dex
035",
            )
            .unwrap();
            config.daemon.helper = Some(dex);
            config.helper = HelperOptions {
                timeout: Duration::from_millis(300),
                icon_chunk: 1,
                icon_size: 48,
            };
        })
    }

    /// A rig with `configure` applied to the app's configuration; it is
    /// given the rig's scratch folder.
    pub(super) fn build(
        daemon: FakeDaemon,
        configure: impl FnOnce(&mut AppConfig, &std::path::Path),
    ) -> Rig {
        let dir = std::env::temp_dir().join(format!(
            "socketsweep-app-{}-{}",
            std::process::id(),
            NEXT_SESSION.fetch_add(1000, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("daemon-arm64");
        std::fs::write(&binary, b"\x7fELF").unwrap();

        let transport = Arc::new(FakeTransport::new().with_device(FakeDevice::ready("P1")));
        daemon.install(&transport, "P1");
        let mut config = AppConfig {
            daemon: DaemonBinaries {
                arm64: Some(binary),
                x86_64: None,
                helper: None,
            },
            launch: LaunchOptions {
                startup_deadline: Duration::from_secs(5),
                retry_gap: Duration::from_millis(5),
                client: ClientConfig {
                    ping_interval: Duration::from_millis(50),
                    dead_after: Duration::from_millis(500),
                    handshake_timeout: Duration::from_secs(5),
                    tick: Duration::from_millis(10),
                },
            },
            history_file: Some(dir.join("history.json")),
            default_destination: Some(dir.join("Desktop")),
            progress_interval: Duration::from_millis(20),
            ..AppConfig::default()
        };
        configure(&mut config, &dir);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let t: Arc<dyn Transport> = transport.clone();
        let app = App::new(t, config, move |e| {
            let _ = lock(&tx).send(e);
        });
        Rig {
            app,
            transport,
            daemon,
            events: Mutex::new(rx),
            dir,
        }
    }

    /// Wait for an event matching `pred`.
    pub(super) fn wait_event(&self, mut pred: impl FnMut(&HostEvent) -> bool) -> HostEvent {
        let rx = lock(&self.events);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let e = rx
                .recv_timeout(left)
                .expect("timed out waiting for an event");
            if pred(&e) {
                return e;
            }
        }
    }

    pub(super) fn wait_finished(&self) -> ScanProgress {
        match self.wait_event(|e| matches!(e, HostEvent::ScanProgress(p) if p.finished.is_some())) {
            HostEvent::ScanProgress(p) => p,
            _ => unreachable!(),
        }
    }
}

pub(super) fn stat(apparent: u64) -> Option<Stat> {
    Some(Stat {
        apparent,
        blocks: apparent.div_ceil(512),
        mtime: 0,
    })
}

pub(super) fn entry(name: &str, kind: Kind, size: u64, id: Option<u32>) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind,
        stat: stat(size),
        id,
    }
}

pub(super) fn totals(apparent: u64, files: u32, dirs: u32) -> Totals {
    Totals {
        apparent,
        disk: apparent.div_ceil(512) * 512,
        files,
        dirs,
        ..Totals::default()
    }
}

pub(super) fn row(view: &View, name: &str) -> NodeRef {
    view.rows
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no row {name} in {view:?}"))
        .node
}

//! Running the app helper for a scan, on a thread of its own, and serving
//! the icons it brought back.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Weak};

use base64::Engine;
use socketsweep_gui_contract::{AppsStatus, AppsUpdated, SizeMetric};
use socketsweep_transport::Transport;
use socketsweep_wire::apps::AppInfo;

use super::state::{Conn, Inner, Scan};
use super::HostEvent;
use crate::apps::helper::{self, HelperOptions};
use crate::apps::AppCatalog;
use crate::lock;

/// What the helper has reported on one phone, kept for the connection.
#[derive(Default)]
pub(super) struct AppsCache {
    /// The helper has been pushed to the phone.
    pushed: bool,
    /// The last list of installed apps.
    catalog: Option<AppCatalog>,
    /// PNG bytes per package.
    icons: HashMap<String, Vec<u8>>,
    /// Packages whose icon has been asked for, whether one came back or the
    /// run failed, so no package is asked for twice.
    asked: HashSet<String>,
}

/// What a new scan's tree starts with: the last list from this phone, with
/// the icons already held, or nothing yet.
pub(super) fn initial_catalog(inner: &Inner, conn: &Conn) -> AppCatalog {
    if inner.config.daemon.helper.is_none() {
        return AppCatalog::unavailable();
    }
    let cache = lock(&conn.apps);
    match &cache.catalog {
        Some(catalog) => {
            let mut catalog = catalog.clone();
            catalog.add_icons(cache.icons.keys());
            catalog
        }
        None => AppCatalog::default(),
    }
}

/// The icon of `package` as base64 PNG, if the current phone gave one.
pub(super) fn icon(inner: &Inner, package: &str) -> Option<String> {
    let conn = lock(&inner.conn).clone()?;
    let cache = lock(&conn.apps);
    let png = cache.icons.get(package)?;
    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

/// Ask the phone about its apps for `scan`, without holding anything up.
pub(super) fn spawn(inner: &Arc<Inner>, conn: Arc<Conn>, scan: Arc<Scan>) {
    let (Some(dex), Some(transport)) =
        (inner.config.daemon.helper.clone(), inner.built_transport())
    else {
        return;
    };
    let job = Job {
        inner: Arc::downgrade(inner),
        options: inner.config.helper.clone(),
        conn,
        scan,
        transport,
    };
    let spawned = std::thread::Builder::new()
        .name("socketsweep-apps".into())
        .spawn(move || job.run(&dex));
    if let Err(e) = spawned {
        eprintln!("socketsweep: cannot start the app helper thread: {e}");
    }
}

struct Job {
    inner: Weak<Inner>,
    options: HelperOptions,
    conn: Arc<Conn>,
    scan: Arc<Scan>,
    transport: Arc<dyn Transport>,
}

impl Job {
    fn run(&self, dex: &Path) {
        if !self.current() {
            return;
        }
        if let Err(e) = self.push(dex) {
            eprintln!("socketsweep: cannot push the app helper: {e}");
            self.give_up();
            return;
        }
        match helper::list_apps(&self.transport, &self.conn.daemon.serial, &self.options) {
            Ok(apps) => self.names(apps),
            Err(e) => {
                eprintln!("socketsweep: the app helper gave no app names: {e}");
                self.give_up();
                return;
            }
        }
        self.icons();
    }

    /// This scan is still the current one: a rescan or a disconnect stops
    /// the job, so it never touches a phone it no longer owns.
    fn current(&self) -> bool {
        self.inner.upgrade().is_some_and(|inner| {
            lock(&inner.scan)
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(s, &self.scan))
        })
    }

    fn push(&self, dex: &Path) -> Result<(), String> {
        if lock(&self.conn.apps).pushed {
            return Ok(());
        }
        helper::push_helper(self.transport.as_ref(), &self.conn.daemon.serial, dex)
            .map_err(|e| e.to_string())?;
        lock(&self.conn.apps).pushed = true;
        Ok(())
    }

    /// Package names only: keep an earlier list if there is one.
    fn give_up(&self) {
        {
            let mut tree = lock(&self.scan.tree);
            if tree.apps().status() == AppsStatus::Pending {
                tree.set_apps(AppCatalog::unavailable());
            }
        }
        self.announce();
    }

    fn names(&self, apps: Vec<AppInfo>) {
        let labels = apps.into_iter().map(|a| (a.pkg, a.label)).collect();
        let mut catalog = AppCatalog::ready(labels);
        {
            let mut cache = lock(&self.conn.apps);
            cache.catalog = Some(catalog.clone());
            catalog.add_icons(cache.icons.keys());
        }
        lock(&self.scan.tree).set_apps(catalog);
        self.announce();
    }

    /// Icons for the installed packages that own folders, biggest first, a
    /// chunk per run. A failed run loses only its own chunk.
    fn icons(&self) {
        let wanted: Vec<String> = {
            let tree = lock(&self.scan.tree);
            let cache = lock(&self.conn.apps);
            tree.packages_by_size(SizeMetric::Disk)
                .into_iter()
                .filter(|p| tree.apps().installed(p) && !cache.asked.contains(p))
                .collect()
        };
        for chunk in wanted.chunks(self.options.icon_chunk.max(1)) {
            if !self.current() {
                return;
            }
            lock(&self.conn.apps).asked.extend(chunk.iter().cloned());
            let serial = &self.conn.daemon.serial;
            match helper::fetch_icons(&self.transport, serial, chunk, &self.options) {
                Ok(icons) if icons.is_empty() => {}
                Ok(icons) => {
                    let got: Vec<String> = icons.keys().cloned().collect();
                    lock(&self.conn.apps).icons.extend(icons);
                    lock(&self.scan.tree).add_icons(&got);
                    self.announce();
                }
                Err(e) => eprintln!(
                    "socketsweep: no icons for {} apps from the app helper: {e}",
                    chunk.len()
                ),
            }
        }
    }

    fn announce(&self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let (status, icons) = {
            let tree = lock(&self.scan.tree);
            (tree.apps().status(), tree.apps().icon_count())
        };
        inner.emit(HostEvent::AppsUpdated(AppsUpdated {
            session: self.scan.session,
            status,
            icons,
        }));
    }
}

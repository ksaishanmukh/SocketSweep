//! The steps against a connected app: scan, focus, delete, copy.

use std::path::Path;
use std::sync::mpsc::Receiver;

use socketsweep_gui_contract::{Commands, NodeRef, ScanOutcome, View};
use socketsweep_host_core::{App, HostEvent};
use socketsweep_transport::Transport;

use super::checks::{check_view, compare_copy, dir_size, row, wait_progress};
use super::{cmd, ensure, Config, Fixture};
use crate::fixture::{self, FIXTURE_DIR};

/// Everything the steps share.
pub(super) struct Scenario<'a> {
    pub app: &'a App,
    pub transport: &'a dyn Transport,
    pub rx: &'a Receiver<HostEvent>,
    pub fixture: &'a Fixture,
    pub cfg: &'a Config,
    pub root: &'a str,
}

impl Scenario<'_> {
    pub(super) fn run(&self, log: &mut dyn FnMut(&str)) -> Result<(), String> {
        let (node, view) = self.scan_and_focus(log)?;
        let view = self.delete_folder(node, &view, log)?;
        self.copy_folder(&view, log)
    }

    /// Scan to the end with the fixture focused; its view, final and exact.
    fn scan_and_focus(&self, log: &mut dyn FnMut(&str)) -> Result<(NodeRef, View), String> {
        let (app, rx, t) = (self.app, self.rx, self.cfg.timeout);
        let (want, apparent) = (self.fixture.want, self.fixture.apparent);
        let started = cmd("start_scan", app.start_scan(None))?;
        log(&format!("scan started: {}", started.path));
        let p = wait_progress(rx, t, "SkeletonReady", |p| {
            p.skeleton.is_some() || p.finished.is_some()
        })?;
        // On a fast device one progress event can carry the skeleton, the settled
        // focus and the end of the scan together, so the end is remembered from
        // whichever wait saw it rather than waited for again.
        let mut finished = p.finished.clone();
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
        if finished.is_none() {
            let p = wait_progress(rx, t, "the fixture to settle", |p| {
                p.focus.is_some_and(|f| f.node == node && f.settled) || p.finished.is_some()
            })?;
            log(&format!(
                "focus on {FIXTURE_DIR} settled at {} ms ({:?})",
                p.elapsed_ms, p.phase
            ));
            if let Some(v) = p.view.as_ref().filter(|v| v.node == node) {
                check_view(v, want, apparent, "the focused fixture (scan-progress)")?;
            }
            finished = p.finished;
        } else {
            log("the scan finished before the focus could be sent");
        }
        let fin = match finished {
            Some(fin) => fin,
            None => wait_progress(rx, t, "the end of the scan", |p| p.finished.is_some())?
                .finished
                .expect("finished"),
        };
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
        Ok((node, view))
    }

    /// Delete a folder: gone on the phone, totals reduced. The fixture's new
    /// view.
    fn delete_folder(
        &self,
        node: NodeRef,
        view: &View,
        log: &mut dyn FnMut(&str),
    ) -> Result<View, String> {
        let (fs, cfg) = (&self.fixture.fs, self.cfg);
        let victim = "Download";
        let vpath = format!("{}/{victim}", self.root);
        let vid = fs
            .lookup(vpath.as_bytes())
            .ok_or("no Download in the fixture")?;
        let gone = fixture::expected_below(fs, vid);
        let (_, _, v_apparent, _) =
            fixture::measure_on_device(self.transport, &cfg.serial, &vpath)?;
        let v_own = dir_size(self.transport, &cfg.serial, &vpath)?;
        let deleted = cmd("delete", self.app.delete(row(view, victim)?))?;
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
        let still = self
            .transport
            .shell(&cfg.serial, &format!("test -e '{vpath}'"))
            .map_err(|e| e.to_string())?;
        ensure(!still.success(), || {
            format!("{vpath} is still on the phone")
        })?;
        fs.delete(vpath.as_bytes());
        let after = fixture::expected(fs);
        let view = cmd("get_view", self.app.get_view(Some(node), Some(1000), None))?;
        check_view(
            &view,
            after,
            self.fixture.apparent - v_apparent - v_own,
            "the fixture after the delete",
        )?;
        log(&format!(
            "deleted {victim}: {} items gone from the phone and the totals",
            deleted.items
        ));
        Ok(view)
    }

    /// Copy a folder to this computer and compare it byte for byte.
    fn copy_folder(&self, view: &View, log: &mut dyn FnMut(&str)) -> Result<(), String> {
        let (fs, cfg) = (&self.fixture.fs, self.cfg);
        let source = "Music";
        let sid = fs
            .lookup(format!("{}/{source}", self.root).as_bytes())
            .ok_or("no Music in the fixture")?;
        let sw = fixture::expected_below(fs, sid);
        let dest = cfg.work.join("copies");
        let _ = std::fs::remove_dir_all(&dest);
        let copied = cmd(
            "copy_to_desktop",
            self.app
                .copy_to_desktop(row(view, source)?, Some(dest.display().to_string())),
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
}

//! Mapping a copy manifest onto local paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use socketsweep_wire::{CopyItem, Kind};

use super::names::{host_safe_name, numbered, UsedNames};
use super::{CopyPlan, PlannedItem};
use crate::tree::basename;

/// Map the manifest of `root` onto `dest_dir`. `exists` says whether a local
/// path is taken, so the copy never overwrites what is already there.
pub fn plan(
    root: &[u8],
    items: &[CopyItem],
    dest_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> CopyPlan {
    let mut plan = CopyPlan::default();

    let (top, renamed_top) = top_path(root, dest_dir, exists);
    plan.top = top;

    // Remote relative directory -> its local path; and per local directory,
    // the names used in it.
    let mut dirs: HashMap<Vec<u8>, PathBuf> = HashMap::new();
    dirs.insert(Vec::new(), plan.top.clone());
    let mut used: HashMap<PathBuf, UsedNames> = HashMap::new();

    for item in items {
        let remote = join_remote(root, &item.rel);
        let rel_display = String::from_utf8_lossy(&item.rel).into_owned();
        let (local, renamed) = if item.rel.is_empty() {
            (plan.top.clone(), renamed_top)
        } else {
            local_path(&item.rel, &mut dirs, &mut used)
        };
        if renamed {
            plan.renamed += 1;
        }
        let planned = PlannedItem {
            remote,
            local: local.clone(),
            kind: item.kind,
            size: item.size,
            mtime: item.mtime,
            rel: rel_display,
        };
        match item.kind {
            Kind::Dir => {
                dirs.insert(item.rel.clone(), local);
                plan.dirs.push(planned);
            }
            Kind::File => {
                plan.bytes += item.size;
                plan.files.push(planned);
            }
            Kind::Symlink | Kind::Other => plan.skipped += 1,
        }
    }
    plan.dirs.sort_by_key(|d| d.local.components().count());
    plan
}

/// Where the copy of `root` lands in `dest_dir`, numbered past anything
/// already there, and whether its name changed.
fn top_path(root: &[u8], dest_dir: &Path, exists: &dyn Fn(&Path) -> bool) -> (PathBuf, bool) {
    let (top_name, mut renamed_top) = host_safe_name(basename(root));
    let mut top_name_final = top_name.clone();
    let mut n = 2;
    while exists(&dest_dir.join(&top_name_final)) {
        top_name_final = numbered(&top_name, n);
        renamed_top = true;
        n += 1;
    }
    (dest_dir.join(&top_name_final), renamed_top)
}

pub(super) fn join_remote(root: &[u8], rel: &[u8]) -> Vec<u8> {
    let mut out = root.to_vec();
    if !rel.is_empty() {
        if out.last() != Some(&b'/') {
            out.push(b'/');
        }
        out.extend_from_slice(rel);
    }
    out
}

/// The local path for remote `rel`, mapping each new component.
fn local_path(
    rel: &[u8],
    dirs: &mut HashMap<Vec<u8>, PathBuf>,
    used: &mut HashMap<PathBuf, UsedNames>,
) -> (PathBuf, bool) {
    let (parent_rel, name) = match rel.iter().rposition(|b| *b == b'/') {
        Some(slash) => (&rel[..slash], &rel[slash + 1..]),
        None => (&rel[..0], rel),
    };
    let parent = match dirs.get(parent_rel) {
        Some(p) => p.clone(),
        None => {
            // A directory the manifest did not list before its contents.
            let (p, _) = local_path(parent_rel, dirs, used);
            dirs.insert(parent_rel.to_vec(), p.clone());
            p
        }
    };
    let (safe, changed) = host_safe_name(name);
    let (unique, numbered) = used.entry(parent.clone()).or_default().claim(&safe);
    (parent.join(unique), changed || numbered)
}

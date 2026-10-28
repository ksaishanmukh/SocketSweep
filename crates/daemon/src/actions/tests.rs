#![allow(clippy::too_many_lines)]

use super::*;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use socketsweep_wire::{ErrCode, Kind};

fn tree() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();
    fs::create_dir_all(r.join("a/b/c")).unwrap();
    fs::write(r.join("a/one.bin"), vec![0u8; 1000]).unwrap();
    fs::write(r.join("a/b/two.bin"), vec![0u8; 5000]).unwrap();
    fs::write(r.join("a/b/c/three.bin"), b"xyz").unwrap();
    fs::write(r.join("top.txt"), b"hello").unwrap();
    tmp
}

#[test]
fn delete_reports_items_and_totals_of_what_it_removed() {
    let tmp = tree();
    let target = tmp.path().join("a");
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let (items, totals) = delete_tree(&target, &cancel, &mut |n| seen.push(n)).unwrap();
    assert_eq!(items, 6, "a, b, c and three files");
    assert_eq!(seen, (1..=6).collect::<Vec<_>>());
    assert_eq!(totals.files, 3);
    assert_eq!(totals.dirs, 3);
    assert!(totals.apparent >= 6003);
    assert!(totals.disk >= 6003);
    assert!(!target.exists());
    assert!(tmp.path().join("top.txt").exists());
}

#[test]
fn a_cancelled_delete_stops() {
    let tmp = tree();
    let cancel = AtomicBool::new(true);
    let err = delete_tree(&tmp.path().join("a"), &cancel, &mut |_| {}).unwrap_err();
    assert_eq!(err.code, ErrCode::Cancelled);
    assert!(tmp.path().join("a/b/c/three.bin").exists());
}

#[test]
fn the_manifest_lists_parents_first_with_relative_names() {
    let tmp = tree();
    let cancel = AtomicBool::new(false);
    let mut items = Vec::new();
    manifest_tree(&tmp.path().join("a"), &cancel, &mut |i| items.push(i)).unwrap();
    assert_eq!(items[0].rel, b"");
    assert_eq!(items[0].kind, Kind::Dir);
    let pos = |rel: &[u8]| items.iter().position(|i| i.rel == rel).unwrap();
    assert!(pos(b"b") < pos(b"b/c"));
    assert!(pos(b"b/c") < pos(b"b/c/three.bin"));
    assert_eq!(items[pos(b"b/two.bin")].size, 5000);
    assert_eq!(items[pos(b"b")].size, 0);
    assert_eq!(items.len(), 6);
}

#[cfg(unix)]
#[test]
fn links_are_listed_and_unlinked_never_followed() {
    let tmp = tree();
    let r = tmp.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep.txt"), b"k").unwrap();
    std::os::unix::fs::symlink(outside.path(), r.join("a/escape")).unwrap();

    let cancel = AtomicBool::new(false);
    let mut items = Vec::new();
    manifest_tree(&r.join("a"), &cancel, &mut |i| items.push(i)).unwrap();
    let link = items.iter().find(|i| i.rel == b"escape").unwrap();
    assert_eq!(link.kind, Kind::Symlink);
    assert!(!items.iter().any(|i| i.rel.starts_with(b"escape/")));

    let (items, totals) = delete_tree(&r.join("a"), &cancel, &mut |_| {}).unwrap();
    assert_eq!(items, 7);
    assert_eq!(totals.files, 4, "three files and the link");
    assert!(outside.path().join("keep.txt").exists());
}

#[test]
fn local_actions_confine_requests_to_the_root() {
    let tmp = tree();
    let root = tmp.path().join("a");
    let acts = LocalActions::new(&root);
    let b = |p: PathBuf| path_to_bytes(&p);
    assert!(acts.resolve_scan_root(&b(root.clone())).is_ok());
    assert!(acts.resolve_scan_root(&b(root.join("b"))).is_ok());
    assert_eq!(
        acts.resolve_scan_root(&b(tmp.path().to_path_buf()))
            .unwrap_err()
            .code,
        ErrCode::OutsideRoot
    );
    assert_eq!(
        acts.resolve_delete(&b(root.clone())).unwrap_err().code,
        ErrCode::ProtectedRoot
    );
    assert_eq!(
        acts.resolve_delete(&b(tmp.path().join("top.txt")))
            .unwrap_err()
            .code,
        ErrCode::OutsideRoot
    );
    assert_eq!(
        acts.resolve_delete(&b(root.join("nope"))).unwrap_err().code,
        ErrCode::NotFound
    );
    assert!(acts.resolve_copy(&b(root.clone())).is_ok());
    let resolved = acts.resolve_delete(&b(root.join("b"))).unwrap();
    let scan_root = acts.resolve_scan_root(&b(root.clone())).unwrap();
    let mut scan_path = scan_root.clone();
    scan_path.extend_from_slice(b"/b");
    assert!(acts.same_entry(&scan_path, &resolved));
}

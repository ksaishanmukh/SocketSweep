use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use socketsweep_wire::{CopyItem, Kind};

use super::names::{numbered, UsedNames};
use super::plan::join_remote;
use super::*;
use crate::lock;
use socketsweep_transport::fake::{FakeDevice, FakeOp, FakeTransport};

fn safe(name: &[u8]) -> String {
    host_safe_name(name).0
}

#[test]
fn ordinary_names_are_kept() {
    assert_eq!(
        host_safe_name(b"IMG_0001.jpg"),
        ("IMG_0001.jpg".into(), false)
    );
    assert_eq!(
        host_safe_name("Café ☕.txt".as_bytes()),
        ("Café ☕.txt".into(), false)
    );
    assert_eq!(safe(b".nomedia"), ".nomedia");
}

#[test]
fn invalid_utf8_is_replaced() {
    let (name, changed) = host_safe_name(b"pic\xFF.jpg");
    assert_eq!(name, "pic\u{FFFD}.jpg");
    assert!(changed);
}

#[test]
fn windows_reserved_characters_and_controls_are_replaced() {
    assert_eq!(safe(b"a<b>c:d\"e|f?g*h\\i"), "a_b_c_d_e_f_g_h_i");
    assert_eq!(safe(b"tab\there"), "tab_here");
    assert_eq!(safe(b"what?.mp3"), "what_.mp3");
}

#[test]
fn trailing_dots_and_spaces_are_kept_visible() {
    assert_eq!(safe(b"name. "), "name__");
    assert_eq!(safe(b"."), "_");
    assert_eq!(safe(b".."), "__");
    assert_eq!(safe(b""), "_");
}

#[test]
fn reserved_device_names_are_prefixed() {
    for name in ["CON", "con", "nul.txt", "Com1.log", "LPT9", "aux .x"] {
        let (out, changed) = host_safe_name(name.as_bytes());
        assert!(out.starts_with('_'), "{name} -> {out}");
        assert!(changed);
    }
    assert_eq!(safe(b"CONSOLE.txt"), "CONSOLE.txt");
    assert_eq!(safe(b"COM10"), "COM10");
}

#[test]
fn colliding_names_are_numbered_case_insensitively() {
    let mut used = UsedNames::default();
    assert_eq!(used.claim("a.txt"), ("a.txt".into(), false));
    assert_eq!(used.claim("A.TXT"), ("A (2).TXT".into(), true));
    assert_eq!(used.claim("a.txt"), ("a (3).txt".into(), true));
    assert_eq!(numbered("noext", 2), "noext (2)");
    assert_eq!(numbered(".hidden", 2), ".hidden (2)");
}

fn item(rel: &[u8], kind: Kind, size: u64) -> CopyItem {
    CopyItem {
        rel: rel.to_vec(),
        kind,
        size,
        mode: 0o644,
        mtime: 1_700_000_000,
    }
}

#[test]
fn a_manifest_is_planned_without_overwriting_or_collisions() {
    let items = [
        item(b"", Kind::Dir, 0),
        item(b"sub", Kind::Dir, 0),
        item(b"sub/a?.jpg", Kind::File, 3),
        item(b"sub/a_.jpg", Kind::File, 4),
        item(b"link", Kind::Symlink, 0),
        item(b"x\xFF", Kind::File, 5),
    ];
    let dest = Path::new("/dest");
    let taken = |p: &Path| p == Path::new("/dest/Camera");
    let plan = plan(b"/storage/emulated/0/DCIM/Camera", &items, dest, &taken);
    assert_eq!(plan.top, Path::new("/dest/Camera (2)"));
    assert_eq!(plan.dirs.len(), 2);
    assert_eq!(plan.skipped, 1);
    assert_eq!(plan.bytes, 12);
    let locals: Vec<_> = plan.files.iter().map(|f| f.local.clone()).collect();
    let top = Path::new("/dest/Camera (2)");
    assert_eq!(
        locals,
        [
            top.join("sub").join("a_.jpg"),
            top.join("sub").join("a_ (2).jpg"),
            top.join("x\u{FFFD}"),
        ]
    );
    // The top folder, a?.jpg, a_.jpg (numbered) and x\xFF.
    assert_eq!(plan.renamed, 4);
    assert_eq!(
        plan.files[0].remote,
        b"/storage/emulated/0/DCIM/Camera/sub/a?.jpg"
    );
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "socketsweep-copy-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn files_are_pulled_into_place_with_progress() {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    let root = b"/sdcard/Music";
    let mut items = vec![item(b"", Kind::Dir, 0), item(b"deep", Kind::Dir, 0)];
    for i in 0..7u8 {
        let rel = format!("deep/t{i}.mp3").into_bytes();
        t.put_file(
            "P1",
            &join_remote(root, &rel),
            vec![i; 100 + i as usize],
            0o644,
        );
        items.push(item(&rel, Kind::File, 100 + i as u64));
    }
    let dest = scratch("pull");
    let plan = plan(root, &items, &dest, &|p| p.exists());
    let ticks = Mutex::new(Vec::new());
    let done = execute(&t, "P1", &plan, &AtomicBool::new(false), &|t| {
        lock(&ticks).push(t)
    })
    .unwrap();
    assert_eq!(done.files_done, 7);
    assert_eq!(done.bytes_done, plan.bytes);
    assert_eq!(
        std::fs::read(dest.join("Music").join("deep").join("t3.mp3")).unwrap(),
        vec![3; 103]
    );
    assert!(lock(&ticks).iter().any(|t| t.current.is_some()));
    let _ = std::fs::remove_dir_all(dest);
}

#[test]
fn a_failed_pull_stops_the_copy_and_leaves_no_partial_file() {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    t.fail_next(FakeOp::Pull, TransportError::Refused("gone".into()));
    let dest = scratch("fail");
    let items = [item(b"", Kind::File, 1)];
    let plan = plan(b"/sdcard/a.txt", &items, &dest, &|p| p.exists());
    let err = execute(&t, "P1", &plan, &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(matches!(err, CopyError::Transport(_)));
    assert!(!dest.join("a.txt").exists());
    let _ = std::fs::remove_dir_all(dest);
}

#[test]
fn a_cancelled_copy_takes_no_new_files() {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    t.put_file("P1", b"/sdcard/d/a", vec![1], 0o644);
    let dest = scratch("cancel");
    let items = [item(b"", Kind::Dir, 0), item(b"a", Kind::File, 1)];
    let plan = plan(b"/sdcard/d", &items, &dest, &|p| p.exists());
    let err = execute(&t, "P1", &plan, &AtomicBool::new(true), &|_| {}).unwrap_err();
    assert!(matches!(err, CopyError::Cancelled));
    assert!(!dest.join("d").join("a").exists());
    let _ = std::fs::remove_dir_all(dest);
}

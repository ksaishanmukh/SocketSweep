//! Deletes and copy manifests.

use std::sync::Arc;
use std::time::Duration;

use socketsweep_engine::{Fs, StdFs};
use socketsweep_wire::{Caps, ClientMsg, ErrCode, Kind, Outcome, ScanEvent, ScanOpts, ServerMsg};

use super::support::*;

#[test]
fn a_hinted_delete_prunes_first_and_totals_add_up() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (a_files, a_dirs, _) = truth(&root.join("a"));
    let (files, dirs, _) = truth(&root);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(30)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let scan = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    let mut events = Vec::new();
    let a = loop {
        let f = host.recv();
        if let ServerMsg::Scan(batch) = f.msg {
            events.extend(batch);
            let found = events.iter().find_map(|e| match e {
                ScanEvent::Listing {
                    dir: 0, entries, ..
                } => entries
                    .iter()
                    .find(|en| en.name == b"a")
                    .and_then(|en| en.id),
                _ => None,
            });
            if let Some(a) = found {
                break a;
            }
        }
    };
    let del = host.send(ClientMsg::Delete {
        path: child_path(&root, "a"),
        hint: Some((scan, a)),
    });
    let mut deleted = None;
    events.extend(scan_events(&host, scan, |f| {
        if f.id == del {
            deleted = Some(f.msg);
        }
    }));
    let deleted = deleted.unwrap_or_else(|| host.reply().msg);
    let ServerMsg::Deleted { items, totals } = deleted else {
        panic!("not deleted: {deleted:?}")
    };
    assert_eq!(items, u64::from(a_files + a_dirs + 1));
    assert_eq!(totals.files, a_files);
    assert_eq!(totals.dirs, a_dirs + 1);
    assert!(!root.join("a").exists());

    check_stream(&events);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ScanEvent::Pruned { dir } if *dir == a)),
        "the subtree was pruned"
    );
    let (outcome, scan_totals) = finished(&events);
    assert_eq!(outcome, Outcome::Complete);
    assert_eq!(scan_totals.files, files - a_files);
    assert_eq!(scan_totals.dirs, dirs - a_dirs - 1);
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn deletes_are_guarded() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let cases = [
        (root_bytes(&root), ErrCode::ProtectedRoot),
        (child_path(&root, "a/.."), ErrCode::BadRequest),
        (child_path(&root, "../outside"), ErrCode::NotFound),
        (child_path(&root, "a/../.."), ErrCode::BadRequest),
        (child_path(&root, "nope"), ErrCode::NotFound),
    ];
    for (path, want) in cases {
        let id = host.send(ClientMsg::Delete { path, hint: None });
        let f = host.reply();
        assert_eq!(f.id, id);
        match f.msg {
            ServerMsg::Failed { code, .. } => assert_eq!(code, want),
            other => panic!("{other:?}"),
        }
    }
    std::fs::write(tmp.path().join("outside"), b"x").unwrap();
    let id = host.send(ClientMsg::Delete {
        path: child_path(&root, "../outside"),
        hint: None,
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::OutsideRoot,
            ..
        }
    ));
    assert!(tmp.path().join("outside").exists());

    // A file, with no scan running.
    let id = host.send(ClientMsg::Delete {
        path: child_path(&root, "top"),
        hint: None,
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(
        matches!(f.msg, ServerMsg::Deleted { items: 1, totals } if totals.files == 1 && totals.apparent == 600)
    );
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn a_copy_manifest_lists_the_subtree_in_parts() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let id = host.send(ClientMsg::PrepareCopy {
        path: child_path(&root, "a"),
    });
    let mut items = Vec::new();
    let manifest_root = loop {
        let f = host.reply();
        assert_eq!(f.id, id);
        let ServerMsg::CopyManifest { root, files, last } = f.msg else {
            panic!("{f:?}")
        };
        items.extend(files);
        if last {
            break root;
        }
    };
    let (files, dirs, _) = truth(&root.join("a"));
    assert_eq!(items.len() as u32, files + dirs + 1);
    assert_eq!(items[0].rel, b"");
    assert!(manifest_root.ends_with(b"a"));
    let five = items.iter().find(|i| i.rel == b"w/5").unwrap();
    assert_eq!((five.kind, five.size), (Kind::File, 500));

    let id = host.send(ClientMsg::PrepareCopy {
        path: root_bytes(&root),
    });
    let mut n = 0;
    loop {
        let f = host.reply();
        assert_eq!(f.id, id);
        let ServerMsg::CopyManifest { files, last, .. } = f.msg else {
            panic!("{f:?}")
        };
        n += files.len();
        if last {
            break;
        }
    }
    let (files, dirs, _) = truth(&root);
    assert_eq!(n as u32, files + dirs + 1, "the root itself may be copied");
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn copy_needs_the_capability() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::DISK_USAGE);
    let id = host.send(ClientMsg::PrepareCopy {
        path: child_path(&root, "a"),
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::Unsupported,
            ..
        }
    ));
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

//! A scripted daemon serving a phone with app folders, for the app helper,
//! overview and batch action tests.

use socketsweep_wire::{
    ClientFrame, ClientMsg, CopyItem, ErrCode, FsOp, Kind, Outcome, ScanEvent, ServerMsg, Totals,
};

use super::rig::{entry, totals, Rig, RESOLVED};
use crate::testkit::{FakeDaemon, Responder};

/// `/sdcard` = { Android/data/{com.a/a.bin 300, com.b/b.bin 200,
/// com.c/c.bin 100}, Broken/ (unreadable), big.mp4 5000 }, scanned in one
/// batch. Deleting `com.b` is refused; copying any path lists one 4-byte
/// file `f` inside it.
pub(super) fn android() -> FakeDaemon {
    FakeDaemon::new(|frame: &ClientFrame, out: &Responder| match &frame.msg {
        ClientMsg::StartScan { .. } => {
            out.send(frame.id, ServerMsg::Scan(scan()));
        }
        ClientMsg::Delete { path, .. } if path.ends_with(b"com.b") => {
            out.send(
                frame.id,
                ServerMsg::Failed {
                    code: ErrCode::PermissionDenied,
                    errno: Some(13),
                    message: "Permission denied".into(),
                },
            );
        }
        ClientMsg::Delete { .. } => {
            out.send(
                frame.id,
                ServerMsg::Deleted {
                    items: 2,
                    totals: totals(300, 1, 1),
                },
            );
        }
        ClientMsg::PrepareCopy { path } => {
            let item = |rel: &[u8], kind, size| CopyItem {
                rel: rel.to_vec(),
                kind,
                size,
                mode: 0o644,
                mtime: 0,
            };
            out.send(
                frame.id,
                ServerMsg::CopyManifest {
                    root: path.clone(),
                    files: vec![item(b"", Kind::Dir, 0), item(b"f", Kind::File, 4)],
                    last: true,
                },
            );
        }
        _ => {}
    })
}

fn scan() -> Vec<ScanEvent> {
    let listing = |dir, entries| ScanEvent::Listing {
        dir,
        part: 0,
        last: true,
        entries,
    };
    let done = |dir, totals| ScanEvent::SubtreeDone { dir, totals };
    let unreadable = Totals {
        errors: 1,
        flags: Totals::UNREADABLE,
        ..Totals::default()
    };
    let mut all_done = totals(5600, 4, 6);
    all_done.flags = Totals::UNREADABLE;
    all_done.errors = 1;
    vec![
        ScanEvent::Started {
            root: RESOLVED.to_vec(),
            dev: 1,
            workers: 4,
        },
        listing(
            0,
            vec![
                entry("Android", Kind::Dir, 0, Some(1)),
                entry("Broken", Kind::Dir, 0, Some(6)),
                entry("big.mp4", Kind::File, 5000, None),
            ],
        ),
        listing(1, vec![entry("data", Kind::Dir, 0, Some(2))]),
        listing(
            2,
            vec![
                entry("com.a", Kind::Dir, 0, Some(3)),
                entry("com.b", Kind::Dir, 0, Some(4)),
                entry("com.c", Kind::Dir, 0, Some(5)),
            ],
        ),
        listing(3, vec![entry("a.bin", Kind::File, 300, None)]),
        listing(4, vec![entry("b.bin", Kind::File, 200, None)]),
        listing(5, vec![entry("c.bin", Kind::File, 100, None)]),
        ScanEvent::DirFailed {
            dir: 6,
            op: FsOp::Open,
            errno: 13,
        },
        done(3, totals(300, 1, 0)),
        done(4, totals(200, 1, 0)),
        done(5, totals(100, 1, 0)),
        done(2, totals(600, 3, 3)),
        done(1, totals(600, 3, 4)),
        done(6, unreadable),
        done(0, all_done),
        ScanEvent::Finished {
            outcome: Outcome::Complete,
            totals: all_done,
            elapsed_ms: 5,
        },
    ]
}

/// Connect and run the whole scan.
pub(super) fn scanned(rig: &Rig) {
    rig.app.connect(None, None).unwrap();
    rig.app.start_scan(None).unwrap();
    rig.wait_finished();
}

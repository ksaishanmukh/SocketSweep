//! Samples of every message, and helpers shared by the test files.

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::*;

pub(super) fn stat() -> Stat {
    Stat {
        apparent: 5,
        blocks: 8,
        mtime: -1,
    }
}

pub(super) fn totals() -> Totals {
    Totals {
        apparent: 1,
        disk: 2,
        files: 3,
        dirs: 4,
        errors: 5,
        flags: Totals::UNREADABLE,
        newest_mtime: 100,
    }
}

pub(super) fn welcome() -> Welcome {
    Welcome {
        session: [0x11; 16],
        daemon_build: "v".into(),
    }
}

pub(super) fn client_msgs() -> Vec<ClientMsg> {
    vec![
        ClientMsg::Ping { nonce: 9 },
        ClientMsg::StartScan {
            root: b"/sdcard".to_vec(),
            opts: ScanOpts::default(),
        },
        ClientMsg::Focus {
            scan: 2,
            dir: 3,
            seq: 4,
        },
        ClientMsg::ClearFocus { scan: 2, seq: 5 },
        ClientMsg::Cancel { target: 2 },
        ClientMsg::Delete {
            path: b"/x".to_vec(),
            hint: Some((2, 3)),
        },
        ClientMsg::PrepareCopy {
            path: b"/x".to_vec(),
        },
        ClientMsg::Shutdown,
    ]
}

pub(super) fn scan_events() -> Vec<ScanEvent> {
    let mut events = scan_events_through_subtree_done();
    events.extend(scan_events_after_subtree_done());
    events
}

fn scan_events_through_subtree_done() -> Vec<ScanEvent> {
    vec![
        ScanEvent::Started {
            root: b"/r".to_vec(),
            dev: 66,
            workers: 8,
        },
        ScanEvent::Listing {
            dir: 0,
            part: 0,
            last: true,
            entries: vec![
                Entry {
                    name: b"a".to_vec(),
                    kind: Kind::File,
                    stat: Some(stat()),
                    id: None,
                },
                Entry {
                    name: b"d".to_vec(),
                    kind: Kind::Dir,
                    stat: None,
                    id: Some(1),
                },
            ],
        },
        ScanEvent::Stats {
            dir: 1,
            start: 2,
            stats: vec![Some(stat()), None],
        },
        ScanEvent::DirFailed {
            dir: 1,
            op: FsOp::ReadDir,
            errno: 13,
        },
        ScanEvent::SubtreeDone {
            dir: 1,
            totals: totals(),
        },
    ]
}

fn scan_events_after_subtree_done() -> Vec<ScanEvent> {
    vec![
        ScanEvent::SkeletonReady {
            listed: 1,
            frontier: 2,
            elapsed_ms: 300,
        },
        ScanEvent::FocusState {
            seq: 4,
            dir: 3,
            settled: true,
        },
        ScanEvent::Pruned { dir: 3 },
        ScanEvent::Progress {
            listed: 1,
            known: 2,
        },
        ScanEvent::Finished {
            outcome: Outcome::Cancelled,
            totals: totals(),
            elapsed_ms: 1,
        },
    ]
}

pub(super) fn server_msgs() -> Vec<ServerMsg> {
    vec![
        ServerMsg::Pong { nonce: 9 },
        ServerMsg::Heartbeat,
        ServerMsg::Scan(vec![ScanEvent::Pruned { dir: 3 }]),
        ServerMsg::DeleteProgress { items: 10 },
        ServerMsg::Deleted {
            items: 11,
            totals: totals(),
        },
        ServerMsg::CopyManifest {
            root: b"/c".to_vec(),
            files: vec![CopyItem {
                rel: b"f".to_vec(),
                kind: Kind::File,
                size: 3,
                mode: 0o644,
                mtime: 2,
            }],
            last: true,
        },
        ServerMsg::Failed {
            code: ErrCode::OutsideRoot,
            errno: Some(2),
            message: "no".into(),
        },
    ]
}

pub(super) fn hello_replies() -> Vec<HelloReply> {
    vec![HelloReply::Welcome(welcome()), HelloReply::Refused]
}

pub(super) fn enc<T: Serialize>(v: &T) -> Vec<u8> {
    postcard::to_stdvec(v).unwrap()
}

pub(super) fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(v: &T) {
    let mut buf = Vec::new();
    write_frame(&mut buf, v).unwrap();
    let mut cursor = buf.as_slice();
    let back: T = read_frame(&mut cursor).unwrap().unwrap();
    assert_eq!(&back, v);
    assert!(cursor.is_empty(), "the frame was not read to its end");
}

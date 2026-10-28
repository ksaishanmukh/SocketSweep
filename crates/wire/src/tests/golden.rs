//! The exact bytes of a frame of every message.

use serde::Serialize;

use super::support::*;
use crate::*;

#[track_caller]
fn golden<T: Serialize>(value: &T, expected: &[u8]) {
    let actual = enc(value);
    assert_eq!(
        actual, expected,
        "the encoding changed: bump PROTOCOL_VERSION and update the expected bytes"
    );
}

#[test]
fn golden_hello() {
    let hello = Hello::new(
        [0xA5; TOKEN_LEN],
        [0x11; 16],
        Caps::DISK_USAGE | Caps::COPY_MANIFEST,
    );
    let mut expected = vec![b'S', b'S', b'W', b'P', 4];
    expected.extend([0xA5; TOKEN_LEN]);
    expected.extend([0x11; 16]);
    expected.push(9);
    golden(&hello, &expected);

    // The full frame: the length prefix is little-endian.
    let frame = encode_frame(&hello).unwrap();
    assert_eq!(frame[..4], [54, 0, 0, 0]);
    assert_eq!(frame[4..], expected[..]);
}

#[test]
fn golden_hello_replies() {
    let mut expected = vec![0, 4];
    expected.extend([0x11; 16]);
    expected.extend([9, 1, b'v', 8]);
    // Limits: 16 MiB, 4096, 65536, 64, 16, as varints.
    expected.extend([0x80, 0x80, 0x80, 0x08, 0x80, 0x20, 0x80, 0x80, 0x04, 64, 16]);
    golden(&HelloReply::Welcome(welcome()), &expected);
    golden(&HelloReply::Refused, &[1]);
    golden(&HelloReply::WrongVersion { version: 5 }, &[2, 5]);
}

/// The two `StartScan` frames: default options, and a changed worker count
/// and mode.
fn start_scan_frames() -> Vec<(ClientMsg, &'static [u8])> {
    vec![
        (
            ClientMsg::StartScan {
                root: b"/sdcard".to_vec(),
                opts: ScanOpts::default(),
            },
            &[
                1, 7, b'/', b's', b'd', b'c', b'a', b'r', b'd', // root
                0,    // workers: None
                64,   // max_depth
                0xE8, 0x07, // skeleton_budget_ms: 1000
                2, 4, 1, // min depth, max depth, one_filesystem
                0x00, 0x00, 0x40, 0x40, 0, 255, // open: 3.0, 0, none
                0x00, 0x00, 0x80, 0x3F, 1, 2, // near: 1.0, 1, 2
                0x00, 0x00, 0x80, 0x40, 255, 255, // back: 4.0, auto, none
                0x90, 0x03, // ramp_ms: 400
                0,    // mode: Focus
            ],
        ),
        (
            ClientMsg::StartScan {
                root: b"/r".to_vec(),
                opts: ScanOpts {
                    workers: Some(3),
                    mode: ScanMode::Plain,
                    ..ScanOpts::default()
                },
            },
            &[
                1, 2, b'/', b'r', // root
                1, 3,  // workers: Some(3)
                64, // max_depth
                0xE8, 0x07, // skeleton_budget_ms: 1000
                2, 4, 1, // min depth, max depth, one_filesystem
                0x00, 0x00, 0x40, 0x40, 0, 255, // open: 3.0, 0, none
                0x00, 0x00, 0x80, 0x3F, 1, 2, // near: 1.0, 1, 2
                0x00, 0x00, 0x80, 0x40, 255, 255, // back: 4.0, auto, none
                0x90, 0x03, // ramp_ms: 400
                1,    // mode: Plain
            ],
        ),
    ]
}

#[test]
fn golden_client_frames() {
    let mut frames: Vec<(ClientMsg, &[u8])> = vec![(ClientMsg::Ping { nonce: 9 }, &[0, 9])];
    frames.extend(start_scan_frames());
    let rest: Vec<(ClientMsg, &[u8])> = vec![
        (
            ClientMsg::Focus {
                scan: 2,
                dir: 3,
                seq: 4,
            },
            &[2, 2, 3, 4],
        ),
        (ClientMsg::ClearFocus { scan: 2, seq: 5 }, &[3, 2, 5]),
        (ClientMsg::Cancel { target: 2 }, &[4, 2]),
        (
            ClientMsg::Delete {
                path: b"/x".to_vec(),
                hint: Some((2, 3)),
            },
            &[5, 2, b'/', b'x', 1, 2, 3],
        ),
        (
            ClientMsg::PrepareCopy {
                path: b"/x".to_vec(),
            },
            &[6, 2, b'/', b'x'],
        ),
        (ClientMsg::Shutdown, &[7]),
    ];
    frames.extend(rest);
    for (msg, body) in frames {
        let mut expected = vec![0xAC, 0x02]; // id 300
        expected.extend_from_slice(body);
        golden(&ClientFrame { id: 300, msg }, &expected);
    }
}

#[test]
fn golden_server_frames() {
    const TOTALS: [u8; 6] = [1, 2, 3, 4, 5, 1];
    let mut deleted = vec![4, 11];
    deleted.extend(TOTALS);
    let frames: Vec<(ServerMsg, Vec<u8>)> = vec![
        (ServerMsg::Pong { nonce: 9 }, vec![0, 9]),
        (ServerMsg::Heartbeat, vec![1]),
        (
            ServerMsg::Scan(vec![ScanEvent::Pruned { dir: 3 }]),
            vec![2, 1, 7, 3],
        ),
        (ServerMsg::DeleteProgress { items: 10 }, vec![3, 10]),
        (
            ServerMsg::Deleted {
                items: 11,
                totals: totals(),
            },
            deleted,
        ),
        (
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
            // mode 0o644 = 420 = varint A4 03; mtime 2 zigzags to 4.
            vec![5, 2, b'/', b'c', 1, 1, b'f', 0, 3, 0xA4, 0x03, 4, 1],
        ),
        (
            ServerMsg::Failed {
                code: ErrCode::OutsideRoot,
                errno: Some(2),
                message: "no".into(),
            },
            // errno 2 zigzags to 4.
            vec![6, 6, 1, 4, 2, b'n', b'o'],
        ),
    ];
    for (msg, body) in frames {
        let mut expected = vec![0]; // id 0, session level
        expected.extend(body);
        golden(&ServerFrame { id: 0, msg }, &expected);
    }
}

#[test]
fn golden_scan_events() {
    const TOTALS: [u8; 6] = [1, 2, 3, 4, 5, 1];
    // apparent 5, blocks 8, mtime -1 (zigzag 1).
    const STAT: [u8; 3] = [5, 8, 1];

    let mut listing = vec![1, 0, 0, 1, 2]; // dir 0, part 0, last, 2 entries
    listing.extend([1, b'a', 0, 1]); // "a", File, Some(stat)
    listing.extend(STAT);
    listing.push(0); // id: None
    listing.extend([1, b'd', 1, 0, 1, 1]); // "d", Dir, None, Some(1)

    let mut stats = vec![2, 1, 2, 2, 1];
    stats.extend(STAT);
    stats.push(0);

    let mut subtree = vec![4, 1];
    subtree.extend(TOTALS);

    let mut finished = vec![9, 1];
    finished.extend(TOTALS);
    finished.push(1);

    let expected: Vec<Vec<u8>> = vec![
        vec![0, 2, b'/', b'r', 66, 8],
        listing,
        stats,
        vec![3, 1, 1, 26], // errno 13 zigzags to 26
        subtree,
        vec![5, 1, 2, 0xAC, 0x02],
        vec![6, 4, 3, 1],
        vec![7, 3],
        vec![8, 1, 2, 3, 4, 5, 3, 1, 4, 1, 7, 0xB9, 0x17],
        finished,
    ];
    let events = scan_events();
    assert_eq!(events.len(), expected.len());
    for (event, bytes) in events.iter().zip(&expected) {
        golden(event, bytes);
    }
}

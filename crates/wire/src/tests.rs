//! Round trips, golden bytes, variant indices and framing edge cases.
//!
//! The golden byte tests are the guard on the wire format: they hard-code the
//! exact encoding of a frame of every message, so reordering a field or a
//! variant, or changing a type, fails here. When such a change is intended,
//! bump [`PROTOCOL_VERSION`] and update the expected bytes in the same change.

use std::io::{self, Read};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::*;

// ── Samples: one of every variant ───────────────────────────────────────────

fn stat() -> Stat {
    Stat {
        apparent: 5,
        blocks: 8,
        mtime: -1,
    }
}

fn totals() -> Totals {
    Totals {
        apparent: 1,
        disk: 2,
        files: 3,
        dirs: 4,
        errors: 5,
        flags: Totals::UNREADABLE,
    }
}

fn welcome() -> Welcome {
    Welcome {
        version: PROTOCOL_VERSION,
        session: [0x11; 16],
        caps: Caps::DISK_USAGE | Caps::COPY_MANIFEST,
        daemon_build: "v".into(),
        workers: 8,
        limits: Limits::default(),
    }
}

fn client_msgs() -> Vec<ClientMsg> {
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

/// The variant index each message must encode as. Exhaustive, so a new
/// variant does not compile until it is given a sample and a golden encoding.
fn client_index(m: &ClientMsg) -> u8 {
    match m {
        ClientMsg::Ping { .. } => 0,
        ClientMsg::StartScan { .. } => 1,
        ClientMsg::Focus { .. } => 2,
        ClientMsg::ClearFocus { .. } => 3,
        ClientMsg::Cancel { .. } => 4,
        ClientMsg::Delete { .. } => 5,
        ClientMsg::PrepareCopy { .. } => 6,
        ClientMsg::Shutdown => 7,
    }
}

fn scan_events() -> Vec<ScanEvent> {
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
            files: 3,
            disk: 4,
            pending: 5,
            alloc: [3, 1, 4],
            stalled: vec![(7, 3001)],
        },
        ScanEvent::Finished {
            outcome: Outcome::Cancelled,
            totals: totals(),
            elapsed_ms: 1,
        },
    ]
}

fn scan_index(e: &ScanEvent) -> u8 {
    match e {
        ScanEvent::Started { .. } => 0,
        ScanEvent::Listing { .. } => 1,
        ScanEvent::Stats { .. } => 2,
        ScanEvent::DirFailed { .. } => 3,
        ScanEvent::SubtreeDone { .. } => 4,
        ScanEvent::SkeletonReady { .. } => 5,
        ScanEvent::FocusState { .. } => 6,
        ScanEvent::Pruned { .. } => 7,
        ScanEvent::Progress { .. } => 8,
        ScanEvent::Finished { .. } => 9,
    }
}

fn server_msgs() -> Vec<ServerMsg> {
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

fn server_index(m: &ServerMsg) -> u8 {
    match m {
        ServerMsg::Pong { .. } => 0,
        ServerMsg::Heartbeat => 1,
        ServerMsg::Scan(_) => 2,
        ServerMsg::DeleteProgress { .. } => 3,
        ServerMsg::Deleted { .. } => 4,
        ServerMsg::CopyManifest { .. } => 5,
        ServerMsg::Failed { .. } => 6,
    }
}

fn hello_replies() -> Vec<HelloReply> {
    vec![
        HelloReply::Welcome(welcome()),
        HelloReply::Refused,
        HelloReply::WrongVersion { version: 5 },
    ]
}

fn reply_index(r: &HelloReply) -> u8 {
    match r {
        HelloReply::Welcome(_) => 0,
        HelloReply::Refused => 1,
        HelloReply::WrongVersion { .. } => 2,
    }
}

fn enc<T: Serialize>(v: &T) -> Vec<u8> {
    postcard::to_stdvec(v).unwrap()
}

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(v: &T) {
    let mut buf = Vec::new();
    write_frame(&mut buf, v).unwrap();
    let mut cursor = buf.as_slice();
    let back: T = read_frame(&mut cursor).unwrap().unwrap();
    assert_eq!(&back, v);
    assert!(cursor.is_empty(), "the frame was not read to its end");
}

// ── Round trips ─────────────────────────────────────────────────────────────

#[test]
fn every_client_message_round_trips() {
    for (i, msg) in client_msgs().into_iter().enumerate() {
        round_trip(&ClientFrame {
            id: i as u32 + 1,
            msg,
        });
    }
}

#[test]
fn every_server_message_round_trips() {
    for (i, msg) in server_msgs().into_iter().enumerate() {
        round_trip(&ServerFrame { id: i as u32, msg });
    }
    round_trip(&ServerFrame {
        id: 1,
        msg: ServerMsg::Scan(scan_events()),
    });
}

#[test]
fn every_scan_event_and_outcome_round_trips() {
    for event in scan_events() {
        round_trip(&event);
    }
    for outcome in [
        Outcome::Complete,
        Outcome::Cancelled,
        Outcome::Aborted {
            code: ErrCode::Internal,
        },
    ] {
        round_trip(&outcome);
    }
    for kind in [Kind::File, Kind::Dir, Kind::Symlink, Kind::Other] {
        round_trip(&kind);
    }
    for op in [FsOp::Open, FsOp::ReadDir, FsOp::Stat] {
        round_trip(&op);
    }
}

#[test]
fn handshake_messages_round_trip() {
    round_trip(&Hello::new([0xA5; TOKEN_LEN], [0x11; 16], Caps::all()));
    for reply in hello_replies() {
        round_trip(&reply);
    }
}

#[test]
fn unknown_capability_bits_survive_decoding_and_are_intersected_away() {
    let mut hello = Hello::new([1; TOKEN_LEN], [2; 16], Caps::DISK_USAGE);
    hello.caps = Caps::from_bits_retain(Caps::DISK_USAGE.bits() | 1 << 40);
    let back: Hello = postcard::from_bytes(&enc(&hello)).unwrap();
    assert_eq!(back.caps.bits(), hello.caps.bits());

    let welcome = back.welcome(Caps::all(), "b".into(), 8, Limits::default());
    assert_eq!(welcome.caps, Caps::DISK_USAGE);
    assert_eq!(welcome.session, [2; 16]);
    assert_eq!(welcome.version, PROTOCOL_VERSION);
}

#[test]
fn non_utf8_names_survive() {
    // 0xFF is never valid UTF-8. A String-based protocol would mangle this, and
    // a mangled path is one a delete could resolve wrongly.
    let raw = vec![0xFFu8, 0xFE, b'p', 0x80];
    let event = ScanEvent::Listing {
        dir: 0,
        part: 0,
        last: true,
        entries: vec![Entry {
            name: raw.clone(),
            kind: Kind::File,
            stat: None,
            id: None,
        }],
    };
    round_trip(&event);
    round_trip(&ClientMsg::Delete {
        path: raw,
        hint: None,
    });
}

// ── Variant indices ─────────────────────────────────────────────────────────

/// Every sample encodes as its expected index, and the samples cover every
/// index from 0 up, so none is missing.
fn check_indices<T: Serialize>(samples: &[T], index: impl Fn(&T) -> u8) {
    let mut seen: Vec<u8> = samples
        .iter()
        .map(|s| {
            let bytes = enc(s);
            assert_eq!(bytes[0], index(s), "variant index moved");
            bytes[0]
        })
        .collect();
    seen.sort_unstable();
    seen.dedup();
    let expected: Vec<u8> = (0..seen.len() as u8).collect();
    assert_eq!(seen, expected, "a variant has no sample");
}

#[test]
fn variant_indices_are_stable() {
    check_indices(&client_msgs(), client_index);
    check_indices(&server_msgs(), server_index);
    check_indices(&scan_events(), scan_index);
    check_indices(&hello_replies(), reply_index);

    assert_eq!(enc(&Kind::File), [0]);
    assert_eq!(enc(&Kind::Dir), [1]);
    assert_eq!(enc(&Kind::Symlink), [2]);
    assert_eq!(enc(&Kind::Other), [3]);
    assert_eq!(enc(&FsOp::Open), [0]);
    assert_eq!(enc(&FsOp::ReadDir), [1]);
    assert_eq!(enc(&FsOp::Stat), [2]);
    assert_eq!(enc(&Outcome::Complete), [0]);
    assert_eq!(enc(&Outcome::Cancelled), [1]);
    assert_eq!(
        enc(&Outcome::Aborted {
            code: ErrCode::BadRequest
        }),
        [2, 0]
    );
    let codes = [
        ErrCode::BadRequest,
        ErrCode::DuplicateId,
        ErrCode::Busy,
        ErrCode::Unsupported,
        ErrCode::NotFound,
        ErrCode::PermissionDenied,
        ErrCode::OutsideRoot,
        ErrCode::ProtectedRoot,
        ErrCode::Cancelled,
        ErrCode::Io,
        ErrCode::Internal,
    ];
    for (i, code) in codes.iter().enumerate() {
        assert_eq!(enc(code), [i as u8], "{code:?}");
    }
}

#[test]
fn capability_bits_are_stable() {
    assert_eq!(Caps::DISK_USAGE.bits(), 1);
    assert_eq!(Caps::LISTING_PARTS.bits(), 2);
    assert_eq!(Caps::DEFERRED_STAT.bits(), 4);
    assert_eq!(Caps::COPY_MANIFEST.bits(), 8);
    assert_eq!(Caps::DIR_MTIME.bits(), 16);
    assert_eq!(Caps::RESUME.bits(), 32);
    // Encoded as the bits, a varint, not as flag names.
    assert_eq!(enc(&(Caps::DISK_USAGE | Caps::RESUME)), [33]);
}

// ── Golden bytes ────────────────────────────────────────────────────────────

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

#[test]
fn golden_client_frames() {
    let frames: Vec<(ClientMsg, &[u8])> = vec![
        (ClientMsg::Ping { nonce: 9 }, &[0, 9]),
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

// ── Handshake ───────────────────────────────────────────────────────────────

const TOKEN: Token = [7; TOKEN_LEN];

#[test]
fn a_hello_prefix_decodes_from_any_hello() {
    let body = enc(&Hello::new(TOKEN, [3; 16], Caps::all()));
    let (prefix, rest) = postcard::take_from_bytes::<HelloPrefix>(&body).unwrap();
    assert_eq!(
        prefix,
        HelloPrefix {
            magic: MAGIC,
            version: PROTOCOL_VERSION
        }
    );
    assert!(!rest.is_empty());

    // A future Hello whose fields after the frozen ones are entirely different.
    #[derive(Serialize)]
    struct FutureHello {
        magic: [u8; 4],
        version: u16,
        token: Token,
        something_new: Vec<String>,
    }
    let future = enc(&FutureHello {
        magic: MAGIC,
        version: 900,
        token: TOKEN,
        something_new: vec!["x".into()],
    });
    assert_eq!(
        HelloPrefix::peek(&future),
        Some(HelloPrefix {
            magic: MAGIC,
            version: 900
        })
    );
    assert_eq!(
        check_hello(&future, &TOKEN),
        HelloCheck::WrongVersion { host_version: 900 }
    );
    // The version is only revealed once the token matched.
    assert_eq!(check_hello(&future, &[8; TOKEN_LEN]), HelloCheck::Refused);
}

#[test]
fn hellos_are_checked_token_then_version() {
    let good = Hello::new(TOKEN, [3; 16], Caps::DISK_USAGE);
    assert_eq!(
        check_hello(&enc(&good), &TOKEN),
        HelloCheck::Accepted(good.clone())
    );
    assert_eq!(check_hello(&enc(&good), &TOKEN).rejection(), None);

    let mut wrong_token = good.clone();
    wrong_token.token[TOKEN_LEN - 1] ^= 1;
    let check = check_hello(&enc(&wrong_token), &TOKEN);
    assert_eq!(check, HelloCheck::Refused);
    assert_eq!(check.rejection(), Some(HelloReply::Refused));

    // Wrong token and wrong version: the token is checked first.
    let mut both = wrong_token.clone();
    both.version = 3;
    assert_eq!(check_hello(&enc(&both), &TOKEN), HelloCheck::Refused);

    let mut old = good.clone();
    old.version = 3;
    let check = check_hello(&enc(&old), &TOKEN);
    assert_eq!(check, HelloCheck::WrongVersion { host_version: 3 });
    assert_eq!(
        check.rejection(),
        Some(HelloReply::WrongVersion {
            version: PROTOCOL_VERSION
        })
    );

    let mut bad_magic = good.clone();
    bad_magic.magic = *b"HTTP";
    assert_eq!(check_hello(&enc(&bad_magic), &TOKEN), HelloCheck::Refused);
}

#[test]
fn garbage_and_trailing_bytes_are_refused() {
    assert_eq!(check_hello(&[], &TOKEN), HelloCheck::Refused);
    assert_eq!(
        check_hello(b"GET / HTTP/1.1\r\n", &TOKEN),
        HelloCheck::Refused
    );

    let mut body = enc(&Hello::new(TOKEN, [3; 16], Caps::empty()));
    body.push(0);
    assert_eq!(check_hello(&body, &TOKEN), HelloCheck::Refused);
}

// ── Tokens ──────────────────────────────────────────────────────────────────

#[test]
fn tokens_round_trip_through_hex() {
    let token: Token = std::array::from_fn(|i| (i * 37) as u8);
    let hex = token_to_hex(&token);
    assert_eq!(hex.len(), TOKEN_LEN * 2);
    assert_eq!(token_from_hex(&hex), Some(token));
    assert_eq!(token_from_hex(&hex.to_uppercase()), Some(token));
}

#[test]
fn malformed_hex_tokens_are_rejected() {
    let good = "ab".repeat(TOKEN_LEN);
    assert!(token_from_hex(&good).is_some());
    assert_eq!(token_from_hex(""), None);
    assert_eq!(token_from_hex(&good[2..]), None, "too short");
    assert_eq!(token_from_hex(&format!("{good}ab")), None, "too long");
    assert_eq!(token_from_hex(&format!(" {}", &good[1..])), None);
    assert_eq!(token_from_hex(&"zz".repeat(TOKEN_LEN)), None);
}

#[test]
fn token_comparison_is_exact() {
    let a: Token = [7; TOKEN_LEN];
    assert!(tokens_match(&a, &a.clone()));

    let mut last = a;
    last[TOKEN_LEN - 1] ^= 1;
    assert!(!tokens_match(&a, &last));

    let mut first = a;
    first[0] ^= 0x80;
    assert!(!tokens_match(&a, &first));
}

// ── Framing ─────────────────────────────────────────────────────────────────

fn sample_frame() -> ServerFrame {
    ServerFrame {
        id: 1,
        msg: ServerMsg::Scan(scan_events()),
    }
}

#[test]
fn many_frames_stream_back_in_order() {
    let mut buf = Vec::new();
    for i in 0..100u64 {
        write_frame(
            &mut buf,
            &ServerFrame {
                id: 1,
                msg: ServerMsg::DeleteProgress { items: i },
            },
        )
        .unwrap();
    }

    let mut cursor = buf.as_slice();
    for i in 0..100u64 {
        let f: ServerFrame = read_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(f.msg, ServerMsg::DeleteProgress { items: i });
    }
    // Clean end of stream, not an error.
    assert!(read_frame::<_, ServerFrame>(&mut cursor).unwrap().is_none());
}

#[test]
fn an_empty_stream_is_a_clean_end() {
    assert!(read_frame::<_, ServerFrame>(&mut &[][..])
        .unwrap()
        .is_none());
    assert!(read_frame_body(&mut &[][..]).unwrap().is_none());
}

#[test]
fn a_stream_ending_inside_the_length_prefix_is_malformed() {
    let full = encode_frame(&sample_frame()).unwrap();
    for cut in 1..4 {
        let err = read_frame::<_, ServerFrame>(&mut &full[..cut]).unwrap_err();
        assert!(
            matches!(err, WireError::Malformed(_)),
            "{cut} prefix bytes: got {err:?}"
        );
    }
}

#[test]
fn a_truncated_body_is_malformed() {
    let full = encode_frame(&sample_frame()).unwrap();
    for cut in [4, 5, full.len() - 1] {
        let err = read_frame::<_, ServerFrame>(&mut &full[..cut]).unwrap_err();
        assert!(
            matches!(err, WireError::Malformed(_)),
            "cut at {cut}: {err:?}"
        );
    }
}

#[test]
fn an_oversized_length_prefix_is_rejected_without_allocating() {
    let mut buf = (MAX_FRAME_BYTES + 1).to_le_bytes().to_vec();
    buf.extend_from_slice(b"whatever");
    let err = read_frame::<_, ServerFrame>(&mut buf.as_slice()).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");

    let err = read_frame_body(&mut &u32::MAX.to_le_bytes()[..]).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");
}

#[test]
fn a_frame_of_exactly_the_maximum_is_accepted() {
    let body = vec![0u8; MAX_FRAME_BYTES as usize];
    let mut buf = MAX_FRAME_BYTES.to_le_bytes().to_vec();
    buf.extend_from_slice(&body);
    assert_eq!(
        read_frame_body(&mut buf.as_slice())
            .unwrap()
            .map(|b| b.len()),
        Some(MAX_FRAME_BYTES as usize)
    );
}

#[test]
fn an_oversized_message_is_refused_on_write() {
    let huge = ServerMsg::Failed {
        code: ErrCode::Internal,
        errno: None,
        message: "x".repeat(MAX_FRAME_BYTES as usize),
    };
    let mut buf = Vec::new();
    let err = write_frame(&mut buf, &huge).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");
    assert!(buf.is_empty(), "nothing is written for a refused frame");
}

#[test]
fn trailing_bytes_in_a_body_are_malformed() {
    let mut body = enc(&ServerFrame {
        id: 0,
        msg: ServerMsg::Heartbeat,
    });
    body.push(0);
    let mut buf = (body.len() as u32).to_le_bytes().to_vec();
    buf.extend(body);
    let err = read_frame::<_, ServerFrame>(&mut buf.as_slice()).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");
}

#[test]
fn an_undecodable_body_is_a_decode_error() {
    // Variant 200 of ServerMsg does not exist.
    let buf = [2, 0, 0, 0, 0, 200];
    let err = read_frame::<_, ServerFrame>(&mut &buf[..]).unwrap_err();
    assert!(matches!(err, WireError::Decode(_)), "got {err:?}");
}

/// Hands back one byte per call, as a socket is entitled to.
struct Trickle<'a>(&'a [u8]);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match (self.0.split_first(), buf.first_mut()) {
            (Some((b, rest)), Some(slot)) => {
                *slot = *b;
                self.0 = rest;
                Ok(1)
            }
            _ => Ok(0),
        }
    }
}

#[test]
fn a_frame_split_across_reads_is_reassembled() {
    let buf = encode_frame(&sample_frame()).unwrap();
    let back: ServerFrame = read_frame(&mut Trickle(&buf)).unwrap().unwrap();
    assert_eq!(back, sample_frame());
}

/// Fails with `Interrupted` before every byte, which readers must retry.
struct Interrupting<'a> {
    data: &'a [u8],
    interrupt: bool,
}

impl Read for Interrupting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.interrupt = !self.interrupt;
        if self.interrupt {
            return Err(io::ErrorKind::Interrupted.into());
        }
        Trickle(self.data)
            .read(buf)
            .inspect(|&n| self.data = &self.data[n..])
    }
}

#[test]
fn interrupted_reads_are_retried() {
    let buf = encode_frame(&sample_frame()).unwrap();
    let mut r = Interrupting {
        data: &buf,
        interrupt: false,
    };
    let back: ServerFrame = read_frame(&mut r).unwrap().unwrap();
    assert_eq!(back, sample_frame());
}

#[test]
fn other_read_errors_are_io_errors() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::ConnectionReset.into())
        }
    }
    let err = read_frame::<_, ServerFrame>(&mut Broken).unwrap_err();
    assert!(matches!(err, WireError::Io(_)), "got {err:?}");
}

// ── Constants ───────────────────────────────────────────────────────────────

#[test]
fn constants_match_the_specification() {
    assert_eq!(PROTOCOL_VERSION, 4);
    assert_eq!(MAX_FRAME_BYTES, 16 << 20);
    assert_eq!(MAX_LISTING_PART, 4096);
    assert_eq!(MAX_SCAN_BATCH_BYTES, 64 << 10);
    assert_eq!(BULK_HIGH_WATER, 4 << 20);
    assert_eq!(BULK_LOW_WATER, 2 << 20);
    assert_eq!(HEARTBEAT_INTERVAL.as_secs(), 2);
    assert_eq!(HOST_PING_INTERVAL.as_secs(), 5);
    assert_eq!(HOST_DEAD_AFTER.as_secs(), 15);
    assert_eq!(DAEMON_DEAD_AFTER.as_secs(), 30);
    assert_eq!(STALL_THRESHOLD.as_secs(), 3);
    // The liveness timeouts are ordered so a busy peer is never mistaken for a
    // dead one.
    assert!(HEARTBEAT_INTERVAL * 3 < HOST_DEAD_AFTER);
    assert!(HOST_PING_INTERVAL * 3 <= HOST_DEAD_AFTER);
    assert!(HOST_PING_INTERVAL * 3 < DAEMON_DEAD_AFTER);
}

#[test]
fn default_scan_options_match_the_specification() {
    let opts = ScanOpts::default();
    assert_eq!(opts.workers, None);
    assert_eq!(opts.skeleton_budget_ms, 1000);
    assert_eq!(opts.skeleton_min_depth, 2);
    assert_eq!(opts.skeleton_max_depth, 4);
    assert_eq!(opts.ramp_ms, 400);
    assert_eq!(opts.mode, ScanMode::Focus);

    let [open, near, back] = opts.tiers;
    assert_eq!(
        (open.weight, open.floor, open.cap),
        (3.0, 0, TierCfg::NO_CAP)
    );
    assert_eq!((near.weight, near.floor, near.cap), (1.0, 1, 2));
    assert_eq!(back.weight, 4.0);
    assert_eq!(back.cap, TierCfg::NO_CAP);
    // max(1, W / 4)
    assert_eq!(back.floor_for(8), 2);
    assert_eq!(back.floor_for(4), 1);
    assert_eq!(back.floor_for(2), 1);
    assert_eq!(near.floor_for(8), 1);
    assert_eq!(Tier::Back.index(), 2);
}

#[test]
fn totals_add_and_subtract() {
    let mut t = Totals::default();
    t.add(&totals());
    t.add(&totals());
    assert_eq!((t.apparent, t.files, t.flags), (2, 6, Totals::UNREADABLE));
    t.subtract(&totals());
    assert_eq!(t.apparent, 1);
    assert_eq!(t.flags, Totals::UNREADABLE, "flags are history, not sums");
    assert_eq!(stat().disk(), 8 * BLOCK_SIZE);
}

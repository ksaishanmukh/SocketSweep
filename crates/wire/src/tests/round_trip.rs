//! Every message survives encode and decode.

use super::support::*;
use crate::*;

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

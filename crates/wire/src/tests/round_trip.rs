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
    round_trip(&Hello::new([0xA5; TOKEN_LEN], [0x11; 16]));
    for reply in hello_replies() {
        round_trip(&reply);
    }
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

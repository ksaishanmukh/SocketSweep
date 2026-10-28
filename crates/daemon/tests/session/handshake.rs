//! The handshake.

use std::io::Write;
use std::sync::Arc;

use socketsweep_daemon::session::{End, HandshakeError};
use socketsweep_engine::StdFs;
use socketsweep_wire::{
    encode_frame, read_frame, Caps, ClientFrame, ClientMsg, HelloReply, PROTOCOL_VERSION,
};

use super::support::*;

#[test]
fn a_good_hello_is_welcomed_with_negotiated_caps() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root);
    let host_caps = Caps::DISK_USAGE | Caps::COPY_MANIFEST | Caps::RESUME;
    end.write_all(&encode_frame(&hello(TOKEN, host_caps)).unwrap())
        .unwrap();
    let HelloReply::Welcome(w) = read_frame(&mut end).unwrap().unwrap() else {
        panic!("not welcomed")
    };
    assert_eq!(w.version, PROTOCOL_VERSION);
    assert_eq!(w.session, [9; 16]);
    assert_eq!(w.caps, Caps::DISK_USAGE | Caps::COPY_MANIFEST);
    assert_eq!(w.workers, 4);
    assert_eq!(w.limits, socketsweep_wire::Limits::default());
    end.write_all(
        &encode_frame(&ClientFrame {
            id: 1,
            msg: ClientMsg::Shutdown,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(t.join().unwrap().unwrap(), End::Shutdown);
}

#[test]
fn a_wrong_token_is_refused_and_reveals_nothing() {
    let tmp = fixture();
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root_of(&tmp));
    // Wrong token *and* wrong version: the token is checked first.
    let mut h = hello([8; 32], Caps::all());
    h.version = 3;
    end.write_all(&encode_frame(&h).unwrap()).unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert_eq!(reply, HelloReply::Refused);
    assert!(matches!(t.join().unwrap(), Err(HandshakeError::Refused)));
    assert!(read_frame::<_, HelloReply>(&mut end).unwrap().is_none());
}

#[test]
fn a_right_token_of_another_version_is_told_ours() {
    let tmp = fixture();
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root_of(&tmp));
    let mut h = hello(TOKEN, Caps::all());
    h.version = 3;
    end.write_all(&encode_frame(&h).unwrap()).unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert_eq!(
        reply,
        HelloReply::WrongVersion {
            version: PROTOCOL_VERSION
        }
    );
    assert!(matches!(
        t.join().unwrap(),
        Err(HandshakeError::WrongVersion { host_version: 3 })
    ));
}

#[test]
fn garbage_instead_of_a_hello_is_refused() {
    let tmp = fixture();
    let (t, mut end) = start_with(config(), Arc::new(StdFs), &root_of(&tmp));
    end.write_all(&encode_frame(&ClientMsg::Shutdown).unwrap())
        .unwrap();
    let reply: HelloReply = read_frame(&mut end).unwrap().unwrap();
    assert_eq!(reply, HelloReply::Refused);
    assert!(t.join().unwrap().is_err());
}

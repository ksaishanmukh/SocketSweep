//! The handshake as the client sees it.

use super::super::*;
use super::support::*;
use crate::testkit::Greeting;
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_transport::Transport;
use socketsweep_wire::{Caps, TOKEN_LEN};

#[test]
fn the_handshake_is_accepted_with_the_right_token() {
    let client = connect(&idle());
    assert_eq!(client.welcome().workers, 4);
    assert_eq!(client.welcome().session, SESSION);
    assert!(client.ping(Duration::from_secs(5)).is_ok());
    client.close();
}

#[test]
fn a_wrong_token_is_refused_and_not_retryable() {
    let err = connect_with(&idle(), [9; TOKEN_LEN], quick())
        .err()
        .unwrap();
    assert_eq!(err, ClientError::Refused);
    assert!(!err.retryable());

    let err = connect_with(&idle().with_greeting(Greeting::Refuse), TOKEN, quick())
        .err()
        .unwrap();
    assert_eq!(err, ClientError::Refused);
}

#[test]
fn a_daemon_of_another_version_is_reported() {
    let err = connect_with(
        &idle().with_greeting(Greeting::WrongVersion(3)),
        TOKEN,
        quick(),
    )
    .err()
    .unwrap();
    assert_eq!(err, ClientError::WrongVersion { daemon: 3 });
}

#[test]
fn a_tunnel_nobody_listens_on_is_retryable() {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    let stream = t.open_tunnel("P1", "nobody").unwrap();
    let err = Client::connect(stream, TOKEN, SESSION, Caps::all(), quick())
        .err()
        .unwrap();
    assert!(err.retryable(), "{err:?}");
}

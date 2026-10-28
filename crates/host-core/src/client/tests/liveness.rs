//! Pings, dead daemons and closing.

use super::super::*;
use super::support::*;
use crate::testkit::{wait_for, FakeDaemon, Greeting};
use socketsweep_wire::ClientMsg;
use std::sync::mpsc;

#[test]
fn a_silent_daemon_is_declared_dead_and_waiters_are_released() {
    let daemon = idle().with_greeting(Greeting::AcceptThenSilent);
    let client = connect(&daemon);
    let (tx, rx) = mpsc::channel();
    client.set_on_close(Box::new(move |why| tx.send(why).unwrap()));

    // A delete the daemon never answers is released when the peer is dead.
    let c = client.clone();
    let waiter = std::thread::spawn(move || c.delete(b"/sdcard/x".to_vec(), None, &mut |_| {}));

    let why = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(why.contains("not answered"), "{why}");
    assert!(matches!(
        waiter.join().unwrap(),
        Err(ClientError::Closed(_))
    ));
    assert!(!client.is_open());
    // It pinged while idle before giving up.
    assert!(lock(&daemon.received)
        .iter()
        .any(|f| matches!(f.msg, ClientMsg::Ping { .. })));
    assert!(matches!(
        client.ping(Duration::from_secs(1)),
        Err(ClientError::Closed(_))
    ));
}

#[test]
fn a_daemon_that_answers_pings_stays_alive() {
    let client = connect(&idle());
    std::thread::sleep(Duration::from_millis(1000));
    assert!(client.is_open(), "{:?}", client.closed_reason());
    client.close();
}

#[test]
fn a_deliberate_close_does_not_report_a_lost_connection() {
    let client = connect(&idle());
    let (tx, rx) = mpsc::channel::<String>();
    client.set_on_close(Box::new(move |why| tx.send(why).unwrap()));
    client.close();
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    assert!(!client.is_open());
}

#[test]
fn the_phone_hanging_up_is_reported_once() {
    let daemon = FakeDaemon::new(|frame, _| {
        if matches!(frame.msg, ClientMsg::PrepareCopy { .. }) {
            panic!("simulated daemon crash");
        }
    });
    let client = connect(&daemon);
    let (tx, rx) = mpsc::channel();
    client.set_on_close(Box::new(move |why| tx.send(why).unwrap()));
    let (_, replies) = client.prepare_copy(b"/sdcard/x".to_vec()).unwrap();
    assert!(replies.recv_timeout(Duration::from_secs(5)).is_err());
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(wait_for(Duration::from_secs(1), || !client.is_open()));
    // Set after the fact: called at once.
    let (tx2, rx2) = mpsc::channel();
    client.set_on_close(Box::new(move |why| tx2.send(why).unwrap()));
    rx2.recv_timeout(Duration::from_secs(1)).unwrap();
}

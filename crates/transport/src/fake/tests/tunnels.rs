//! Tunnels and the pipes behind them.

use super::super::*;
use super::support::*;
use crate::{Duplex, Transport};
use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn tunnels_reach_the_registered_handler() {
    let t = phone();
    let (closed_tx, closed_rx) = mpsc::channel();
    t.on_tunnel("P1", "socketsweep", move |mut daemon| {
        // An echo server that reports when the host hangs up.
        let mut buf = [0u8; 64];
        loop {
            match daemon.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => daemon.write_all(&buf[..n]).unwrap(),
                Err(e) => panic!("{e}"),
            }
        }
        closed_tx.send(()).unwrap();
    });

    let mut tunnel = t.open_tunnel("P1", "socketsweep").unwrap();
    tunnel.write_all(b"ping").unwrap();
    let mut buf = [0u8; 4];
    tunnel.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
    assert_eq!(t.tunnels_opened("P1", "socketsweep"), 1);

    drop(tunnel);
    closed_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the handler sees end of stream once the host end is dropped");
}

#[test]
fn a_tunnel_with_no_listener_reads_end_of_stream() {
    let t = phone();
    let mut tunnel = t.open_tunnel("P1", "nobody").unwrap();
    let mut buf = [0u8; 1];
    assert_eq!(tunnel.read(&mut buf).unwrap(), 0);
    assert_eq!(
        tunnel.write(b"x").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn pipe_reads_time_out() {
    let (a, _b) = pipe();
    a.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let mut a = a;
    let started = Instant::now();
    let err = a.read(&mut [0u8; 1]).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() >= Duration::from_millis(20));
}

#[test]
fn pipe_writes_block_at_capacity_until_read() {
    let (mut a, mut b) = pipe_with_capacity(4);
    assert_eq!(
        a.write(b"abcdef").unwrap(),
        4,
        "a partial write up to capacity"
    );

    a.set_write_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    assert_eq!(a.write(b"x").unwrap_err().kind(), io::ErrorKind::TimedOut);
    a.set_write_timeout(None).unwrap();

    let writer = std::thread::spawn(move || {
        a.write_all(b"efgh").unwrap();
        a
    });
    let mut got = [0u8; 8];
    b.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"abcdefgh");
    writer.join().unwrap();
}

#[test]
fn data_sent_before_close_is_still_delivered() {
    let (mut a, mut b) = pipe();
    a.write_all(b"last words").unwrap();
    drop(a);
    let mut s = String::new();
    b.read_to_string(&mut s).unwrap();
    assert_eq!(s, "last words");
}

#[test]
fn shutdown_through_a_clone_wakes_a_blocked_reader() {
    let (a, mut b) = pipe();
    let mut reader = a.try_clone().unwrap();
    let blocked = std::thread::spawn(move || reader.read(&mut [0u8; 1]).unwrap());
    std::thread::sleep(Duration::from_millis(20));
    a.shutdown().unwrap();
    assert_eq!(blocked.join().unwrap(), 0);
    // The peer sees end of stream too, and cannot write any more.
    assert_eq!(b.read(&mut [0u8; 1]).unwrap(), 0);
    assert_eq!(b.write(b"x").unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn a_pipe_end_stays_open_while_any_clone_lives() {
    let (a, mut b) = pipe();
    let mut clone = a.try_clone().unwrap();
    drop(a);
    clone.write_all(b"x").unwrap();
    let mut buf = [0u8; 1];
    b.read_exact(&mut buf).unwrap();
    drop(clone);
    assert_eq!(b.read(&mut buf).unwrap(), 0);
}

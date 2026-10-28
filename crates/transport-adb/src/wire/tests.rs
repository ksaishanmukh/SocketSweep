#![allow(clippy::too_many_lines)]

use super::sync::SYNC_CHUNK;
use super::*;
use std::io::{Read, Write};

#[test]
fn forward_tcp0_reply_yields_the_port() {
    let mut reply: &[u8] = b"OKAYOKAY000538561";
    assert_eq!(read_forward_reply(&mut reply, true).unwrap(), Some(38561));
}

#[test]
fn forward_fixed_port_reply_has_no_port() {
    let mut reply: &[u8] = b"OKAYOKAY";
    assert_eq!(read_forward_reply(&mut reply, false).unwrap(), None);
}

#[test]
fn forward_failures_carry_the_servers_reason() {
    let mut reply: &[u8] = b"FAIL0014device 'X' not found";
    match read_forward_reply(&mut reply, true) {
        Err(WireError::Fail(msg)) => assert_eq!(msg, "device 'X' not found"),
        other => panic!("{other:?}"),
    }
    let mut reply: &[u8] = b"OKAYFAIL000bcannot bind";
    assert!(matches!(
        read_forward_reply(&mut reply, true),
        Err(WireError::Fail(m)) if m == "cannot bind"
    ));
}

#[test]
fn forward_reply_without_a_valid_port_is_an_error() {
    let mut reply: &[u8] = b"OKAYOKAY";
    assert!(read_forward_reply(&mut reply, true).is_err());
    let mut reply: &[u8] = b"OKAYOKAY0001x";
    assert!(matches!(
        read_forward_reply(&mut reply, true),
        Err(WireError::Bad(_))
    ));
}

#[test]
fn requests_are_length_prefixed_in_hex() {
    let mut out = Vec::new();
    send(&mut out, "host:devices-l").unwrap();
    assert_eq!(out, b"000ehost:devices-l");
}

#[test]
fn shell_v2_separates_streams_and_reports_exit() {
    let mut wire = Vec::new();
    for (id, data) in [
        (1u8, &b"out"[..]),
        (2, b"err"),
        (1, b"put"),
        (3, &[2u8][..]),
    ] {
        wire.push(id);
        wire.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wire.extend_from_slice(data);
    }
    let out = read_shell_v2(&mut wire.as_slice()).unwrap();
    assert_eq!(out.stdout, b"output");
    assert_eq!(out.stderr, b"err");
    assert_eq!(out.exit, Some(2));
}

/// An in-memory stream: reads come from `reply`, writes are recorded.
struct Mem {
    written: Vec<u8>,
    reply: Vec<u8>,
    pos: usize,
}
impl Mem {
    fn new(reply: Vec<u8>) -> Self {
        Mem {
            written: Vec::new(),
            reply,
            pos: 0,
        }
    }
}
impl Read for Mem {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.reply.len() - self.pos);
        buf[..n].copy_from_slice(&self.reply[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
impl Write for Mem {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn sync_send_frames_the_file_and_reads_the_ack() {
    let mut s = Mem::new(b"OKAY\0\0\0\0".to_vec());
    sync_send(&mut s, &mut &b"hello"[..], "/data/local/tmp/x", 0o755).unwrap();
    let w = &s.written;
    // "/data/local/tmp/x" (17) + "," + "33261" (0o100755) = 23 bytes.
    assert_eq!(&w[..8], &sync_header(b"SEND", 23));
    assert_eq!(&w[8..31], b"/data/local/tmp/x,33261");
    assert_eq!(&w[31..39], &sync_header(b"DATA", 5));
    assert_eq!(&w[39..44], b"hello");
    assert_eq!(&w[44..48], b"DONE");

    let mut s = Mem::new(b"FAIL\x06\0\0\0no way".to_vec());
    assert!(matches!(
        sync_send(&mut s, &mut &b""[..], "/x", 0o755),
        Err(WireError::Fail(m)) if m == "no way"
    ));
}

fn stat_reply(mode: u32, size: u32, mtime: u32) -> Vec<u8> {
    let mut v = b"STAT".to_vec();
    for n in [mode, size, mtime] {
        v.extend_from_slice(&n.to_le_bytes());
    }
    v
}

fn data(chunk: &[u8]) -> Vec<u8> {
    let mut v = sync_header(b"DATA", chunk.len() as u32).to_vec();
    v.extend_from_slice(chunk);
    v
}

#[test]
fn sync_recv_v1_streams_chunks_and_reports_progress() {
    let mut reply = stat_reply(0o100644, 8, 1_700_000_000);
    reply.extend(data(b"hello"));
    reply.extend(data(b"abc"));
    reply.extend(sync_header(b"DONE", 0));
    let mut s = Mem::new(reply);
    let mut sink = Vec::new();
    let mut seen = Vec::new();
    let (n, mtime) = sync_recv(&mut s, b"/sdcard/a b", false, &mut sink, &mut |b| {
        seen.push(b)
    })
    .unwrap();
    assert_eq!((n, mtime), (8, 1_700_000_000));
    assert_eq!(sink, b"helloabc");
    assert_eq!(seen, [5, 8]);
    let mut expect = sync_header(b"STAT", 11).to_vec();
    expect.extend_from_slice(b"/sdcard/a b");
    expect.extend_from_slice(&sync_header(b"RECV", 11));
    expect.extend_from_slice(b"/sdcard/a b");
    assert_eq!(s.written, expect);
}

#[test]
fn sync_recv_v2_sends_rcv2_with_flags_and_non_utf8_paths() {
    let mut reply = stat_reply(0o100644, 2, 5);
    reply.extend(data(b"hi"));
    reply.extend(sync_header(b"DONE", 0));
    let mut s = Mem::new(reply);
    let path = b"/sdcard/\xff\xfe";
    let mut sink = Vec::new();
    sync_recv(&mut s, path, true, &mut sink, &mut |_| {}).unwrap();
    let mut expect = sync_header(b"STAT", 10).to_vec();
    expect.extend_from_slice(path);
    expect.extend_from_slice(b"RCV2");
    expect.extend_from_slice(&10u32.to_le_bytes());
    expect.extend_from_slice(path);
    expect.extend_from_slice(b"RCV2");
    expect.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(s.written, expect);
    assert_eq!(sink, b"hi");
}

#[test]
fn sync_recv_reports_missing_files_fail_and_bad_frames() {
    let mut sink = Vec::new();
    // STAT of a missing path answers mode 0.
    let mut s = Mem::new(stat_reply(0, 0, 0));
    assert!(matches!(
        sync_recv(&mut s, b"/x", false, &mut sink, &mut |_| {}),
        Err(RecvError::Wire(WireError::Fail(_)))
    ));
    // FAIL mid-stream.
    let mut reply = stat_reply(0o100644, 1, 1);
    reply.extend(data(b"a"));
    reply.extend(sync_header(b"FAIL", 7));
    reply.extend_from_slice(b"I/O err");
    let mut s = Mem::new(reply);
    assert!(matches!(
        sync_recv(&mut s, b"/x", true, &mut sink, &mut |_| {}),
        Err(RecvError::Wire(WireError::Fail(m))) if m == "I/O err"
    ));
    // Oversized chunk.
    let mut reply = stat_reply(0o100644, 1, 1);
    reply.extend(sync_header(b"DATA", (SYNC_CHUNK + 1) as u32));
    let mut s = Mem::new(reply);
    assert!(matches!(
        sync_recv(&mut s, b"/x", false, &mut sink, &mut |_| {}),
        Err(RecvError::Wire(WireError::Bad(_)))
    ));
    // Truncated stream.
    let mut reply = stat_reply(0o100644, 1, 1);
    reply.extend(sync_header(b"DATA", 4));
    let mut s = Mem::new(reply);
    assert!(matches!(
        sync_recv(&mut s, b"/x", false, &mut sink, &mut |_| {}),
        Err(RecvError::Wire(WireError::Io(_)))
    ));
}

#[test]
fn sync_recv_separates_local_write_failures() {
    struct Full;
    impl Write for Full {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disk full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut reply = stat_reply(0o100644, 1, 1);
    reply.extend(data(b"a"));
    let mut s = Mem::new(reply);
    assert!(matches!(
        sync_recv(&mut s, b"/x", false, &mut Full, &mut |_| {}),
        Err(RecvError::Sink(_))
    ));
}

//! adb's wire protocols: smart sockets, shell v2 and sync.

use std::fmt;
use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// Largest `DATA` chunk the sync protocol accepts.
pub const SYNC_CHUNK: usize = 64 * 1024;

/// Upper bound on a single shell-protocol packet or sync `FAIL` message, so a
/// corrupt length cannot make us allocate gigabytes.
const MAX_PACKET: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum WireError {
    Io(io::Error),
    /// The server or phone refused the request, with its reason.
    Fail(String),
    /// A reply that does not follow the protocol.
    Bad(String),
}

impl From<io::Error> for WireError {
    fn from(e: io::Error) -> Self {
        WireError::Io(e)
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Io(e) => match e.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                    write!(f, "timed out without a reply")
                }
                io::ErrorKind::UnexpectedEof => write!(f, "the connection closed early"),
                _ => write!(f, "{e}"),
            },
            WireError::Fail(msg) => write!(f, "{msg}"),
            WireError::Bad(msg) => write!(f, "unexpected reply: {msg}"),
        }
    }
}

pub type Wire<T> = std::result::Result<T, WireError>;

/// Send a smart-socket request: 4 hex digits of length, then the request.
pub fn send<W: Write>(w: &mut W, service: &str) -> Wire<()> {
    if service.len() > 0xffff {
        return Err(WireError::Bad(format!(
            "request too long ({} bytes)",
            service.len()
        )));
    }
    w.write_all(format!("{:04x}{service}", service.len()).as_bytes())?;
    Ok(())
}

/// `OKAY`, or `FAIL` followed by a length-prefixed reason.
pub fn read_status<R: Read>(r: &mut R) -> Wire<()> {
    let mut status = [0u8; 4];
    r.read_exact(&mut status)?;
    match &status {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(WireError::Fail(read_hex_string(r)?)),
        other => Err(WireError::Bad(String::from_utf8_lossy(other).into_owned())),
    }
}

pub fn request<S: Read + Write>(s: &mut S, service: &str) -> Wire<()> {
    send(s, service)?;
    read_status(s)
}

/// 4 hex digits of length, then that many bytes.
pub fn read_hex_string<R: Read>(r: &mut R) -> Wire<String> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = std::str::from_utf8(&len)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .ok_or_else(|| WireError::Bad(format!("bad length {:?}", String::from_utf8_lossy(&len))))?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The reply to `host-serial:…:forward:` and `…:killforward:`. The server
/// answers `OKAY` once for accepting the request and again for the result, and
/// for `tcp:0` follows with the chosen port as a length-prefixed string. A
/// refusal is a `FAIL` in place of either `OKAY`.
pub fn read_forward_reply<R: Read>(r: &mut R, expect_port: bool) -> Wire<Option<u16>> {
    read_status(r)?;
    read_status(r)?;
    if !expect_port {
        return Ok(None);
    }
    let port = read_hex_string(r)?;
    match port.trim().parse::<u16>() {
        Ok(p) if p != 0 => Ok(Some(p)),
        _ => Err(WireError::Bad(format!("port {port:?}"))),
    }
}

pub const SHELL_STDOUT: u8 = 1;
pub const SHELL_STDERR: u8 = 2;
pub const SHELL_EXIT: u8 = 3;
pub const SHELL_CLOSE_STDIN: u8 = 4;

pub struct ShellOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// None when the connection closed without reporting one.
    pub exit: Option<u8>,
}

/// Shell protocol v2: packets of a 1-byte stream id, a little-endian u32
/// length and the payload, ending with an exit-code packet.
pub fn read_shell_v2<R: Read>(r: &mut R) -> Wire<ShellOutput> {
    let mut out = ShellOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        exit: None,
    };
    loop {
        let mut header = [0u8; 5];
        match r.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(out),
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_PACKET {
            return Err(WireError::Bad(format!("shell packet of {len} bytes")));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        match header[0] {
            SHELL_STDOUT => out.stdout.extend_from_slice(&payload),
            SHELL_STDERR => out.stderr.extend_from_slice(&payload),
            SHELL_EXIT => {
                out.exit = payload.first().copied();
                return Ok(out);
            }
            _ => {}
        }
    }
}

// ── Sync protocol ───────────────────────────────────────────────────────────

/// A sync-protocol header: 4-byte id and a little-endian u32.
pub fn sync_header(id: &[u8; 4], n: u32) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[..4].copy_from_slice(id);
    h[4..].copy_from_slice(&n.to_le_bytes());
    h
}

/// `SEND <path>,<mode>`, the file as `DATA` chunks, `DONE <mtime>`, then the
/// phone's `OKAY` or `FAIL <reason>`.
///
/// The mode goes out in decimal and includes the file-type bits (`S_IFREG`),
/// the same `"%s,%d"` form upstream adb's client sends, so 0o755 becomes
/// `33261` (0o100755).
pub fn sync_send<S: Read + Write>(
    s: &mut S,
    file: &mut impl Read,
    remote: &str,
    mode: u32,
) -> Wire<()> {
    const S_IFREG: u32 = 0o100000;
    let spec = format!("{remote},{}", S_IFREG | mode);
    s.write_all(&sync_header(b"SEND", spec.len() as u32))?;
    s.write_all(spec.as_bytes())?;

    let mut buf = vec![0u8; SYNC_CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        s.write_all(&sync_header(b"DATA", n as u32))?;
        s.write_all(&buf[..n])?;
    }

    let mtime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    s.write_all(&sync_header(b"DONE", mtime))?;
    read_sync_status(s)
}

fn read_sync_status<R: Read>(r: &mut R) -> Wire<()> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header)?;
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    match &header[..4] {
        b"OKAY" => Ok(()),
        b"FAIL" if len <= MAX_PACKET => {
            let mut msg = vec![0u8; len];
            r.read_exact(&mut msg)?;
            Err(WireError::Fail(String::from_utf8_lossy(&msg).into_owned()))
        }
        other => Err(WireError::Bad(String::from_utf8_lossy(other).into_owned())),
    }
}

/// Why a pull stopped: the connection, or the local file.
#[derive(Debug)]
pub enum RecvError {
    Wire(WireError),
    Sink(io::Error),
}

impl From<WireError> for RecvError {
    fn from(e: WireError) -> Self {
        RecvError::Wire(e)
    }
}

impl From<io::Error> for RecvError {
    fn from(e: io::Error) -> Self {
        RecvError::Wire(e.into())
    }
}

/// A sync request carrying a path: id, path length, path.
fn sync_path_request<W: Write>(w: &mut W, id: &[u8; 4], path: &[u8]) -> io::Result<()> {
    w.write_all(&sync_header(id, path.len() as u32))?;
    w.write_all(path)
}

/// `STAT`: `(mode, size, mtime)`. A mode of 0 means the path does not exist.
fn sync_stat<S: Read + Write>(s: &mut S, path: &[u8]) -> Wire<(u32, u32, u32)> {
    sync_path_request(s, b"STAT", path)?;
    let mut reply = [0u8; 16];
    s.read_exact(&mut reply)?;
    if &reply[..4] != b"STAT" {
        return Err(WireError::Bad(
            String::from_utf8_lossy(&reply[..4]).into_owned(),
        ));
    }
    let word = |i: usize| u32::from_le_bytes([reply[i], reply[i + 1], reply[i + 2], reply[i + 3]]);
    Ok((word(4), word(8), word(12)))
}

/// Pull `remote` over an open sync session into `sink`, reporting bytes
/// received so far. `v2` selects `RCV2` (adb's RECV2) over `RECV`. Returns the
/// byte count and the file's mtime.
pub fn sync_recv<S: Read + Write>(
    s: &mut S,
    remote: &[u8],
    v2: bool,
    sink: &mut impl Write,
    progress: &mut dyn FnMut(u64),
) -> std::result::Result<(u64, u32), RecvError> {
    let (mode, _, mtime) = sync_stat(s, remote)?;
    if mode == 0 {
        return Err(WireError::Fail("No such file or directory".into()).into());
    }

    if v2 {
        // As upstream adb's client sends it: the usual path request under the
        // v2 id, then a setup record of the id again and the flags (0: no
        // compression).
        sync_path_request(s, b"RCV2", remote)
            .and_then(|()| s.write_all(b"RCV2"))
            .and_then(|()| s.write_all(&0u32.to_le_bytes()))
            .map_err(WireError::from)?;
    } else {
        sync_path_request(s, b"RECV", remote).map_err(WireError::from)?;
    }

    let mut total = 0u64;
    let mut buf = vec![0u8; SYNC_CHUNK];
    loop {
        let mut header = [0u8; 8];
        s.read_exact(&mut header)?;
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        match &header[..4] {
            b"DATA" => {
                if len > SYNC_CHUNK {
                    return Err(WireError::Bad(format!("data chunk of {len} bytes")).into());
                }
                s.read_exact(&mut buf[..len])?;
                sink.write_all(&buf[..len]).map_err(RecvError::Sink)?;
                total += len as u64;
                progress(total);
            }
            b"DONE" => return Ok((total, mtime)),
            b"FAIL" if len <= MAX_PACKET => {
                let mut msg = vec![0u8; len];
                s.read_exact(&mut msg)?;
                return Err(WireError::Fail(String::from_utf8_lossy(&msg).into_owned()).into());
            }
            other => return Err(WireError::Bad(String::from_utf8_lossy(other).into_owned()).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

//! The sync protocol: `SEND`, `STAT` and `RECV`/`RCV2`.

use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Wire, WireError, MAX_PACKET};

/// Largest `DATA` chunk the sync protocol accepts.
pub const SYNC_CHUNK: usize = 64 * 1024;

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

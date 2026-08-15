//! Wire format shared by the on-device daemon and the desktop host.
//!
//! Both ends depend on this crate, so there is exactly one definition of the
//! encoding and no opportunity for the two to disagree.
//!
//! # Framing
//!
//! Every message is a little-endian `u32` byte length followed by that many
//! bytes of [postcard](https://docs.rs/postcard). A full scan is tens of
//! thousands of records, and postcard encodes integers as varints and omits
//! field names entirely.
//!
//! Variants are encoded by their index, so the order of [`Request`] and
//! [`Frame`] is part of the wire contract. Adding a variant anywhere but the end
//! changes the meaning of every variant after it; daemon and host ship together
//! in one bundle, so this is safe to change, but never mix versions.
//!
//! # Paths are bytes, not strings
//!
//! Android filenames are arbitrary byte sequences with no guarantee of being
//! UTF-8. Carrying them as `String` would corrupt them in transit, and a
//! corrupted path can arrive as a delete target. They stay `Vec<u8>` end to end;
//! conversion to text happens only for display.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, Read, Write};

/// Upper bound on a single frame, so a corrupt length prefix cannot make the
/// reader allocate wildly. A 100k-entry directory encodes to a few MB.
pub const MAX_FRAME_BYTES: u32 = 64 * 1024 * 1024;

/// Host → daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Ping,
    /// Walk `root` and stream [`Frame::Dir`] until [`Frame::ScanDone`].
    Scan {
        root: Vec<u8>,
    },
    /// Recursively delete `path`. The daemon validates it against the session
    /// root and refuses anything outside — see the `guard` module in the daemon.
    Delete {
        path: Vec<u8>,
    },
    Shutdown,
}

/// Daemon → host. A scan produces many `Dir` frames then one `ScanDone`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    Pong,
    /// Opens a scan stream, carrying the root the daemon resolved to.
    ///
    /// Not necessarily the root that was requested: `/sdcard` is a symlink chain
    /// to `/storage/emulated/0` on a typical device, and the daemon canonicalises
    /// before walking. Every following [`Frame::Dir`] is keyed on the resolved
    /// path, so the host must index against this rather than what it asked for.
    ScanStarted {
        root: Vec<u8>,
    },
    /// The contents of one directory.
    ///
    /// A directory is discovered while reading its parent, so the frame naming it
    /// as an entry always precedes the frame describing its contents. That
    /// ordering is what lets the host build the tree without buffering.
    Dir {
        path: Vec<u8>,
        entries: Vec<Entry>,
    },
    ScanDone(ScanStats),
    Deleted {
        items: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: Vec<u8>,
    /// Byte length for files. Always 0 for directories — the host aggregates
    /// subtree totals as frames arrive, which is what lets the UI show sizes
    /// climbing during the scan instead of waiting for a final total.
    pub size: u64,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    pub total_size: u64,
    /// Entries that could not be read — almost always permission denied.
    pub errors: u64,
    pub elapsed_ms: u64,
}

// ── Framing ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    Encode(postcard::Error),
    Decode(postcard::Error),
    /// A length prefix exceeded [`MAX_FRAME_BYTES`], or the stream ended
    /// part-way through a frame.
    Malformed(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Io(e) => write!(f, "i/o error: {e}"),
            ProtocolError::Encode(e) => write!(f, "failed to encode frame: {e}"),
            ProtocolError::Decode(e) => write!(f, "failed to decode frame: {e}"),
            ProtocolError::Malformed(m) => write!(f, "malformed stream: {m}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(e: io::Error) -> Self {
        ProtocolError::Io(e)
    }
}

/// Write one length-prefixed message.
pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<(), ProtocolError> {
    let body = postcard::to_stdvec(msg).map_err(ProtocolError::Encode)?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|n| *n <= MAX_FRAME_BYTES)
        .ok_or_else(|| {
            ProtocolError::Malformed(format!("frame of {} bytes is too large", body.len()))
        })?;

    w.write_all(&len.to_le_bytes())?;
    w.write_all(&body)?;
    Ok(())
}

/// Read one length-prefixed message.
///
/// Returns `Ok(None)` on a clean end of stream — the peer closed between frames,
/// which is normal shutdown, not an error.
pub fn read_msg<R: Read, T: serde::de::DeserializeOwned>(
    r: &mut R,
) -> Result<Option<T>, ProtocolError> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::Malformed(format!(
            "length prefix of {len} bytes exceeds the {MAX_FRAME_BYTES} byte limit"
        )));
    }

    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            ProtocolError::Malformed(format!("stream ended {len} bytes into a frame body"))
        } else {
            ProtocolError::Io(e)
        }
    })?;

    postcard::from_bytes(&body)
        .map(Some)
        .map_err(ProtocolError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_frame() -> Frame {
        Frame::Dir {
            path: b"/sdcard/DCIM".to_vec(),
            entries: vec![
                Entry {
                    name: b"IMG_0001.jpg".to_vec(),
                    size: 4_194_304,
                    kind: EntryKind::File,
                },
                Entry {
                    name: b"Camera".to_vec(),
                    size: 0,
                    kind: EntryKind::Dir,
                },
            ],
        }
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &dir_frame()).unwrap();
        let back: Frame = read_msg(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(back, dir_frame());
    }

    #[test]
    fn requests_round_trip() {
        for req in [
            Request::Ping,
            Request::Scan {
                root: b"/sdcard".to_vec(),
            },
            Request::Delete {
                path: b"/sdcard/Download/big.zip".to_vec(),
            },
            Request::Shutdown,
        ] {
            let mut buf = Vec::new();
            write_msg(&mut buf, &req).unwrap();
            let back: Request = read_msg(&mut buf.as_slice()).unwrap().unwrap();
            assert_eq!(back, req);
        }
    }

    /// The daemon reports the root it resolved to, which is not the root that
    /// was asked for: /sdcard is a symlink chain on a real device.
    #[test]
    fn scan_started_carries_the_resolved_root() {
        let frame = Frame::ScanStarted {
            root: b"/storage/emulated/0".to_vec(),
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &frame).unwrap();
        let back: Frame = read_msg(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(back, frame);
    }

    #[test]
    fn many_frames_stream_back_in_order() {
        let mut buf = Vec::new();
        for i in 0..100u64 {
            write_msg(&mut buf, &Frame::Deleted { items: i }).unwrap();
        }
        write_msg(&mut buf, &Frame::ScanDone(ScanStats::default())).unwrap();

        let mut cursor = buf.as_slice();
        for i in 0..100u64 {
            let f: Frame = read_msg(&mut cursor).unwrap().unwrap();
            assert_eq!(f, Frame::Deleted { items: i });
        }
        assert!(matches!(
            read_msg::<_, Frame>(&mut cursor).unwrap().unwrap(),
            Frame::ScanDone(_)
        ));
        // Clean end of stream, not an error.
        assert!(read_msg::<_, Frame>(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn non_utf8_names_survive() {
        // 0xFF is never valid UTF-8. A String-based protocol would mangle this,
        // and a mangled path is one that DELETE could resolve wrongly.
        let raw = vec![0xFFu8, 0xFE, b'p', b'i', b'c', 0x80];
        let frame = Frame::Dir {
            path: vec![b'/', 0xFF],
            entries: vec![Entry {
                name: raw.clone(),
                size: 1,
                kind: EntryKind::File,
            }],
        };

        let mut buf = Vec::new();
        write_msg(&mut buf, &frame).unwrap();
        let back: Frame = read_msg(&mut buf.as_slice()).unwrap().unwrap();

        let Frame::Dir { path, entries } = back else {
            panic!("wrong variant")
        };
        assert_eq!(path, vec![b'/', 0xFF]);
        assert_eq!(entries[0].name, raw);
    }

    #[test]
    fn oversized_length_prefix_is_rejected_without_allocating() {
        let mut buf = (MAX_FRAME_BYTES + 1).to_le_bytes().to_vec();
        buf.extend_from_slice(b"whatever");
        let err = read_msg::<_, Frame>(&mut buf.as_slice()).unwrap_err();
        assert!(matches!(err, ProtocolError::Malformed(_)), "got {err:?}");
    }

    #[test]
    fn truncated_body_is_an_error_not_a_silent_none() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &dir_frame()).unwrap();
        buf.truncate(buf.len() - 3);

        let err = read_msg::<_, Frame>(&mut buf.as_slice()).unwrap_err();
        assert!(matches!(err, ProtocolError::Malformed(_)), "got {err:?}");
    }

    #[test]
    fn postcard_encodes_far_smaller_than_equivalent_json() {
        // Representative directory: 200 files with realistic names and sizes.
        let entries: Vec<Entry> = (0..200)
            .map(|i| Entry {
                name: format!("IMG_{i:05}.jpg").into_bytes(),
                size: 3_000_000 + i as u64,
                kind: EntryKind::File,
            })
            .collect();
        let frame = Frame::Dir {
            path: b"/sdcard/DCIM/Camera".to_vec(),
            entries,
        };

        let mut encoded = Vec::new();
        write_msg(&mut encoded, &frame).unwrap();

        // The same data as JSON, with an absolute path per node plus field
        // names and quoting.
        let json_equivalent: usize = (0..200)
            .map(|i| {
                format!(
                    r#"{{"name":"IMG_{i:05}.jpg","path":"/sdcard/DCIM/Camera/IMG_{i:05}.jpg","type":"file","size":{}}},"#,
                    3_000_000 + i
                )
                .len()
            })
            .sum();

        assert!(
            encoded.len() * 3 < json_equivalent,
            "expected postcard ({} bytes) to be well under a third of JSON ({} bytes)",
            encoded.len(),
            json_equivalent,
        );
    }
}

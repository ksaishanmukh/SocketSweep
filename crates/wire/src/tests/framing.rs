//! Frames on a stream: edge cases of reading and writing.

use std::io::{self, Read};

use super::support::*;
use crate::*;

fn sample_frame() -> ServerFrame {
    ServerFrame {
        id: 1,
        msg: ServerMsg::Scan(scan_events()),
    }
}

#[test]
fn many_frames_stream_back_in_order() {
    let mut buf = Vec::new();
    for i in 0..100u64 {
        write_frame(
            &mut buf,
            &ServerFrame {
                id: 1,
                msg: ServerMsg::DeleteProgress { items: i },
            },
        )
        .unwrap();
    }

    let mut cursor = buf.as_slice();
    for i in 0..100u64 {
        let f: ServerFrame = read_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(f.msg, ServerMsg::DeleteProgress { items: i });
    }
    // Clean end of stream, not an error.
    assert!(read_frame::<_, ServerFrame>(&mut cursor).unwrap().is_none());
}

#[test]
fn an_empty_stream_is_a_clean_end() {
    assert!(read_frame::<_, ServerFrame>(&mut &[][..])
        .unwrap()
        .is_none());
    assert!(read_frame_body(&mut &[][..]).unwrap().is_none());
}

#[test]
fn a_stream_ending_inside_the_length_prefix_is_malformed() {
    let full = encode_frame(&sample_frame()).unwrap();
    for cut in 1..4 {
        let err = read_frame::<_, ServerFrame>(&mut &full[..cut]).unwrap_err();
        assert!(
            matches!(err, WireError::Malformed(_)),
            "{cut} prefix bytes: got {err:?}"
        );
    }
}

#[test]
fn a_truncated_body_is_malformed() {
    let full = encode_frame(&sample_frame()).unwrap();
    for cut in [4, 5, full.len() - 1] {
        let err = read_frame::<_, ServerFrame>(&mut &full[..cut]).unwrap_err();
        assert!(
            matches!(err, WireError::Malformed(_)),
            "cut at {cut}: {err:?}"
        );
    }
}

#[test]
fn an_oversized_length_prefix_is_rejected_without_allocating() {
    let mut buf = (MAX_FRAME_BYTES + 1).to_le_bytes().to_vec();
    buf.extend_from_slice(b"whatever");
    let err = read_frame::<_, ServerFrame>(&mut buf.as_slice()).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");

    let err = read_frame_body(&mut &u32::MAX.to_le_bytes()[..]).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");
}

#[test]
fn a_frame_of_exactly_the_maximum_is_accepted() {
    let body = vec![0u8; MAX_FRAME_BYTES as usize];
    let mut buf = MAX_FRAME_BYTES.to_le_bytes().to_vec();
    buf.extend_from_slice(&body);
    assert_eq!(
        read_frame_body(&mut buf.as_slice())
            .unwrap()
            .map(|b| b.len()),
        Some(MAX_FRAME_BYTES as usize)
    );
}

#[test]
fn an_oversized_message_is_refused_on_write() {
    let huge = ServerMsg::Failed {
        code: ErrCode::Internal,
        errno: None,
        message: "x".repeat(MAX_FRAME_BYTES as usize),
    };
    let mut buf = Vec::new();
    let err = write_frame(&mut buf, &huge).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");
    assert!(buf.is_empty(), "nothing is written for a refused frame");
}

#[test]
fn trailing_bytes_in_a_body_are_malformed() {
    let mut body = enc(&ServerFrame {
        id: 0,
        msg: ServerMsg::Heartbeat,
    });
    body.push(0);
    let mut buf = (body.len() as u32).to_le_bytes().to_vec();
    buf.extend(body);
    let err = read_frame::<_, ServerFrame>(&mut buf.as_slice()).unwrap_err();
    assert!(matches!(err, WireError::Malformed(_)), "got {err:?}");
}

#[test]
fn an_undecodable_body_is_a_decode_error() {
    // Variant 200 of ServerMsg does not exist.
    let buf = [2, 0, 0, 0, 0, 200];
    let err = read_frame::<_, ServerFrame>(&mut &buf[..]).unwrap_err();
    assert!(matches!(err, WireError::Decode(_)), "got {err:?}");
}

/// Hands back one byte per call, as a socket is entitled to.
struct Trickle<'a>(&'a [u8]);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match (self.0.split_first(), buf.first_mut()) {
            (Some((b, rest)), Some(slot)) => {
                *slot = *b;
                self.0 = rest;
                Ok(1)
            }
            _ => Ok(0),
        }
    }
}

#[test]
fn a_frame_split_across_reads_is_reassembled() {
    let buf = encode_frame(&sample_frame()).unwrap();
    let back: ServerFrame = read_frame(&mut Trickle(&buf)).unwrap().unwrap();
    assert_eq!(back, sample_frame());
}

/// Fails with `Interrupted` before every byte, which readers must retry.
struct Interrupting<'a> {
    data: &'a [u8],
    interrupt: bool,
}

impl Read for Interrupting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.interrupt = !self.interrupt;
        if self.interrupt {
            return Err(io::ErrorKind::Interrupted.into());
        }
        Trickle(self.data)
            .read(buf)
            .inspect(|&n| self.data = &self.data[n..])
    }
}

#[test]
fn interrupted_reads_are_retried() {
    let buf = encode_frame(&sample_frame()).unwrap();
    let mut r = Interrupting {
        data: &buf,
        interrupt: false,
    };
    let back: ServerFrame = read_frame(&mut r).unwrap().unwrap();
    assert_eq!(back, sample_frame());
}

#[test]
fn other_read_errors_are_io_errors() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::ConnectionReset.into())
        }
    }
    let err = read_frame::<_, ServerFrame>(&mut Broken).unwrap_err();
    assert!(matches!(err, WireError::Io(_)), "got {err:?}");
}

//! Length-prefixed postcard frames.

use std::fmt;
use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::MAX_FRAME_BYTES;

/// Why a frame could not be written or read.
#[derive(Debug)]
pub enum WireError {
    Io(io::Error),
    Encode(postcard::Error),
    /// The body arrived whole but is not a valid message of the expected type.
    Decode(postcard::Error),
    /// The stream ended part-way through a frame, or a length prefix exceeded
    /// [`MAX_FRAME_BYTES`]. The connection is unusable after either.
    Malformed(String),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Io(e) => write!(f, "i/o error: {e}"),
            WireError::Encode(e) => write!(f, "failed to encode frame: {e}"),
            WireError::Decode(e) => write!(f, "failed to decode frame: {e}"),
            WireError::Malformed(m) => write!(f, "malformed stream: {m}"),
        }
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WireError::Io(e) => Some(e),
            WireError::Encode(e) | WireError::Decode(e) => Some(e),
            WireError::Malformed(_) => None,
        }
    }
}

impl From<io::Error> for WireError {
    fn from(e: io::Error) -> Self {
        WireError::Io(e)
    }
}

/// Encode one message as a complete frame: length prefix and body.
///
/// The daemon's writer encodes events as they are queued, so it can account
/// the bulk queue in bytes ([`BULK_HIGH_WATER`](crate::BULK_HIGH_WATER)).
pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, WireError> {
    let mut frame = postcard::to_extend(msg, vec![0u8; 4]).map_err(WireError::Encode)?;
    let body_len = frame.len() - 4;
    let len = u32::try_from(body_len)
        .ok()
        .filter(|n| *n <= MAX_FRAME_BYTES)
        .ok_or_else(|| {
            WireError::Malformed(format!(
                "frame of {body_len} bytes exceeds the {MAX_FRAME_BYTES} byte limit"
            ))
        })?;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    Ok(frame)
}

/// Write one frame. Not flushed: wrap a buffered writer's batch in a flush.
pub fn write_frame<W: Write + ?Sized, T: Serialize>(w: &mut W, msg: &T) -> Result<(), WireError> {
    w.write_all(&encode_frame(msg)?)?;
    Ok(())
}

/// Read one frame.
///
/// Returns `Ok(None)` on a clean end of stream: the peer closed between frames,
/// which is normal shutdown, not an error. A stream that ends after one to
/// three bytes of the length prefix, or inside the body, is not a clean close
/// and is reported as [`WireError::Malformed`], as is a length prefix above
/// [`MAX_FRAME_BYTES`], which is refused before anything is allocated. So is a
/// body with bytes left over after its message: postcard would ignore them,
/// but they mean the two ends disagree about the format.
pub fn read_frame<R: Read + ?Sized, T: DeserializeOwned>(
    r: &mut R,
) -> Result<Option<T>, WireError> {
    let Some(body) = read_frame_body(r)? else {
        return Ok(None);
    };
    let (msg, rest) = postcard::take_from_bytes(&body).map_err(WireError::Decode)?;
    if !rest.is_empty() {
        return Err(WireError::Malformed(format!(
            "{} unread bytes after a {} byte frame's message",
            rest.len(),
            body.len()
        )));
    }
    Ok(Some(msg))
}

/// Read one frame's body without decoding it, with the same end-of-stream and
/// size rules as [`read_frame`].
///
/// For the daemon's first frame, which it must inspect with
/// [`check_hello`](crate::check_hello) before it knows the frame is a `Hello`
/// of its own version.
pub fn read_frame_body<R: Read + ?Sized>(r: &mut R) -> Result<Option<Vec<u8>>, WireError> {
    let mut len_buf = [0u8; 4];
    let mut filled = 0;
    while filled < len_buf.len() {
        match r.read(&mut len_buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(WireError::Malformed(format!(
                    "stream ended {filled} bytes into a length prefix"
                )))
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }

    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        return Err(WireError::Malformed(format!(
            "length prefix of {len} bytes exceeds the {MAX_FRAME_BYTES} byte limit"
        )));
    }

    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            WireError::Malformed(format!("stream ended inside a {len} byte frame body"))
        } else {
            WireError::Io(e)
        }
    })?;
    Ok(Some(body))
}

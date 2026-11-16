//! The first frame: checking the host's `Hello` and answering it.

use std::io::Write;

use socketsweep_wire::{check_hello, encode_frame, read_frame_body, HelloReply, WireError};

use super::{Conn, SessionConfig};

/// Why the handshake did not lead to a session.
#[derive(Debug)]
pub enum HandshakeError {
    /// The peer closed before sending a `Hello`.
    Closed,
    /// Wrong magic, token or version, or not a `Hello`: answered `Refused`.
    Refused,
    Wire(WireError),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::Closed => write!(f, "closed before the handshake"),
            HandshakeError::Refused => write!(f, "refused: bad token or version, or not a Hello"),
            HandshakeError::Wire(e) => write!(f, "handshake failed: {e}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Read the connection's first frame, check it with [`check_hello`] and
/// answer it.
pub fn handshake(conn: &mut Conn, cfg: &SessionConfig) -> Result<(), HandshakeError> {
    let body = match read_frame_body(&mut conn.reader) {
        Ok(Some(b)) => b,
        Ok(None) => return Err(HandshakeError::Closed),
        Err(e) => return Err(HandshakeError::Wire(e)),
    };
    let hello = check_hello(&body, &cfg.token);
    let reply = match &hello {
        Some(hello) => HelloReply::Welcome(hello.welcome(cfg.daemon_build.clone())),
        None => HelloReply::Refused,
    };
    let bytes = encode_frame(&reply).map_err(HandshakeError::Wire)?;
    conn.writer
        .write_all(&bytes)
        .and_then(|()| conn.writer.flush())
        .map_err(|e| HandshakeError::Wire(e.into()))?;
    hello.map(|_| ()).ok_or(HandshakeError::Refused)
}

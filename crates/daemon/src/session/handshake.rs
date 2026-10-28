//! The first frame: checking the host's `Hello` and answering it.

use std::io::Write;

use socketsweep_wire::{
    check_hello, encode_frame, read_frame_body, Caps, Hello, HelloCheck, HelloReply, WireError,
};

use super::{Conn, SessionConfig};

/// Why the handshake did not lead to a session.
#[derive(Debug)]
pub enum HandshakeError {
    /// The peer closed before sending a `Hello`.
    Closed,
    /// Wrong magic, wrong token, or not a `Hello`: answered `Refused`.
    Refused,
    /// Right token, other version: answered `WrongVersion`.
    WrongVersion {
        host_version: u16,
    },
    Wire(WireError),
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::Closed => write!(f, "closed before the handshake"),
            HandshakeError::Refused => write!(f, "refused: bad token or not a Hello"),
            HandshakeError::WrongVersion { host_version } => {
                write!(f, "refused: the host speaks protocol {host_version}")
            }
            HandshakeError::Wire(e) => write!(f, "handshake failed: {e}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Read the connection's first frame, check it with [`check_hello`] (token,
/// then version, then the rest) and answer it. Returns the accepted `Hello`
/// and the negotiated capabilities.
pub fn handshake(conn: &mut Conn, cfg: &SessionConfig) -> Result<(Hello, Caps), HandshakeError> {
    let body = match read_frame_body(&mut conn.reader) {
        Ok(Some(b)) => b,
        Ok(None) => return Err(HandshakeError::Closed),
        Err(e) => return Err(HandshakeError::Wire(e)),
    };
    let check = check_hello(&body, &cfg.token);
    let reply = match &check {
        HelloCheck::Accepted(hello) => {
            let welcome =
                hello.welcome(cfg.caps, cfg.daemon_build.clone(), cfg.workers, cfg.limits);
            HelloReply::Welcome(welcome)
        }
        rejected => rejected.rejection().expect("a rejection"),
    };
    let bytes = encode_frame(&reply).map_err(HandshakeError::Wire)?;
    conn.writer
        .write_all(&bytes)
        .and_then(|()| conn.writer.flush())
        .map_err(|e| HandshakeError::Wire(e.into()))?;
    match check {
        HelloCheck::Accepted(hello) => {
            let caps = hello.caps & cfg.caps;
            Ok((hello, caps))
        }
        HelloCheck::Refused => Err(HandshakeError::Refused),
        HelloCheck::WrongVersion { host_version } => {
            Err(HandshakeError::WrongVersion { host_version })
        }
    }
}

//! The first frame in each direction.

use serde::{Deserialize, Serialize};

use crate::token::{tokens_match, Token};
use crate::{SessionId, MAGIC, PROTOCOL_VERSION};

/// The first frame on every connection, host to daemon, sent on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Always [`MAGIC`].
    pub magic: [u8; 4],
    /// The host's [`PROTOCOL_VERSION`].
    pub version: u16,
    pub token: Token,
    /// Fresh for each session. The daemon echoes it in [`Welcome`].
    pub session: SessionId,
}

impl Hello {
    /// A `Hello` from this build of the host.
    pub fn new(token: Token, session: SessionId) -> Self {
        Hello {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            token,
            session,
        }
    }

    /// The daemon's answer to an accepted `Hello`.
    pub fn welcome(&self, daemon_build: String) -> Welcome {
        Welcome {
            session: self.session,
            daemon_build,
        }
    }
}

/// The daemon's first frame, answering [`Hello`]. After `Refused` the daemon
/// closes the connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelloReply {
    Welcome(Welcome),
    /// Wrong magic, token or version, or the frame was not a `Hello`.
    /// Deliberately says nothing else: anything on the desktop that can reach
    /// the tunnel can send a `Hello`.
    Refused,
}

/// The daemon accepted the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// Echoed from [`Hello::session`].
    pub session: SessionId,
    /// The daemon's build, for logs and bug reports. Free text.
    pub daemon_build: String,
}

/// Check the body of a connection's first frame: `None` means answer
/// [`HelloReply::Refused`].
///
/// Magic and token are both checked (the token in constant time) before
/// either decides anything, so a wrong magic and a wrong token take the same
/// path; the version must equal [`PROTOCOL_VERSION`] exactly. A version
/// mismatch is refused like a bad token, so a caller that has not shown the
/// token learns nothing about the daemon.
pub fn check_hello(body: &[u8], expected: &Token) -> Option<Hello> {
    let (hello, rest) = postcard::take_from_bytes::<Hello>(body).ok()?;
    let token_ok = tokens_match(&hello.token, expected);
    let accepted =
        rest.is_empty() && hello.magic == MAGIC && token_ok && hello.version == PROTOCOL_VERSION;
    accepted.then_some(hello)
}

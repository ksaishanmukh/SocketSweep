//! The first frame in each direction.

use serde::{Deserialize, Serialize};

use crate::messages::Limits;
use crate::token::{tokens_match, Token};
use crate::{SessionId, MAGIC, PROTOCOL_VERSION};

bitflags::bitflags! {
    /// What the phone's daemon can do.
    ///
    /// Capabilities describe the phone and its file system, not compatibility
    /// between releases: host and daemon ship together and their versions
    /// match exactly. The host sends every capability it understands; the
    /// daemon answers with those it also supports, and both act on that
    /// intersection only ([`Hello::welcome`]). Unknown bits survive decoding,
    /// so they can be intersected away rather than failing the handshake.
    ///
    /// Encoded as the underlying `u64`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct Caps: u64 {
        /// [`Stat::blocks`](crate::Stat::blocks) is the file system's real
        /// allocation. Without it, `blocks` is the apparent size rounded up to
        /// 4 KiB: some shared-storage file systems report 0 blocks for files
        /// that plainly occupy space.
        const DISK_USAGE = 1 << 0;
        /// Large listings arrive in several [`ScanEvent::Listing`](crate::ScanEvent::Listing)
        /// parts of at most [`MAX_LISTING_PART`](crate::MAX_LISTING_PART)
        /// entries rather than one.
        const LISTING_PARTS = 1 << 1;
        /// Skeleton listings carry names only, with the sizes following later
        /// in [`ScanEvent::Stats`](crate::ScanEvent::Stats).
        const DEFERRED_STAT = 1 << 2;
        /// [`ClientMsg::PrepareCopy`](crate::ClientMsg::PrepareCopy) is served.
        const COPY_MANIFEST = 1 << 3;
        /// [`Stat::mtime`](crate::Stat::mtime) is filled in for directories,
        /// not only files.
        const DIR_MTIME = 1 << 4;
        /// Reserved: resuming a scan over a new connection after a drop.
        const RESUME = 1 << 5;
    }
}

/// The first frame on every connection, host to daemon, sent on its own.
///
/// **The first three fields are frozen forever**: `magic`, `version` and
/// `token`, in that order and with these types. A daemon of any version can
/// therefore check the token and read the host's version even when the rest of
/// the message has changed shape; see [`HelloPrefix`] and [`check_hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Always [`MAGIC`].
    pub magic: [u8; 4],
    /// The host's [`PROTOCOL_VERSION`].
    pub version: u16,
    pub token: Token,
    /// Fresh for each session. The daemon echoes it in [`Welcome`].
    pub session: SessionId,
    /// Everything this host understands.
    pub caps: Caps,
}

impl Hello {
    /// A `Hello` from this build of the host.
    pub fn new(token: Token, session: SessionId, caps: Caps) -> Self {
        Hello {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            token,
            session,
            caps,
        }
    }

    /// The daemon's answer to an accepted `Hello`: the session echoed, and
    /// only the capabilities both ends have.
    pub fn welcome(
        &self,
        daemon_caps: Caps,
        daemon_build: String,
        workers: u8,
        limits: Limits,
    ) -> Welcome {
        Welcome {
            version: PROTOCOL_VERSION,
            session: self.session,
            caps: self.caps & daemon_caps,
            daemon_build,
            workers,
            limits,
        }
    }
}

/// The frozen leading fields of any [`Hello`], from any version.
///
/// Decode it from a `Hello` body with `postcard::take_from_bytes`, which reads
/// only these fields and ignores the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloPrefix {
    pub magic: [u8; 4],
    pub version: u16,
}

impl HelloPrefix {
    /// Read the prefix of a `Hello` frame body, whatever its version.
    pub fn peek(body: &[u8]) -> Option<HelloPrefix> {
        postcard::take_from_bytes(body).ok().map(|(p, _)| p)
    }
}

/// All the frozen fields, the token included. Private: only [`check_hello`]
/// needs the token from a `Hello` it cannot otherwise decode.
#[derive(Deserialize)]
struct HelloHead {
    magic: [u8; 4],
    version: u16,
    token: Token,
}

/// The daemon's first frame, answering [`Hello`].
///
/// **The variant order is frozen forever**, and so is the shape of `Refused`
/// and `WrongVersion`, so a host of any version can tell why it was turned
/// away. After anything but `Welcome` the daemon closes the connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelloReply {
    Welcome(Welcome),
    /// The token was wrong, or the frame was not a `Hello`. Deliberately says
    /// nothing else.
    Refused,
    /// The token was right but the version differs. Carries the daemon's
    /// version, so the host can tell a stale daemon from a newer one.
    WrongVersion {
        version: u16,
    },
}

/// The daemon accepted the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// Always [`PROTOCOL_VERSION`]. First, so its position never moves.
    pub version: u16,
    /// Echoed from [`Hello::session`].
    pub session: SessionId,
    /// The capabilities both ends have. Neither side relies on any other.
    pub caps: Caps,
    /// The daemon's build, for logs and bug reports. Free text.
    pub daemon_build: String,
    /// Worker threads a scan uses unless [`ScanOpts::workers`](crate::ScanOpts::workers)
    /// overrides it.
    pub workers: u8,
    pub limits: Limits,
}

/// What [`check_hello`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloCheck {
    /// Answer with [`Hello::welcome`].
    Accepted(Hello),
    /// Answer [`HelloReply::Refused`] and close.
    Refused,
    /// Answer [`HelloReply::WrongVersion`] and close. Carries the host's
    /// version, for the daemon's log.
    WrongVersion { host_version: u16 },
}

impl HelloCheck {
    /// The reply to send for a rejected `Hello`, or `None` if it was accepted.
    pub fn rejection(&self) -> Option<HelloReply> {
        match self {
            HelloCheck::Accepted(_) => None,
            HelloCheck::Refused => Some(HelloReply::Refused),
            HelloCheck::WrongVersion { .. } => Some(HelloReply::WrongVersion {
                version: PROTOCOL_VERSION,
            }),
        }
    }
}

/// Check the body of a connection's first frame, in a fixed order:
/// magic and token first (the token in constant time), then the version, then
/// the rest of the message.
///
/// Nothing about the daemon is revealed until the token has matched, not even
/// its version.
pub fn check_hello(body: &[u8], expected: &Token) -> HelloCheck {
    let Ok((head, _)) = postcard::take_from_bytes::<HelloHead>(body) else {
        return HelloCheck::Refused;
    };
    // Both checked before acting on either, so a wrong magic and a wrong token
    // take the same path.
    let token_ok = tokens_match(&head.token, expected);
    if head.magic != MAGIC || !token_ok {
        return HelloCheck::Refused;
    }
    if head.version != PROTOCOL_VERSION {
        return HelloCheck::WrongVersion {
            host_version: head.version,
        };
    }
    match postcard::take_from_bytes::<Hello>(body) {
        Ok((hello, [])) => HelloCheck::Accepted(hello),
        _ => HelloCheck::Refused,
    }
}

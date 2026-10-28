//! Protocol v4 between the desktop host and the phone daemon.
//!
//! Both ends depend on this crate, so there is exactly one definition of every
//! message and of the encoding, and no opportunity for the two to disagree.
//! The specification is section 2 of `docs/architecture.md`; this crate is its
//! executable form, and the two change together.
//!
//! # Framing
//!
//! Every frame is a little-endian `u32` byte length followed by that many bytes
//! of [postcard](https://docs.rs/postcard), at most [`MAX_FRAME_BYTES`]. See
//! [`write_frame`] and [`read_frame`].
//!
//! Postcard writes no field names and encodes each enum variant as its index,
//! so **the order of fields and variants is the wire format**. Golden byte
//! tests pin a representative frame of every enum, so an accidental reorder
//! fails the build rather than a phone. Daemon and host ship together and the
//! version must match exactly (see below), so the format may change freely
//! between releases as long as [`PROTOCOL_VERSION`] is bumped with it.
//!
//! # Handshake
//!
//! 1. The host opens the tunnel and sends a standalone [`Hello`] frame.
//! 2. The daemon checks, in this order: the token (in constant time), then the
//!    version, which must equal [`PROTOCOL_VERSION`] exactly, then negotiates
//!    capabilities. [`check_hello`] implements exactly this order.
//! 3. It answers with one [`HelloReply`] frame: [`HelloReply::Welcome`], after
//!    which both sides exchange [`ClientFrame`]s and [`ServerFrame`]s; or
//!    [`HelloReply::Refused`] / [`HelloReply::WrongVersion`], after which it
//!    closes the connection.
//!
//! The leading fields of [`Hello`] (`magic`, `version`, `token`) and the
//! variant order of [`HelloReply`] are **frozen forever**, so that any future
//! daemon can read an old host's version with [`HelloPrefix`], and any host can
//! read why a daemon of another version turned it away.
//!
//! A wrong token is answered with a bare `Refused` that reveals nothing else,
//! not even the daemon's version: anything on the desktop that can reach the
//! tunnel can send a `Hello`, and the command set includes recursive delete.
//!
//! The token is [`TOKEN_LEN`] random bytes the host generates per session. It
//! reaches the daemon through the [`TOKEN_ENV`] environment variable,
//! hex-encoded ([`token_to_hex`]), never through argv, where any app on the
//! phone could read it from `/proc`.
//!
//! # Requests and replies
//!
//! Every [`ClientFrame`] carries a [`ReqId`] the host chooses, never 0. Every
//! [`ServerFrame`] that answers it carries the same id; id 0
//! ([`SESSION_LEVEL`]) is for frames that belong to no request, such as
//! [`ServerMsg::Heartbeat`]. Each request ends with exactly one terminal frame:
//!
//! | Request | Replies | Ends with |
//! |---|---|---|
//! | [`ClientMsg::Ping`] | — | [`ServerMsg::Pong`] |
//! | [`ClientMsg::StartScan`] | [`ServerMsg::Scan`] batches | [`ScanEvent::Finished`] (inside a batch) |
//! | [`ClientMsg::Delete`] | [`ServerMsg::DeleteProgress`] | [`ServerMsg::Deleted`] |
//! | [`ClientMsg::PrepareCopy`] | [`ServerMsg::CopyManifest`] parts | the part with `last: true` |
//! | [`ClientMsg::Focus`], [`ClientMsg::ClearFocus`], [`ClientMsg::Cancel`] | none: hints | — |
//! | [`ClientMsg::Shutdown`] | none: the daemon exits | — |
//!
//! Any request may instead end with [`ServerMsg::Failed`]. A scan whose root
//! cannot be opened fails before [`ScanEvent::Started`]; once started, a scan
//! always ends with `Finished`, even when cancelled.
//!
//! # Ordering guarantees
//!
//! The host builds its tree straight from the event stream, with no reorder
//! buffer, because the daemon promises:
//!
//! - **G1 – introduced before referenced.** A [`DirId`] is introduced in its
//!   parent's [`ScanEvent::Listing`] (the root, [`ROOT_DIR`], in
//!   [`ScanEvent::Started`]) before any event refers to it.
//! - **G2 – fixed order per directory.** For directory D: all `Listing(D)`
//!   parts, then `Stats(D)`, then each child's `SubtreeDone`, then
//!   `SubtreeDone(D)`.
//! - **G3 – each directory is reported once.** Each `DirId` is listed once, and
//!   is finished once or [`ScanEvent::Pruned`].
//!
//! All three follow from one rule in the daemon: every event is queued while
//! holding the scheduler lock, at the moment the state changes, and a single
//! writer thread sends the queue in order. An event naming an unknown `DirId`
//! is a protocol violation, and the host fails the scan.
//!
//! # Back-pressure and keepalive
//!
//! The daemon's writer keeps two queues. Control frames (`Pong`, `Deleted`,
//! `Failed`, …) always go first. Scan events go through the bulk queue, whose
//! high- and low-water marks ([`BULK_HIGH_WATER`], [`BULK_LOW_WATER`]) stop and
//! restart workers taking new tasks; adding to it never blocks. Scan events are
//! batched into [`ServerMsg::Scan`] frames of at most [`MAX_SCAN_BATCH_BYTES`].
//!
//! The writer sends [`ServerMsg::Heartbeat`] after [`HEARTBEAT_INTERVAL`] with
//! nothing sent. The host sends [`ClientMsg::Ping`] after [`HOST_PING_INTERVAL`]
//! idle, and treats [`HOST_DEAD_AFTER`] of silence as a dead daemon; the daemon
//! gives up on a host after [`DAEMON_DEAD_AFTER`]. A lost connection cancels
//! everything the connection had started.
//!
//! # Paths are bytes, not strings
//!
//! Android file names are arbitrary byte sequences with no guarantee of being
//! UTF-8. Carrying them as `String` would corrupt them in transit, and a
//! corrupted path can arrive as a delete target. They stay `Vec<u8>` end to
//! end; conversion to text happens only for display.
//!
//! # Identity
//!
//! A [`DirId`] means something only within one scan. Path bytes are the
//! lasting identity, used by delete and copy. An id from another session is
//! refused: the session id replaces v3's tree generation check.

mod framing;
mod handshake;
mod messages;
mod token;

#[cfg(test)]
mod tests;

pub use framing::{encode_frame, read_frame, read_frame_body, write_frame, WireError};
pub use handshake::{check_hello, Caps, Hello, HelloCheck, HelloPrefix, HelloReply, Welcome};
pub use messages::{
    ClientFrame, ClientMsg, CopyItem, Entry, ErrCode, FsOp, Kind, Limits, Outcome, ScanEvent,
    ScanMode, ScanOpts, ServerFrame, ServerMsg, Stat, Tier, TierCfg, Totals,
};
pub use token::{token_from_hex, token_to_hex, tokens_match, Token, TOKEN_ENV, TOKEN_LEN};

use std::time::Duration;

/// Sent in [`Hello`] and [`Welcome`]. The daemon refuses a host of any other
/// version, so a stale daemon left on the phone fails loudly instead of
/// misreading frames. Bump it whenever the wire format changes.
pub const PROTOCOL_VERSION: u16 = 4;

/// The first four bytes of every [`Hello`], so a daemon can tell a SocketSweep
/// host from something else that connected to its socket.
pub const MAGIC: [u8; 4] = *b"SSWP";

/// A request id, chosen by the host. Never 0 on a request; see
/// [`SESSION_LEVEL`].
pub type ReqId = u32;

/// The [`ReqId`] of server frames that belong to no request, such as
/// [`ServerMsg::Heartbeat`].
pub const SESSION_LEVEL: ReqId = 0;

/// A directory within one scan, assigned by the daemon: dense, starting at
/// [`ROOT_DIR`], so the host can index a `Vec` with it.
pub type DirId = u32;

/// The [`DirId`] of the scan root, introduced by [`ScanEvent::Started`].
pub const ROOT_DIR: DirId = 0;

/// Identifies one connection's session. The host generates it at random per
/// session and the daemon echoes it in [`Welcome`].
pub type SessionId = [u8; 16];

/// Upper bound on one frame's body, so a corrupt length prefix cannot make the
/// reader allocate wildly. Listings are split into parts of
/// [`MAX_LISTING_PART`] entries and scan events batched to
/// [`MAX_SCAN_BATCH_BYTES`], so a well-behaved peer stays far below it.
pub const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;

/// Most entries in one [`ScanEvent::Listing`] part. A 60,000-file directory is
/// fifteen parts, and each one can be sent while the next is read.
pub const MAX_LISTING_PART: usize = 4096;

/// Target upper bound on the encoded size of one [`ServerMsg::Scan`] batch. A
/// single event larger than this (a full listing part of long names) is sent
/// alone rather than split.
pub const MAX_SCAN_BATCH_BYTES: usize = 64 * 1024;

/// Bulk queue size above which daemon workers stop taking new tasks.
pub const BULK_HIGH_WATER: usize = 4 * 1024 * 1024;

/// Bulk queue size below which daemon workers resume taking tasks after
/// crossing [`BULK_HIGH_WATER`].
pub const BULK_LOW_WATER: usize = 2 * 1024 * 1024;

/// The daemon's writer sends [`ServerMsg::Heartbeat`] after this long with
/// nothing sent.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// The host sends [`ClientMsg::Ping`] after this long with nothing sent.
pub const HOST_PING_INTERVAL: Duration = Duration::from_secs(5);

/// The host treats this long without any frame from the daemon as a dead
/// connection. Comfortably above [`HEARTBEAT_INTERVAL`], so only a dead or
/// wedged daemon reaches it, not a busy one.
pub const HOST_DEAD_AFTER: Duration = Duration::from_secs(15);

/// The daemon treats this long without any frame from the host as a dead
/// connection, cancels everything it started and closes it.
pub const DAEMON_DEAD_AFTER: Duration = Duration::from_secs(30);

/// A directory being read for longer than this is listed in
/// [`ScanEvent::Progress`]'s `stalled`.
pub const STALL_THRESHOLD: Duration = Duration::from_secs(3);

/// Workers check for cancellation at every task boundary and at least this
/// often, in entries, inside a task.
pub const CANCEL_CHECK_ENTRIES: usize = 256;

/// Bytes per block in [`Stat::blocks`], whatever the file system's own block
/// size: `st_blocks` is always in 512-byte units.
pub const BLOCK_SIZE: u64 = 512;

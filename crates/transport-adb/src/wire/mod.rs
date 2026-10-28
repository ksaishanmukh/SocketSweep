//! adb's wire protocols: smart sockets, shell v2 and sync.

mod shell;
mod smart_socket;
mod sync;

use std::fmt;
use std::io;

pub use shell::{read_shell_v2, SHELL_CLOSE_STDIN};
pub use smart_socket::{read_forward_reply, read_hex_string, request, send};
pub use sync::{sync_header, sync_recv, sync_send, RecvError};

/// Upper bound on a single shell-protocol packet or sync `FAIL` message, so a
/// corrupt length cannot make the host allocate gigabytes.
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

#[cfg(test)]
mod tests;

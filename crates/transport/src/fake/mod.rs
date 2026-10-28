//! An in-memory phone, for testing everything above the [`Transport`] trait.
//!
//! Enabled by the `fake` feature (and always in this crate's own tests).
//!
//! [`FakeTransport`] holds any number of [`FakeDevice`]s, each with:
//!
//! - **scripted shell responses**: handlers tried newest first, so a test can
//!   override a default; a command no handler answers exits 127 like `sh`
//!   does for an unknown command. Every command is logged.
//! - **a virtual file store** that [`Transport::push`] writes and
//!   [`Transport::pull`] reads, keyed by exact path bytes.
//! - **tunnel handlers**: [`Transport::open_tunnel`] creates an in-memory
//!   [`pipe`], returns one end and hands the other to the handler registered
//!   for that socket name, on a thread of its own. That is where a fake daemon
//!   plugs in. With no handler, the tunnel reads end of stream at once, as an
//!   adb forward to a socket nothing listens on does.
//!
//! [`FakeTransport::fail_next`] injects one failure into the next call of a
//! given kind.
//!
//! `pipe` is the in-memory duplex pipe, `state` the devices behind a
//! [`FakeTransport`], and `transport` the transport itself.

mod pipe;
mod state;
mod transport;

use std::sync::{Mutex, MutexGuard};

#[cfg(doc)]
use crate::Transport;

pub use pipe::{pipe, pipe_with_capacity, PipeEnd, DEFAULT_PIPE_CAPACITY};
pub use state::{FakeDevice, FakeFile, FakeOp};
pub use transport::FakeTransport;

/// Lock a mutex, recovering the data if a thread panicked while holding it.
/// A panicking test handler should fail that test, not every later call.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests;

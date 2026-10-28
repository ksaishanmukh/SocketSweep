//! Everything after the handshake. See [requests and replies](crate#requests-and-replies).
//!
//! Field and variant order is the wire format: add new variants at the end and
//! bump [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) with any change.
//!
//! The messages are split by direction: `client` holds what the host sends,
//! `options` how a scan is run, `server` what the daemon sends, and
//! `values` the entries, stats and totals both directions carry.

mod client;
mod options;
mod server;
mod values;

pub use client::{ClientFrame, ClientMsg};
pub use options::{Limits, ScanMode, ScanOpts, Tier, TierCfg};
pub use server::{ErrCode, FsOp, Outcome, ScanEvent, ServerFrame, ServerMsg};
pub use values::{CopyItem, Entry, Kind, Stat, Totals};

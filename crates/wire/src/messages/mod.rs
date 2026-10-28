//! Everything after the handshake.
//!
//! Field and variant order is the wire format: add new variants at the end and
//! bump [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) with any change.

mod client;
mod options;
mod server;
mod values;

pub use client::{ClientFrame, ClientMsg};
pub use options::{Limits, ScanMode, ScanOpts, Tier, TierCfg};
pub use server::{ErrCode, FsOp, Outcome, ScanEvent, ServerFrame, ServerMsg};
pub use values::{CopyItem, Entry, Kind, Stat, Totals};

//! Round trips, golden bytes, variant indices and framing edge cases.
//!
//! The golden byte tests are the guard on the wire format: they hard-code the
//! exact encoding of a frame of every message, so reordering a field or a
//! variant, or changing a type, fails here. When such a change is intended,
//! bump [`PROTOCOL_VERSION`] and update the expected bytes in the same change.

mod constants;
mod framing;
mod golden;
mod handshake;
mod indices;
mod round_trip;
mod support;
mod tokens;

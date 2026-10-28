//! The session end to end over an in-memory pipe, with the real engine on
//! `StdFs` and `LocalActions` over a temporary directory.

mod delete_and_copy;
mod handshake;
mod keepalive;
mod requests;
mod scans;
mod support;

//! The SocketSweep phone daemon (`docs/architecture.md`, sections 2 and 3).
//!
//! Pushed to `/data/local/tmp` and started over adb with the session token in
//! `SOCKETSWEEP_TOKEN`; it listens on an abstract unix socket and speaks wire
//! protocol v4 to the desktop host.
//!
//! # Modules
//!
//! | Module | Portable | Role |
//! |---|---|---|
//! | [`session`] | yes | Handshake, reader, request routing; one connection |
//! | [`outbox`] | yes | The writer thread: control queue before bulk queue, batching, heartbeat, dead-peer check; the engine's [`EventSink`](socketsweep_engine::EventSink) |
//! | [`actions`] | yes | The symlink-safe guard, delete with totals, copy manifest walk |
//! | `fs_rustix` | Linux/Android | The real [`Fs`](socketsweep_engine::Fs): `openat`, `getdents64`, `statat` |
//! | `listener` | Linux/Android | Abstract socket; an authenticated new connection replaces the old one |
//!
//! The scan itself (`sched`, `aggregate`, `pool`) is `socketsweep-engine`.
//! Everything portable runs on any host over `std::fs` ([`StdFs`](socketsweep_engine::StdFs)
//! and [`LocalActions`](actions::LocalActions)) or, in `socketsweep-sim`, over
//! an in-memory `SimFs` as `FakeDaemon`.
//!
//! # Why an abstract socket and a token
//!
//! The command set includes recursive delete, so reachability matters.
//! Loopback TCP on the phone is open to any app holding `INTERNET`; an abstract
//! socket owned by the `shell` SELinux domain is not reachable from
//! `untrusted_app`, and `adb forward localabstract:` exists for exactly this.
//! `adb forward` re-exposes it as a loopback port on the desktop, though, so
//! the daemon also refuses to start without a per-session token and serves
//! only connections whose `Hello` presents it.

pub mod actions;
pub mod outbox;
pub mod session;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod fs_rustix;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod listener;

/// The build string sent in `Welcome::daemon_build`.
pub const DAEMON_BUILD: &str = concat!("socketsweep-daemon ", env!("CARGO_PKG_VERSION"));

/// Log one line to stderr, which the host redirects to the daemon's log file
/// and quotes when the daemon never answers.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("[daemon] {}", format_args!($($arg)*))
    };
}

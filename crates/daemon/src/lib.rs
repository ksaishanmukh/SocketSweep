//! The SocketSweep phone daemon.
//!
//! Pushed to `/data/local/tmp` and started over adb with the session token in
//! `SOCKETSWEEP_TOKEN`; it listens on an abstract unix socket and speaks the
//! wire protocol to the desktop host.
//!
//! Everything except the rustix file system and the socket listener is
//! portable: it runs on any host over `std::fs`
//! ([`StdFs`](socketsweep_engine::StdFs) and
//! [`LocalActions`](actions::LocalActions)) or over an in-memory file system.
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

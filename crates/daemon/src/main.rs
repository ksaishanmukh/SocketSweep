//! SocketSweep on-device daemon.
//!
//! Pushed to `/data/local/tmp` and started over ADB. Listens on an abstract
//! unix socket and answers [`Request`](socketsweep_protocol::Request)s from the
//! desktop host.
//!
//! # Why an abstract socket rather than TCP
//!
//! The command set includes recursive delete, so reachability matters. Loopback
//! TCP on the device is open to any installed app holding `INTERNET`. Binaries
//! under `/data/local/tmp` run in the `shell` SELinux domain, and
//! `untrusted_app` is denied `unix_stream_socket connectto` into that domain, so
//! an abstract socket is not reachable the same way. `adb forward
//! localabstract:` exists for exactly this.

// Compiled on every host so its tests run during ordinary development, though
// its only caller, `serve`, is Linux-family only.
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
mod guard;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod serve;

#[cfg(any(target_os = "linux", target_os = "android"))]
fn main() -> std::process::ExitCode {
    serve::main()
}

// Abstract sockets are a Linux-family feature. Other hosts still compile this
// binary so `cargo check` and the guard tests run during development.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn main() -> std::process::ExitCode {
    eprintln!("socketsweep-daemon runs on Android/Linux only");
    std::process::ExitCode::FAILURE
}

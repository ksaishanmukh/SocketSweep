//! SocketSweep on-device daemon.
//!
//! Pushed to `/data/local/tmp` and started over ADB. Listens on an abstract
//! unix socket and answers [`Request`](socketsweep_protocol::Request)s from the
//! desktop host.
//!
//! # Why an abstract socket rather than TCP
//!
//! The previous daemon bound `127.0.0.1:5050` on the phone. Loopback TCP is
//! reachable by any installed app holding `INTERNET`, and the command set
//! includes recursive delete, so any app could wipe `/sdcard` while SocketSweep
//! was connected. Binaries under `/data/local/tmp` run in the `shell` SELinux
//! domain, and `untrusted_app` is denied `unix_stream_socket connectto` into
//! that domain — so an abstract socket is not reachable the same way. This is
//! the arrangement scrcpy uses, and it is what `adb forward localabstract:`
//! exists for.

// Compiled everywhere so its tests run on a development machine, but only
// `serve` calls it, and `serve` is Linux-family only.
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
mod guard;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod serve;

#[cfg(any(target_os = "linux", target_os = "android"))]
fn main() -> std::process::ExitCode {
    serve::main()
}

// Kept compilable on other hosts so `cargo check` and the guard tests run on a
// development machine. Abstract sockets are a Linux-family feature.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn main() -> std::process::ExitCode {
    eprintln!("socketsweep-daemon runs on Android/Linux only");
    std::process::ExitCode::FAILURE
}

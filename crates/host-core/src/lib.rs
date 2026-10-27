//! The desktop host's logic, with no Tauri dependency. See
//! `docs/architecture.md`, sections 1 and 4.
//!
//! The Tauri shell builds one [`App`] with a [`Transport`](socketsweep_transport::Transport),
//! an [`AppConfig`] (bundled daemon paths, history file, desktop folder) and an
//! event callback, exposes each [`Commands`](socketsweep_gui_contract::Commands)
//! method as a command of the same name, and emits each [`HostEvent`] under
//! [`HostEvent::name`].
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`tree`] | Index keyed by wire `DirId`; sizes and measures from the event stream; G1–G3 checks; every query |
//! | [`client`] | One connection: handshake, reader thread routing by `ReqId`, locked writes, keepalive and dead-peer detection |
//! | [`daemon`] | Root allowlist, push, launch with the token in the environment, tunnel and handshake retries, teardown |
//! | [`focus`] | First focus change at once, later ones coalesced over 150 ms, with `seq` |
//! | [`copy`] | Manifest to host-safe local paths, parallel pulls, progress, cancellation |
//! | [`history`] | Last finished scan per serial and root, written atomically |
//! | [`app`] | [`App`]: the `Commands` implementation tying them together |
//!
//! A fake daemon plugs in under a `FakeTransport`: answer the launch command
//! (parse it with [`daemon::parse_launch_command`] for the token and socket
//! name) and serve tunnels to that socket name. This crate's own tests do
//! exactly that with a scripted daemon.

pub mod app;
pub mod client;
pub mod copy;
pub mod daemon;
pub mod focus;
pub mod history;
pub mod tree;

#[cfg(test)]
mod testkit;

pub use app::{App, AppConfig, EventSink, HostEvent};
pub use client::{Client, ClientConfig, ClientError};
pub use daemon::{DaemonBinaries, LaunchOptions};

use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering the data if a thread panicked while holding it.
/// Everything behind these locks is consistent between statements, and a
/// poisoned lock would otherwise turn one bug into every later call failing.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

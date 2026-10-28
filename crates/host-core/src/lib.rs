//! The desktop host's logic, with no Tauri dependency.
//!
//! The Tauri shell builds one [`App`] with a [`Transport`](socketsweep_transport::Transport),
//! an [`AppConfig`] (bundled daemon paths, history file, desktop folder) and an
//! event callback, exposes each [`Commands`](socketsweep_gui_contract::Commands)
//! method as a command of the same name, and emits each [`HostEvent`] under
//! [`HostEvent::name`].
//!
//! A fake daemon plugs in under a `FakeTransport`: answer the launch command
//! (parse it with [`daemon::parse_launch_command`] for the token and socket
//! name) and serve tunnels to that socket name.

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

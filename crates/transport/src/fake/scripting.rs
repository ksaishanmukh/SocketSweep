//! What a test scripts on a fake device: shell answers, files and tunnels.

use std::sync::Arc;

use super::{FakeFile, FakeTransport, PipeEnd};
use crate::Output;

impl FakeTransport {
    /// Answer shell commands with `handler`, which returns `None` for
    /// commands it does not handle. Handlers are tried newest first. A
    /// handler runs without any lock held, so it may call back into the
    /// transport.
    pub fn on_shell(
        &self,
        serial: &str,
        handler: impl Fn(&str) -> Option<Output> + Send + Sync + 'static,
    ) {
        self.with_slot(serial, |slot| slot.shell.push(Arc::new(handler)));
    }

    /// Answer every command that starts with `prefix` with `output`.
    pub fn respond(&self, serial: &str, prefix: &str, output: Output) {
        let prefix = prefix.to_owned();
        self.on_shell(serial, move |command| {
            command.starts_with(&prefix).then(|| output.clone())
        });
    }

    /// Every shell command run on the device, in order.
    pub fn shell_log(&self, serial: &str) -> Vec<String> {
        self.with_slot(serial, |slot| slot.shell_log.clone())
    }

    /// Put a file in the device's store, as if it were on the phone.
    pub fn put_file(&self, serial: &str, remote: &[u8], data: Vec<u8>, mode: u32) {
        self.with_slot(serial, |slot| {
            slot.files.insert(remote.to_vec(), FakeFile { data, mode });
        });
    }

    /// A file in the device's store, if there is one at `remote`.
    pub fn file(&self, serial: &str, remote: &[u8]) -> Option<FakeFile> {
        self.with_slot(serial, |slot| slot.files.get(remote).cloned())
    }

    /// Serve tunnels to the abstract socket `name` with `handler`, which is
    /// given the daemon's end of each new tunnel on a thread of its own.
    pub fn on_tunnel(
        &self,
        serial: &str,
        name: &str,
        handler: impl Fn(PipeEnd) + Send + Sync + 'static,
    ) {
        self.with_slot(serial, |slot| {
            slot.tunnels.insert(name.into(), Arc::new(handler));
        });
    }

    /// How many tunnels to `name` have been opened.
    pub fn tunnels_opened(&self, serial: &str, name: &str) -> usize {
        self.with_slot(serial, |slot| {
            slot.tunnels_opened.get(name).copied().unwrap_or(0)
        })
    }
}

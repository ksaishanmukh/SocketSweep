//! [`FakeTransport`]: configuring fake devices, and the [`Transport`] calls.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::state::{DeviceSlot, FakeState};
use super::{lock, pipe, FakeDevice, FakeFile, FakeOp, PipeEnd};
use crate::{Device, DeviceInfo, DeviceState, Duplex, Output, Result, Transport, TransportError};

/// An in-memory [`Transport`]. See the [module documentation](super).
///
/// Every method that configures a device panics if the serial was never
/// added: that is a mistake in the test, not a condition under test.
#[derive(Default)]
pub struct FakeTransport {
    state: Mutex<FakeState>,
}

impl FakeTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`FakeTransport::add_device`].
    pub fn with_device(self, device: FakeDevice) -> Self {
        self.add_device(device);
        self
    }

    /// Attach a device, replacing any with the same serial.
    pub fn add_device(&self, device: FakeDevice) {
        let mut state = lock(&self.state);
        state.devices.retain(|d| d.device.serial != device.serial);
        state.devices.push(DeviceSlot {
            device,
            shell: Vec::new(),
            shell_log: Vec::new(),
            files: BTreeMap::new(),
            tunnels: HashMap::new(),
            tunnels_opened: HashMap::new(),
        });
    }

    /// Unplug a device, forgetting everything about it.
    pub fn remove_device(&self, serial: &str) {
        lock(&self.state)
            .devices
            .retain(|d| d.device.serial != serial);
    }

    /// Change a device's state, e.g. to simulate the user revoking
    /// authorisation.
    pub fn set_state(&self, serial: &str, new: DeviceState) {
        self.with_slot(serial, |slot| slot.device.state = new);
    }

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

    /// Make the next call of kind `op`, on any device, fail with `error`.
    /// Several can be queued; each is used once.
    pub fn fail_next(&self, op: FakeOp, error: TransportError) {
        lock(&self.state).faults.push((op, error));
    }

    fn with_slot<T>(&self, serial: &str, f: impl FnOnce(&mut DeviceSlot) -> T) -> T {
        let mut state = lock(&self.state);
        let slot = state
            .slot(serial)
            .unwrap_or_else(|| panic!("no fake device {serial} was added"));
        f(slot)
    }
}

/// Chunk size [`Transport::pull`] reports progress at, like adb's sync `DATA`.
const PULL_CHUNK: usize = 64 * 1024;

impl Transport for FakeTransport {
    fn devices(&self) -> Result<Vec<Device>> {
        let mut state = lock(&self.state);
        state.take_fault(FakeOp::Devices)?;
        Ok(state
            .devices
            .iter()
            .map(|slot| Device {
                serial: slot.device.serial.clone(),
                model: slot.device.model.clone(),
                state: slot.device.state.clone(),
                hint: slot.device.hint.clone(),
            })
            .collect())
    }

    fn device_info(&self, serial: &str) -> Result<DeviceInfo> {
        let mut state = lock(&self.state);
        state.take_fault(FakeOp::DeviceInfo)?;
        Ok(state.usable(serial)?.device.info.clone())
    }

    fn shell(&self, serial: &str, command: &str) -> Result<Output> {
        let handlers = {
            let mut state = lock(&self.state);
            state.take_fault(FakeOp::Shell)?;
            let slot = state.usable(serial)?;
            slot.shell_log.push(command.into());
            slot.shell.clone()
        };
        for handler in handlers.iter().rev() {
            if let Some(output) = handler(command) {
                return Ok(output);
            }
        }
        let program = command.split_whitespace().next().unwrap_or("");
        Ok(Output {
            stdout: Vec::new(),
            stderr: format!("/system/bin/sh: {program}: inaccessible or not found\n").into_bytes(),
            exit: Some(127),
        })
    }

    fn push(&self, serial: &str, local: &Path, remote: &str, mode: u32) -> Result<()> {
        lock(&self.state).take_fault(FakeOp::Push)?;
        let data = std::fs::read(local).map_err(|source| TransportError::Local {
            path: local.to_path_buf(),
            source,
        })?;
        let mut state = lock(&self.state);
        let slot = state.usable(serial)?;
        slot.files
            .insert(remote.as_bytes().to_vec(), FakeFile { data, mode });
        Ok(())
    }

    fn pull(
        &self,
        serial: &str,
        remote: &[u8],
        local: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<u64> {
        let data = {
            let mut state = lock(&self.state);
            state.take_fault(FakeOp::Pull)?;
            let slot = state.usable(serial)?;
            match slot.files.get(remote) {
                Some(file) => file.data.clone(),
                None => {
                    return Err(TransportError::Refused(format!(
                        "remote object '{}' does not exist",
                        String::from_utf8_lossy(remote)
                    )))
                }
            }
        };

        let local_err = |source| TransportError::Local {
            path: local.to_path_buf(),
            source,
        };
        let mut file = std::fs::File::create(local).map_err(local_err)?;
        let mut done = 0u64;
        for chunk in data.chunks(PULL_CHUNK) {
            file.write_all(chunk).map_err(local_err)?;
            done += chunk.len() as u64;
            progress(done);
        }
        file.flush().map_err(local_err)?;
        Ok(done)
    }

    fn open_tunnel(&self, serial: &str, name: &str) -> Result<Box<dyn Duplex>> {
        let handler = {
            let mut state = lock(&self.state);
            state.take_fault(FakeOp::OpenTunnel)?;
            let slot = state.usable(serial)?;
            *slot.tunnels_opened.entry(name.into()).or_insert(0) += 1;
            slot.tunnels.get(name).cloned()
        };

        let (near, far) = pipe();
        match handler {
            Some(handler) => {
                std::thread::Builder::new()
                    .name(format!("fake-tunnel-{name}"))
                    .spawn(move || handler(far))
                    .map_err(TransportError::Io)?;
            }
            // Nothing listening: the far end closes at once.
            None => drop(far),
        }
        Ok(Box::new(near))
    }
}

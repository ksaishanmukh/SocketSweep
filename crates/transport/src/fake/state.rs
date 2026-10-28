//! The devices behind a [`FakeTransport`](super::FakeTransport): their
//! shell handlers, files and tunnels, and injected faults.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[cfg(doc)]
use super::FakeTransport;
use super::PipeEnd;
use crate::{DeviceInfo, DeviceState, Output, Result, TransportError};

// ── Fake transport ──────────────────────────────────────────────────────────

/// A device to add to a [`FakeTransport`].
#[derive(Debug, Clone)]
pub struct FakeDevice {
    pub serial: String,
    pub model: String,
    pub state: DeviceState,
    pub hint: Option<String>,
    pub info: DeviceInfo,
}

impl FakeDevice {
    /// A ready, authorised arm64 phone on Android 14.
    pub fn ready(serial: &str) -> Self {
        FakeDevice {
            serial: serial.into(),
            model: "Fake Phone".into(),
            state: DeviceState::Ready,
            hint: None,
            info: DeviceInfo {
                sdk: 34,
                abi: "arm64-v8a".into(),
                model: "Fake Phone".into(),
            },
        }
    }
}

/// A file in a fake device's store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeFile {
    pub data: Vec<u8>,
    pub mode: u32,
}

/// The kinds of call [`FakeTransport::fail_next`] can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FakeOp {
    Devices,
    DeviceInfo,
    Shell,
    Push,
    Pull,
    OpenTunnel,
}

pub(super) type ShellHandler = Arc<dyn Fn(&str) -> Option<Output> + Send + Sync>;
pub(super) type TunnelHandler = Arc<dyn Fn(PipeEnd) + Send + Sync>;

pub(super) struct DeviceSlot {
    pub(super) device: FakeDevice,
    pub(super) shell: Vec<ShellHandler>,
    pub(super) shell_log: Vec<String>,
    pub(super) files: BTreeMap<Vec<u8>, FakeFile>,
    pub(super) tunnels: HashMap<String, TunnelHandler>,
    pub(super) tunnels_opened: HashMap<String, usize>,
}

#[derive(Default)]
pub(super) struct FakeState {
    pub(super) devices: Vec<DeviceSlot>,
    pub(super) faults: Vec<(FakeOp, TransportError)>,
}

impl FakeState {
    pub(super) fn slot(&mut self, serial: &str) -> Option<&mut DeviceSlot> {
        self.devices.iter_mut().find(|d| d.device.serial == serial)
    }

    /// The device, if it is attached and usable, as the real transport
    /// requires for every per-device call.
    pub(super) fn usable(&mut self, serial: &str) -> Result<&mut DeviceSlot> {
        let slot = self.slot(serial).ok_or_else(|| TransportError::NotFound {
            serial: serial.into(),
        })?;
        if slot.device.state != DeviceState::Ready {
            return Err(TransportError::Unusable {
                serial: serial.into(),
                state: slot.device.state.clone(),
                hint: slot.device.hint.clone(),
            });
        }
        Ok(slot)
    }

    pub(super) fn take_fault(&mut self, op: FakeOp) -> Result<()> {
        match self.faults.iter().position(|(o, _)| *o == op) {
            Some(i) => Err(self.faults.remove(i).1),
            None => Ok(()),
        }
    }
}

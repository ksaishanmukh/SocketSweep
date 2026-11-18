//! What the host knows about an attached phone.

/// One attached device, usable or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    /// Human-readable model, for the picker. Falls back to the serial.
    pub model: String,
    pub state: DeviceState,
    /// What the user should do about a device that is not usable, when the
    /// transport knows (a missing udev rule, say).
    pub hint: Option<String>,
}

impl Device {
    /// Only a device in [`DeviceState::Ready`] can be used.
    pub fn usable(&self) -> bool {
        self.state == DeviceState::Ready
    }
}

/// A device's connection state, as adb reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// Connected and authorised (adb's `device`).
    Ready,
    /// The user has not accepted the debugging prompt on the phone.
    Unauthorized,
    /// Visible but not responding; reconnecting usually fixes it.
    Offline,
    /// The host OS will not let adb open it (Linux without a udev rule).
    NoPermission,
    /// Anything else, as reported, so a new state is shown rather than hidden.
    Other(String),
}

/// Facts about a phone that decide which daemon build it can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Android API level, from `ro.build.version.sdk`.
    pub sdk: u32,
    /// Primary ABI, from `ro.product.cpu.abi`, e.g. `arm64-v8a`.
    pub abi: String,
    /// `ro.product.model`, e.g. `Pixel 8`.
    pub model: String,
}

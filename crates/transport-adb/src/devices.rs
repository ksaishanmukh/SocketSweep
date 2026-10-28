//! Parsing `host:devices-l` and `getprop` output.

use socketsweep_transport::{Device, DeviceInfo, DeviceState};

/// Shown for devices Linux will not let adb open.
pub const UDEV_HINT: &str = "Linux is blocking access to this USB device. Add a udev rule for it \
     (see https://developer.android.com/studio/run/device), make sure your user is in the \
     plugdev group, then unplug and reconnect the phone.";

/// One line of `adb devices -l`, before it is dressed up for the UI.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeviceLine {
    pub serial: String,
    /// As adb printed it, possibly several words long.
    pub state: String,
    pub model: Option<String>,
    // Not shown in the UI, but they identify a device in a bug report, so they
    // are parsed rather than discarded.
    #[allow(dead_code)]
    pub product: Option<String>,
    #[allow(dead_code)]
    pub device: Option<String>,
    #[allow(dead_code)]
    pub transport_id: Option<u32>,
}

impl DeviceLine {
    pub fn into_device(self) -> Device {
        let state = normalise_state(&self.state);
        // adb reports models as e.g. "SM_S928B"; underscores read badly in a
        // device picker. "Unk" is a placeholder some tools use for a missing
        // model.
        let model = match self.model.as_deref() {
            Some(m) if !m.is_empty() && m != "Unk" => m.replace('_', " "),
            _ => self.serial.clone(),
        };
        let hint = match state.as_str() {
            "unauthorized" => {
                Some("Accept the \u{201c}Allow USB debugging\u{201d} prompt on the phone.".into())
            }
            "offline" => Some("Unplug and reconnect the cable.".into()),
            "authorizing" => Some("Waiting for the phone to authorise this computer.".into()),
            "noperm" => Some(UDEV_HINT.into()),
            _ => None,
        };
        let state = match state.as_str() {
            "device" => DeviceState::Ready,
            "unauthorized" => DeviceState::Unauthorized,
            "offline" => DeviceState::Offline,
            "noperm" => DeviceState::NoPermission,
            _ => DeviceState::Other(state),
        };
        Device {
            serial: self.serial,
            model,
            state,
            hint,
        }
    }
}

/// adb's states are single lowercase words except Linux's permission failure,
/// which is a sentence: "no permissions (missing udev rules? …); see [url]".
fn normalise_state(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("no permissions") || lower == "noperm" {
        "noperm".into()
    } else {
        lower
    }
}

/// Parse the body of `host:devices-l`. Lines that cannot be read are skipped
/// rather than failing the listing.
pub fn parse_devices_long(text: &str) -> Vec<DeviceLine> {
    text.lines().filter_map(parse_device_line).collect()
}

/// `<serial> <state…> [key:value …]`, e.g.
/// `R5CX1234 device usb:1-1 product:e3qxeea model:SM_S928B device:e3q transport_id:3`.
/// A wireless serial is `host:port`; it is the first token, so its colon never
/// looks like a field.
fn parse_device_line(line: &str) -> Option<DeviceLine> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("List of devices") || line.starts_with('*') {
        return None;
    }
    let mut tokens = line.split_whitespace();
    let serial = tokens.next()?.to_string();
    let rest: Vec<&str> = tokens.collect();

    // The state runs up to the first key:value field. A multi-word state may
    // contain colons ("see [http://…]"), but never a bare `word:value` token.
    let fields_at = rest
        .iter()
        .position(|t| as_field(t).is_some())
        .unwrap_or(rest.len());
    let state = rest[..fields_at].join(" ");
    if state.is_empty() {
        return None;
    }

    let mut parsed = DeviceLine {
        serial,
        state,
        ..DeviceLine::default()
    };
    for (key, value) in rest[fields_at..].iter().filter_map(|t| as_field(t)) {
        match key {
            "model" => parsed.model = Some(value.to_string()),
            "product" => parsed.product = Some(value.to_string()),
            "device" => parsed.device = Some(value.to_string()),
            "transport_id" => parsed.transport_id = value.parse().ok(),
            _ => {}
        }
    }
    Some(parsed)
}

/// `key:value` where the key is a plain identifier and the value is non-empty.
fn as_field(token: &str) -> Option<(&str, &str)> {
    let (key, value) = token.split_once(':')?;
    let plain = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    (plain && !value.is_empty()).then_some((key, value))
}

/// Three lines: sdk, abi, model. A missing model falls back to the serial.
pub fn parse_device_info(getprop: &str, serial: &str) -> Option<DeviceInfo> {
    let mut lines = getprop.lines().map(str::trim);
    let sdk = lines.next()?.parse().ok()?;
    let abi = lines.next()?.to_string();
    let model = lines
        .next()
        .filter(|m| !m.is_empty())
        .unwrap_or(serial)
        .to_string();
    (!abi.is_empty()).then_some(DeviceInfo { sdk, abi, model })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices(text: &str) -> Vec<Device> {
        parse_devices_long(text)
            .into_iter()
            .map(DeviceLine::into_device)
            .collect()
    }

    #[test]
    fn parses_a_ready_device_with_all_fields() {
        let lines = parse_devices_long(
            "R5CX1234ABC            device usb:1-1 product:e3qxeea model:SM_S928B device:e3q transport_id:3\n",
        );
        assert_eq!(
            lines,
            vec![DeviceLine {
                serial: "R5CX1234ABC".into(),
                state: "device".into(),
                model: Some("SM_S928B".into()),
                product: Some("e3qxeea".into()),
                device: Some("e3q".into()),
                transport_id: Some(3),
            }]
        );
        let d = &devices("R5CX1234ABC device usb:1-1 model:SM_S928B transport_id:3")[0];
        assert!(d.usable());
        assert_eq!(d.model, "SM S928B");
        assert_eq!(d.hint, None);
    }

    #[test]
    fn no_permissions_becomes_noperm_with_a_udev_hint() {
        let text = "0123456789ABCDEF       no permissions (missing udev rules? user is in the plugdev group); see [http://developer.android.com/tools/device.html] usb:1-4 transport_id:7\n\
                    R5CX1234ABC            device usb:1-1 model:Pixel_8 transport_id:3\n";
        let list = devices(text);
        assert_eq!(list.len(), 2, "one odd line must not hide the others");
        assert_eq!(list[0].serial, "0123456789ABCDEF");
        assert_eq!(list[0].state, DeviceState::NoPermission);
        assert!(!list[0].usable());
        assert_eq!(list[0].model, "0123456789ABCDEF");
        assert!(list[0].hint.as_deref().unwrap().contains("udev"));
        assert_eq!(parse_devices_long(text)[0].transport_id, Some(7));
        assert!(list[1].usable());
        assert_eq!(list[1].model, "Pixel 8");
    }

    #[test]
    fn unauthorized_and_offline_are_listed_but_unusable() {
        let list = devices(
            "emulator-5554 offline transport_id:1\nR5CX1234ABC\tunauthorized usb:1-1 transport_id:2\n",
        );
        assert_eq!(list[0].state, DeviceState::Offline);
        assert!(!list[0].usable());
        assert_eq!(list[1].state, DeviceState::Unauthorized);
        assert!(!list[1].usable());
        assert_eq!(list[1].model, "R5CX1234ABC");
        assert!(list[1].hint.is_some());
    }

    #[test]
    fn placeholder_or_empty_model_falls_back_to_serial() {
        assert_eq!(
            devices("abc device model:Unk transport_id:1")[0].model,
            "abc"
        );
        // `model:` with no value is not a field at all.
        assert_eq!(devices("abc device model: transport_id:1")[0].model, "abc");
    }

    #[test]
    fn unknown_states_and_fields_pass_through() {
        let lines = parse_devices_long(
            "192.168.1.5:5555 sideload product:x model:Y device:z features:shell_v2,cmd transport_id:9\nonlyserial\n\n",
        );
        assert_eq!(lines.len(), 1, "a line with no state is skipped");
        assert_eq!(lines[0].serial, "192.168.1.5:5555");
        assert_eq!(lines[0].state, "sideload");
        assert_eq!(lines[0].transport_id, Some(9));
        let d = &devices("x Recovery transport_id:1")[0];
        assert_eq!(d.state, DeviceState::Other("recovery".into()));
        assert!(!d.usable());
    }

    #[test]
    fn wireless_serials_are_ordinary_serials() {
        let list = devices(
            "192.168.1.5:5555      device product:husky model:Pixel_8 device:husky transport_id:4\n\
             [fe80::1]:37099       offline transport_id:5\n\
             adb-R5CX1234-AbCdEf._adb-tls-connect._tcp device model:SM_S928B transport_id:6\n\
             10.0.0.9:5555 unauthorized transport_id:8\n",
        );
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].serial, "192.168.1.5:5555");
        assert!(list[0].usable());
        assert_eq!(list[0].model, "Pixel 8");
        assert_eq!(list[1].serial, "[fe80::1]:37099");
        assert_eq!(list[1].state, DeviceState::Offline);
        assert_eq!(list[1].model, "[fe80::1]:37099");
        assert_eq!(list[2].serial, "adb-R5CX1234-AbCdEf._adb-tls-connect._tcp");
        assert!(list[2].usable());
        assert_eq!(list[3].serial, "10.0.0.9:5555");
        assert_eq!(list[3].state, DeviceState::Unauthorized);
    }

    #[test]
    fn empty_listing_is_no_devices() {
        assert!(parse_devices_long("").is_empty());
    }

    #[test]
    fn device_info_reads_sdk_abi_then_model() {
        assert_eq!(
            parse_device_info("34\r\narm64-v8a\r\nPixel 8\r\n", "s"),
            Some(DeviceInfo {
                sdk: 34,
                abi: "arm64-v8a".into(),
                model: "Pixel 8".into()
            })
        );
        assert_eq!(
            parse_device_info("34\narm64-v8a\n\n", "s").unwrap().model,
            "s"
        );
        assert_eq!(parse_device_info("\narm64-v8a\n", "s"), None);
        assert_eq!(parse_device_info("34\n\n", "s"), None);
    }
}

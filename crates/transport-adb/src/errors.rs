//! Mapping adb's failures to [`TransportError`]s.

use std::io;

use socketsweep_transport::{DeviceState, TransportError};

use crate::wire::WireError;

/// A host-service `FAIL`, as the transport's error. adb words these as
/// "device 'X' not found", "device unauthorized…", "device offline".
pub(crate) fn host_fail(serial: Option<&str>, msg: &str) -> TransportError {
    let lower = msg.to_ascii_lowercase();
    match serial {
        Some(serial) if lower.contains("not found") => TransportError::NotFound {
            serial: serial.to_string(),
        },
        Some(serial) if lower.contains("unauthorized") => TransportError::Unusable {
            serial: serial.to_string(),
            state: DeviceState::Unauthorized,
            hint: None,
        },
        Some(serial) if lower.contains("offline") => TransportError::Unusable {
            serial: serial.to_string(),
            state: DeviceState::Offline,
            hint: None,
        },
        _ => TransportError::Refused(msg.to_string()),
    }
}

/// A wire failure of some request, with what was being attempted.
pub(crate) fn wire_to_error(e: WireError, what: &str) -> TransportError {
    match e {
        WireError::Fail(msg) => TransportError::Refused(format!("{what}: {msg}")),
        WireError::Bad(msg) => TransportError::Protocol(format!("{what}: {msg}")),
        WireError::Io(e) => match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                TransportError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{what}: {}", WireError::Io(e)),
                ))
            }
            _ => TransportError::Io(e),
        },
    }
}

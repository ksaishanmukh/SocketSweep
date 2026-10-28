//! Listing phones, choosing one, and connecting to its daemon.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use socketsweep_gui_contract::{Connected, Device, DeviceState, ErrorCode};
use socketsweep_transport::{self as transport, Transport, TransportError};

use super::errors::{err, launch_err, transport_err, CmdResult};
use super::state::{Busy, Conn, Inner};
use crate::daemon::{check_root, Daemon, DEFAULT_ROOT};
use crate::lock;

pub(super) fn list_devices(inner: &Inner) -> CmdResult<Vec<Device>> {
    let devices = inner.transport()?.devices().map_err(transport_err)?;
    Ok(devices
        .into_iter()
        .map(|d| Device {
            usable: d.usable(),
            hint: d.hint.clone().or_else(|| match &d.state {
                transport::DeviceState::Unauthorized => Some(
                    "Accept the \u{201c}Allow USB debugging\u{201d} prompt on the phone.".into(),
                ),
                transport::DeviceState::Offline => Some("Unplug and reconnect the phone.".into()),
                _ => None,
            }),
            state: device_state(&d.state),
            model: d.model,
            serial: d.serial,
        })
        .collect())
}

pub(super) fn connect(
    inner: &Arc<Inner>,
    serial: Option<String>,
    root: Option<String>,
) -> CmdResult<Connected> {
    let _busy = inner.claim(Busy::Connecting)?;
    let root = root.unwrap_or_else(|| DEFAULT_ROOT.into());
    check_root(&root).map_err(launch_err)?;
    let device = resolve_serial(inner, serial)?;
    let transport = inner.transport()?;

    // The old phone's session goes first, so a failed reconnect is not
    // left serving it.
    inner.end_session();
    let epoch = inner.epoch.load(Ordering::SeqCst);

    let daemon = Daemon::start(
        transport.as_ref(),
        &device.serial,
        &root,
        &inner.config.daemon,
        &inner.config.launch,
    )
    .map_err(launch_err)?;

    let weak: Weak<Inner> = Arc::downgrade(inner);
    daemon.client.set_on_close(Box::new(move |why| {
        if let Some(inner) = weak.upgrade() {
            inner.connection_lost(epoch, why);
        }
    }));

    install(inner, daemon, &device, epoch, transport.as_ref())?;
    let conn = inner.conn()?;
    if !conn.daemon.client.is_open() {
        // Lost before the callback was in place: it has already run.
        return Err(err(ErrorCode::Device, "The phone disconnected."));
    }
    let previous = inner
        .history
        .as_ref()
        .and_then(|h| h.previous(&device.serial, &root));
    Ok(Connected {
        serial: device.serial,
        model: conn.model.clone(),
        root,
        copy_destination: inner
            .config
            .default_destination
            .as_ref()
            .map(|d| d.display().to_string()),
        previous,
    })
}

/// Make `daemon` the connection, unless a disconnect since `epoch` means the
/// connect was abandoned.
fn install(
    inner: &Inner,
    daemon: Daemon,
    device: &transport::Device,
    epoch: u64,
    transport: &dyn Transport,
) -> CmdResult<()> {
    let mut slot = lock(&inner.conn);
    if inner.epoch.load(Ordering::SeqCst) != epoch {
        drop(slot);
        daemon.stop(transport);
        return Err(err(ErrorCode::Cancelled, "Disconnected while connecting."));
    }
    let model = if daemon.info.model.is_empty() {
        device.model.clone()
    } else {
        daemon.info.model.clone()
    };
    *slot = Some(Arc::new(Conn {
        daemon,
        model,
        epoch,
    }));
    Ok(())
}

fn resolve_serial(inner: &Inner, serial: Option<String>) -> CmdResult<transport::Device> {
    let devices = inner.transport()?.devices().map_err(transport_err)?;
    let device = match serial {
        Some(serial) => devices
            .into_iter()
            .find(|d| d.serial == serial)
            .ok_or_else(|| transport_err(TransportError::NotFound { serial }))?,
        None => {
            let mut usable: Vec<_> = devices.iter().filter(|d| d.usable()).cloned().collect();
            match usable.len() {
                1 => usable.remove(0),
                // None usable: report why the first one is not.
                0 => devices
                    .into_iter()
                    .next()
                    .ok_or_else(|| transport_err(TransportError::NoDevices))?,
                _ => {
                    return Err(err(
                        ErrorCode::Invalid,
                        "Several phones are connected. Choose one.",
                    ))
                }
            }
        }
    };
    if !device.usable() {
        return Err(transport_err(TransportError::Unusable {
            serial: device.serial,
            state: device.state,
            hint: device.hint,
        }));
    }
    Ok(device)
}

fn device_state(state: &transport::DeviceState) -> DeviceState {
    match state {
        transport::DeviceState::Ready => DeviceState::Ready,
        transport::DeviceState::Unauthorized => DeviceState::Unauthorized,
        transport::DeviceState::Offline => DeviceState::Offline,
        transport::DeviceState::NoPermission => DeviceState::NoPermission,
        transport::DeviceState::Other(_) => DeviceState::Other,
    }
}

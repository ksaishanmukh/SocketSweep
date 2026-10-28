//! The [`Transport`] calls, over the adb server.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use socketsweep_transport::{
    Device, DeviceInfo, Duplex, Output, Result, Transport, TransportError,
};

use crate::devices::{parse_device_info, parse_devices_long, DeviceLine};
use crate::errors::wire_to_error;
use crate::wire::{
    read_shell_v2, request, sync_header, sync_recv, sync_send, RecvError, WireError,
    SHELL_CLOSE_STDIN,
};
use crate::{AdbTransport, IO_TIMEOUT, PULL_TIMEOUT, PUSH_TIMEOUT};

impl Transport for AdbTransport {
    fn devices(&self) -> Result<Vec<Device>> {
        let listing = self.host_query("host:devices-l")?;
        Ok(parse_devices_long(&listing)
            .into_iter()
            .map(DeviceLine::into_device)
            .collect())
    }

    fn device_info(&self, serial: &str) -> Result<DeviceInfo> {
        let out = self.shell(
            serial,
            "getprop ro.build.version.sdk; getprop ro.product.cpu.abi; getprop ro.product.model",
        )?;
        let text = out.stdout_text();
        parse_device_info(&text, serial).ok_or_else(|| {
            TransportError::Protocol(format!(
                "device {serial} did not report its Android version and CPU type (got {:?})",
                text.trim()
            ))
        })
    }

    fn shell(&self, serial: &str, command: &str) -> Result<Output> {
        let fail = |e: WireError| wire_to_error(e, &format!("adb shell {command}"));

        // Shell protocol v2 (Android 7+) carries stdout, stderr and the exit
        // code separately. Older phones reject it, and fall back to the raw
        // stream, which carries stdout and stderr mixed and no exit code.
        let mut stream = self.transport(serial, IO_TIMEOUT)?;
        match request(&mut stream, &format!("shell,v2,raw:{command}")) {
            Ok(()) => {
                // Nothing is ever sent on stdin; closing it stops a command
                // that reads stdin from waiting on us.
                stream
                    .write_all(&[SHELL_CLOSE_STDIN, 0, 0, 0, 0])
                    .map_err(|e| fail(e.into()))?;
                let out = read_shell_v2(&mut stream).map_err(fail)?;
                Ok(Output {
                    stdout: out.stdout,
                    stderr: out.stderr,
                    exit: out.exit.map(i32::from),
                })
            }
            Err(WireError::Fail(_)) => {
                let mut stream = self.transport(serial, IO_TIMEOUT)?;
                request(&mut stream, &format!("shell:{command}")).map_err(fail)?;
                let mut stdout = Vec::new();
                stream
                    .read_to_end(&mut stdout)
                    .map_err(|e| fail(e.into()))?;
                Ok(Output {
                    stdout,
                    stderr: Vec::new(),
                    exit: None,
                })
            }
            Err(e) => Err(fail(e)),
        }
    }

    fn push(&self, serial: &str, local: &Path, remote: &str, mode: u32) -> Result<()> {
        let mut file = File::open(local).map_err(|source| TransportError::Local {
            path: local.to_path_buf(),
            source,
        })?;
        let fail = |e: WireError| wire_to_error(e, &format!("Cannot push to {remote}"));

        let mut stream = self.transport(serial, PUSH_TIMEOUT)?;
        request(&mut stream, "sync:").map_err(fail)?;
        sync_send(&mut stream, &mut file, remote, mode).map_err(fail)?;
        // Ends the sync session cleanly; the file is already on the phone.
        let _ = stream.write_all(&sync_header(b"QUIT", 0));
        Ok(())
    }

    fn pull(
        &self,
        serial: &str,
        remote: &[u8],
        local: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<u64> {
        let shown = String::from_utf8_lossy(remote).into_owned();
        let v2 = self.has_sendrecv_v2(serial);
        let mut stream = self.transport(serial, PULL_TIMEOUT)?;
        request(&mut stream, "sync:")
            .map_err(|e| wire_to_error(e, &format!("Cannot pull {shown}")))?;

        let local_err = |source: io::Error| TransportError::Local {
            path: local.to_path_buf(),
            source,
        };
        let mut part = local.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let mut file = File::create(&part).map_err(local_err)?;

        let result = sync_recv(&mut stream, remote, v2, &mut file, progress)
            .map_err(|e| match e {
                RecvError::Wire(w) => wire_to_error(w, &format!("Cannot pull {shown}")),
                RecvError::Sink(source) => local_err(source),
            })
            .and_then(|(n, mtime)| file.flush().map(|()| (n, mtime)).map_err(local_err));
        drop(file);
        match result {
            Ok((n, mtime)) => {
                if let Err(e) = fs::rename(&part, local) {
                    let _ = fs::remove_file(&part);
                    return Err(local_err(e));
                }
                if mtime > 0 {
                    // Best effort: a copy with the wrong mtime is still a copy.
                    if let Ok(f) = File::options().write(true).open(local) {
                        let _ = f.set_modified(UNIX_EPOCH + Duration::from_secs(u64::from(mtime)));
                    }
                }
                let _ = stream.write_all(&sync_header(b"QUIT", 0));
                Ok(n)
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                Err(e)
            }
        }
    }

    fn open_tunnel(&self, serial: &str, name: &str) -> Result<Box<dyn Duplex>> {
        let mut stream = self.transport(serial, IO_TIMEOUT)?;
        request(&mut stream, &format!("localabstract:{name}"))
            .map_err(|e| wire_to_error(e, &format!("Cannot reach the daemon socket {name}")))?;
        // From here the connection is the daemon's byte stream, and the caller
        // owns its timeouts: blocking until told otherwise.
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        Ok(Box::new(stream))
    }
}

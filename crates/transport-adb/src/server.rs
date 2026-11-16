//! Talking to the adb server: connections, host queries, and
//! starting the bundled server when none is listening.

use std::io;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use socketsweep_transport::{Result, TransportError};

use crate::errors::{host_fail, wire_to_error};
use crate::wire::{read_hex_string, request, WireError};
use crate::{
    AdbTransport, CONNECT_DEVICE_TIMEOUT, CONNECT_TIMEOUT, IO_TIMEOUT, START_SERVER_TIMEOUT,
};

#[cfg(doc)]
use socketsweep_transport::Transport;

impl AdbTransport {
    /// `adb connect <addr>`: attach a wireless device (`host:port`). Afterwards
    /// the device is listed with that address as its serial.
    pub fn connect_device(&self, addr: &str) -> Result<()> {
        let mut stream = self.open(CONNECT_DEVICE_TIMEOUT)?;
        let msg = request(&mut stream, &format!("host:connect:{addr}"))
            .and_then(|()| read_hex_string(&mut stream))
            .map_err(|e| wire_to_error(e, &format!("Cannot connect to {addr}")))?;
        if msg.starts_with("connected to") || msg.starts_with("already connected to") {
            Ok(())
        } else {
            Err(TransportError::Refused(msg.trim().to_string()))
        }
    }

    /// A host request answered by one length-prefixed string.
    pub(super) fn host_query(&self, service: &str) -> Result<String> {
        let mut stream = self.open(IO_TIMEOUT)?;
        request(&mut stream, service)
            .and_then(|()| read_hex_string(&mut stream))
            .map_err(|e| match e {
                WireError::Fail(msg) => host_fail(None, &msg),
                e => TransportError::Server(format!("{service}: {e}")),
            })
    }

    /// A connection bound to one device, ready for a device service request.
    pub(super) fn transport(&self, serial: &str, timeout: Duration) -> Result<TcpStream> {
        let mut stream = self.open(timeout)?;
        request(&mut stream, &format!("host:transport:{serial}")).map_err(|e| match e {
            WireError::Fail(msg) => host_fail(Some(serial), &msg),
            e => TransportError::Server(e.to_string()),
        })?;
        Ok(stream)
    }

    /// A fresh connection to the adb server, starting the bundled one first if
    /// and only if nothing is listening.
    fn open(&self, timeout: Duration) -> Result<TcpStream> {
        let stream = match TcpStream::connect_timeout(&self.server, CONNECT_TIMEOUT) {
            Ok(stream) => stream,
            Err(_) => {
                self.start_server()?;
                TcpStream::connect_timeout(&self.server, CONNECT_TIMEOUT).map_err(|e| {
                    TransportError::Server(format!("started adb but cannot reach it: {e}"))
                })?
            }
        };
        let setup = || -> io::Result<()> {
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.set_nodelay(true)
        };
        setup().map_err(|e| TransportError::Server(e.to_string()))?;
        Ok(stream)
    }

    fn start_server(&self) -> Result<()> {
        let mut command = Command::new(&self.adb_binary);
        command
            .arg("start-server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|e| {
            TransportError::Server(format!("cannot run {}: {e}", self.adb_binary.display()))
        })?;
        let deadline = Instant::now() + START_SERVER_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(TransportError::Server(format!(
                        "adb start-server failed ({status})"
                    )))
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TransportError::Server("adb start-server timed out".into()));
                }
                Err(e) => return Err(TransportError::Server(format!("adb start-server: {e}"))),
            }
        }
    }
}

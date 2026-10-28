//! Shell protocol v2: stdout, stderr and the exit code, separately.

use std::io::{self, Read};

use super::{Wire, WireError, MAX_PACKET};

pub const SHELL_STDOUT: u8 = 1;
pub const SHELL_STDERR: u8 = 2;
pub const SHELL_EXIT: u8 = 3;
pub const SHELL_CLOSE_STDIN: u8 = 4;

pub struct ShellOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// None when the connection closed without reporting one.
    pub exit: Option<u8>,
}

/// Shell protocol v2: packets of a 1-byte stream id, a little-endian u32
/// length and the payload, ending with an exit-code packet.
pub fn read_shell_v2<R: Read>(r: &mut R) -> Wire<ShellOutput> {
    let mut out = ShellOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        exit: None,
    };
    loop {
        let mut header = [0u8; 5];
        match r.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(out),
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_PACKET {
            return Err(WireError::Bad(format!("shell packet of {len} bytes")));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        match header[0] {
            SHELL_STDOUT => out.stdout.extend_from_slice(&payload),
            SHELL_STDERR => out.stderr.extend_from_slice(&payload),
            SHELL_EXIT => {
                out.exit = payload.first().copied();
                return Ok(out);
            }
            _ => {}
        }
    }
}

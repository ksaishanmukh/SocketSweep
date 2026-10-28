//! Smart-socket framing: hex-length requests, `OKAY`/`FAIL` statuses, and
//! length-prefixed strings.

use std::io::{Read, Write};

use super::{Wire, WireError};

/// Send a smart-socket request: 4 hex digits of length, then the request.
pub fn send<W: Write>(w: &mut W, service: &str) -> Wire<()> {
    if service.len() > 0xffff {
        return Err(WireError::Bad(format!(
            "request too long ({} bytes)",
            service.len()
        )));
    }
    w.write_all(format!("{:04x}{service}", service.len()).as_bytes())?;
    Ok(())
}

/// `OKAY`, or `FAIL` followed by a length-prefixed reason.
pub fn read_status<R: Read>(r: &mut R) -> Wire<()> {
    let mut status = [0u8; 4];
    r.read_exact(&mut status)?;
    match &status {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(WireError::Fail(read_hex_string(r)?)),
        other => Err(WireError::Bad(String::from_utf8_lossy(other).into_owned())),
    }
}

pub fn request<S: Read + Write>(s: &mut S, service: &str) -> Wire<()> {
    send(s, service)?;
    read_status(s)
}

/// 4 hex digits of length, then that many bytes.
pub fn read_hex_string<R: Read>(r: &mut R) -> Wire<String> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = std::str::from_utf8(&len)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .ok_or_else(|| WireError::Bad(format!("bad length {:?}", String::from_utf8_lossy(&len))))?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The reply to `host-serial:…:forward:` and `…:killforward:`. The server
/// answers `OKAY` once for accepting the request and again for the result, and
/// for `tcp:0` follows with the chosen port as a length-prefixed string. A
/// refusal is a `FAIL` in place of either `OKAY`.
pub fn read_forward_reply<R: Read>(r: &mut R, expect_port: bool) -> Wire<Option<u16>> {
    read_status(r)?;
    read_status(r)?;
    if !expect_port {
        return Ok(None);
    }
    let port = read_hex_string(r)?;
    match port.trim().parse::<u16>() {
        Ok(p) if p != 0 => Ok(Some(p)),
        _ => Err(WireError::Bad(format!("port {port:?}"))),
    }
}

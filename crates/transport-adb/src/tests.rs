use super::*;
use crate::errors::host_fail;
use socketsweep_transport::{DeviceState, TransportError};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

#[test]
fn host_failures_map_to_the_transport_errors() {
    assert!(matches!(
        host_fail(Some("R5"), "device 'R5' not found"),
        TransportError::NotFound { serial } if serial == "R5"
    ));
    assert!(matches!(
        host_fail(Some("R5"), "device unauthorized."),
        TransportError::Unusable {
            state: DeviceState::Unauthorized,
            ..
        }
    ));
    assert!(matches!(
        host_fail(Some("R5"), "device offline"),
        TransportError::Unusable {
            state: DeviceState::Offline,
            ..
        }
    ));
    assert!(matches!(host_fail(None, "boom"), TransportError::Refused(m) if m == "boom"));
}

fn read_request(s: &mut TcpStream) -> String {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let n = usize::from_str_radix(std::str::from_utf8(&len).unwrap(), 16).unwrap();
    let mut body = vec![0u8; n];
    s.read_exact(&mut body).unwrap();
    String::from_utf8(body).unwrap()
}

/// A fake adb server on loopback: three connections, each answering
/// `host:transport:` and one service. `localabstract:daemon` then echoes.
fn fake_server() -> (SocketAddr, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut s, _) = listener.accept().unwrap();
            let first = read_request(&mut s);
            if first == "host:transport:gone" {
                s.write_all(b"FAIL0017device 'gone' not found").unwrap();
                continue;
            }
            assert!(first.starts_with("host:transport:"));
            s.write_all(b"OKAY").unwrap();
            if read_request(&mut s) == "localabstract:daemon" {
                s.write_all(b"OKAY").unwrap();
                let mut buf = [0u8; 4];
                s.read_exact(&mut buf).unwrap();
                s.write_all(&buf).unwrap();
                let _ = s.read(&mut buf);
            } else {
                s.write_all(b"FAIL0006closed").unwrap();
            }
        }
    });
    (addr, handle)
}

#[test]
fn open_tunnel_is_a_direct_localabstract_service() {
    let (addr, server) = fake_server();
    let adb = AdbTransport::with_server(Path::new("adb-not-needed"), addr);

    let tunnel = adb.open_tunnel("R5CX1234", "daemon").unwrap();
    let mut writer = tunnel.try_clone().unwrap();
    let mut reader = tunnel;
    writer.write_all(b"ping").unwrap();
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
    writer.shutdown().unwrap();
    assert_eq!(reader.read(&mut buf).unwrap(), 0);

    // A name nothing listens on is refused with the server's reason.
    match adb.open_tunnel("R5CX1234", "nothing") {
        Err(TransportError::Refused(m)) => assert!(m.contains("closed"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // A vanished device is NotFound.
    assert!(matches!(
        adb.open_tunnel("gone", "daemon"),
        Err(TransportError::NotFound { serial }) if serial == "gone"
    ));
    server.join().unwrap();
}

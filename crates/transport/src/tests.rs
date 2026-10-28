use super::*;
use std::net::TcpListener;

#[test]
fn output_success_treats_a_missing_exit_code_as_success() {
    let mut out = Output::default();
    assert!(out.success());
    out.exit = Some(0);
    assert!(out.success());
    out.exit = Some(1);
    assert!(!out.success());
}

#[test]
fn only_ready_devices_are_usable() {
    let mut d = Device {
        serial: "s".into(),
        model: "m".into(),
        state: DeviceState::Ready,
        hint: None,
    };
    assert!(d.usable());
    for state in [
        DeviceState::Unauthorized,
        DeviceState::Offline,
        DeviceState::NoPermission,
        DeviceState::Other("recovery".into()),
    ] {
        d.state = state;
        assert!(!d.usable());
    }
}

#[test]
fn unusable_errors_carry_the_hint() {
    let e = TransportError::Unusable {
        serial: "R58".into(),
        state: DeviceState::NoPermission,
        hint: Some("Add a udev rule.".into()),
    };
    assert_eq!(
        e.to_string(),
        "Device R58 cannot be opened. Add a udev rule."
    );
}

/// A TCP socket is a `Duplex`, and a shutdown through one handle wakes a
/// reader blocked on another.
#[test]
fn tcp_streams_are_duplexes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 5];
        s.read_exact(&mut buf).unwrap();
        s.write_all(&buf).unwrap();
        // Hold the socket open until the client is done.
        let _ = s.read(&mut buf);
    });

    let stream: Box<dyn Duplex> = Box::new(TcpStream::connect(addr).unwrap());
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = stream;
    writer.write_all(b"hello").unwrap();
    let mut buf = [0u8; 5];
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");

    writer.shutdown().unwrap();
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
    server.join().unwrap();
}

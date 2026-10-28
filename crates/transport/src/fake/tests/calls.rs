//! Shell, push, pull and device state on the fake transport.

use super::super::*;
use super::support::*;
use crate::{DeviceState, Output, Transport, TransportError};
use std::path::Path;
use std::sync::Arc;

#[test]
fn it_is_usable_as_a_shared_trait_object() {
    let t: Arc<dyn Transport> = Arc::new(phone());
    let t2 = Arc::clone(&t);
    std::thread::spawn(move || t2.devices().unwrap())
        .join()
        .unwrap();
    assert_eq!(t.devices().unwrap().len(), 1);
}

#[test]
fn devices_are_listed_whatever_their_state() {
    let t = phone().with_device(FakeDevice {
        state: DeviceState::Unauthorized,
        ..FakeDevice::ready("P2")
    });
    let devices = t.devices().unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices[0].usable());
    assert_eq!(devices[1].state, DeviceState::Unauthorized);
    assert!(!devices[1].usable());

    t.remove_device("P1");
    assert_eq!(t.devices().unwrap().len(), 1);
}

#[test]
fn calls_to_missing_or_unusable_devices_fail() {
    let t = phone();
    assert!(matches!(
        t.shell("nope", "true"),
        Err(TransportError::NotFound { .. })
    ));

    t.set_state("P1", DeviceState::NoPermission);
    match t.device_info("P1") {
        Err(TransportError::Unusable { state, .. }) => {
            assert_eq!(state, DeviceState::NoPermission)
        }
        other => panic!("expected Unusable, got {other:?}"),
    }
    t.set_state("P1", DeviceState::Ready);
    assert_eq!(t.device_info("P1").unwrap().abi, "arm64-v8a");
}

#[test]
fn shell_commands_are_scripted_logged_and_newest_first() {
    let t = phone();
    let ok = |s: &str| Output {
        stdout: s.as_bytes().to_vec(),
        stderr: Vec::new(),
        exit: Some(0),
    };
    t.respond("P1", "getprop", ok("34\n"));
    assert_eq!(
        t.shell("P1", "getprop ro.build.version.sdk")
            .unwrap()
            .stdout_text(),
        "34\n"
    );

    // A newer handler overrides an older one.
    t.respond("P1", "getprop", ok("35\n"));
    assert_eq!(t.shell("P1", "getprop x").unwrap().stdout_text(), "35\n");

    // Unscripted commands fail the way sh does.
    let out = t.shell("P1", "frobnicate --now").unwrap();
    assert_eq!(out.exit, Some(127));
    assert!(!out.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("frobnicate"));

    assert_eq!(
        t.shell_log("P1"),
        [
            "getprop ro.build.version.sdk",
            "getprop x",
            "frobnicate --now"
        ]
    );
}

#[test]
fn a_shell_handler_may_call_back_into_the_transport() {
    let t = Arc::new(phone());
    let weak = Arc::downgrade(&t);
    t.on_shell("P1", move |command| {
        let t = weak.upgrade()?;
        let path = command.strip_prefix("rm ")?;
        let existed = t.file("P1", path.as_bytes()).is_some();
        t.put_file("P1", path.as_bytes(), Vec::new(), 0);
        Some(Output {
            exit: Some(if existed { 0 } else { 1 }),
            ..Output::default()
        })
    });
    t.put_file("P1", b"/data/local/tmp/x", vec![1], 0o644);
    assert!(t.shell("P1", "rm /data/local/tmp/x").unwrap().success());
}

#[test]
fn pushed_files_land_in_the_store_and_pull_back() {
    let dir = scratch("push-pull");
    let local = dir.join("daemon");
    let data: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
    std::fs::write(&local, &data).unwrap();

    let t = phone();
    t.push("P1", &local, "/data/local/tmp/socketsweep", 0o755)
        .unwrap();
    let stored = t.file("P1", b"/data/local/tmp/socketsweep").unwrap();
    assert_eq!(stored.mode, 0o755);
    assert_eq!(stored.data, data);

    let back = dir.join("back");
    let mut seen = Vec::new();
    let n = t
        .pull("P1", b"/data/local/tmp/socketsweep", &back, &mut |done| {
            seen.push(done)
        })
        .unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(std::fs::read(&back).unwrap(), data);
    // Cumulative, in sync-sized chunks, ending at the full size.
    assert_eq!(seen, [65_536, 131_072, 196_608, 200_000]);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn non_utf8_remote_paths_are_exact() {
    let dir = scratch("bytes");
    let t = phone();
    let name = b"/sdcard/\xFF\xFEpic.jpg";
    t.put_file("P1", name, b"jpeg".to_vec(), 0o660);
    t.pull("P1", name, &dir.join("pic"), &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(dir.join("pic")).unwrap(), b"jpeg");
    assert!(matches!(
        t.pull("P1", b"/sdcard/\xFFpic.jpg", &dir.join("x"), &mut |_| {}),
        Err(TransportError::Refused(_))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pushing_a_missing_local_file_is_a_local_error() {
    let t = phone();
    let err = t
        .push("P1", Path::new("does/not/exist"), "/x", 0o755)
        .unwrap_err();
    assert!(matches!(err, TransportError::Local { .. }), "{err:?}");
}

#[test]
fn injected_faults_fire_once() {
    let t = phone();
    t.fail_next(FakeOp::Devices, TransportError::Server("down".into()));
    assert!(matches!(t.devices(), Err(TransportError::Server(_))));
    assert!(t.devices().is_ok());

    t.fail_next(FakeOp::OpenTunnel, TransportError::Refused("no".into()));
    assert!(t.open_tunnel("P1", "x").is_err());
    assert!(t.open_tunnel("P1", "x").is_ok());
}

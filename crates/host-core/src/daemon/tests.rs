use std::path::Path;

use socketsweep_transport::DeviceInfo;
use socketsweep_wire::{token_to_hex, TOKEN_ENV, TOKEN_LEN};

use super::command::take_quoted;
use super::lifecycle::push_daemon;
use super::*;

#[test]
fn the_supported_roots_are_accepted() {
    for root in [
        "/sdcard",
        "/storage/emulated/0",
        "/storage/emulated/10",
        "/storage/1A2B-3C4D",
    ] {
        assert!(check_root(root).is_ok(), "{root}");
    }
}

#[test]
fn anything_else_is_refused() {
    for root in [
        "",
        "/",
        "/sdcard/",
        "/sdcard/DCIM",
        "/sdcard; rm -rf /",
        "/sdcard'",
        "/storage/emulated/",
        "/storage/emulated/0/",
        "/storage/1A2B-3C4G",
        "/data",
        "/storage/emulated/../../data",
    ] {
        assert!(check_root(root).is_err(), "{root:?} should be refused");
    }
}

#[test]
fn shell_quoting_survives_single_quotes_and_spaces() {
    assert_eq!(shell_quote("/sdcard"), "'/sdcard'");
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
}

#[test]
fn the_launch_command_round_trips_and_keeps_the_token_off_argv() {
    let token = [0xAB; TOKEN_LEN];
    let cmd = launch_command(&token, "socketsweep-x", "/storage/emulated/0");
    assert!(cmd.starts_with(&format!("{TOKEN_ENV}={}", token_to_hex(&token))));
    let after_bin = cmd.split(DEVICE_BIN).nth(1).unwrap();
    assert!(!after_bin.contains(&token_to_hex(&token)));
    let parsed = parse_launch_command(&cmd).unwrap();
    assert_eq!(parsed.token_hex, token_to_hex(&token));
    assert_eq!(parsed.socket, "socketsweep-x");
    assert_eq!(parsed.root, "/storage/emulated/0");
    assert_eq!(
        take_quoted(r"'it'\''s' rest"),
        Some(("it's".into(), " rest"))
    );
    assert!(parse_launch_command("pkill -f x").is_none());
}

#[test]
fn a_gzipped_daemon_is_pushed_decompressed_and_a_raw_one_as_is() {
    use socketsweep_transport::fake::{FakeDevice, FakeTransport};
    use std::io::Write;

    let dir = std::env::temp_dir().join(format!("ss-daemon-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let payload = b"ELF pretend daemon".repeat(100);

    let gz = dir.join("daemon-arm64.gz");
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(&payload).unwrap();
    std::fs::write(&gz, enc.finish().unwrap()).unwrap();
    let raw = dir.join("daemon-raw");
    std::fs::write(&raw, &payload).unwrap();

    for path in [&gz, &raw] {
        let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
        push_daemon(&t, "P1", path).unwrap();
        let file = t.file("P1", DEVICE_BIN.as_bytes()).unwrap();
        assert_eq!(file.data, payload, "{}", path.display());
        assert_eq!(file.mode, 0o755);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_binary_is_picked_by_abi_and_old_phones_are_refused() {
    let bins = DaemonBinaries {
        arm64: Some("arm".into()),
        x86_64: None,
    };
    let info = |sdk, abi: &str| DeviceInfo {
        sdk,
        abi: abi.into(),
        model: "m".into(),
    };
    assert_eq!(bins.pick(&info(34, "arm64-v8a")).unwrap(), Path::new("arm"));
    assert!(matches!(
        bins.pick(&info(34, "x86_64")),
        Err(LaunchError::Missing(_))
    ));
    assert!(matches!(
        bins.pick(&info(29, "arm64-v8a")),
        Err(LaunchError::Unsupported(_))
    ));
    assert!(matches!(
        bins.pick(&info(34, "armeabi-v7a")),
        Err(LaunchError::Unsupported(_))
    ));
}

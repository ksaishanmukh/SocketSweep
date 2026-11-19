use super::*;

fn phone() -> Phone {
    Phone {
        model: "Pixel 8".into(),
        sdk: 34,
        abi: "arm64-v8a".into(),
        daemon_build: "daemon 1.0.0".into(),
        serial: "ABC123".into(),
        root: "/sdcard".into(),
    }
}

fn report(phone: Option<Phone>) -> Report {
    Report {
        app_version: "1.0.0",
        phone,
        log: vec![
            "INFO connected to Pixel 8".into(),
            "INFO copy to /home/me/Desktop done for ABC123".into(),
        ],
    }
}

#[test]
fn public_report_names_the_phone_but_not_its_serial_or_paths() {
    let text = render(&report(Some(phone())), false);
    assert!(text.contains("App: 1.0.0"));
    assert!(text.contains("Phone: Pixel 8, Android API 34, arm64-v8a"));
    assert!(text.contains("Daemon: daemon 1.0.0"));
    assert!(text.contains("INFO connected to Pixel 8"));
    assert!(!text.contains("ABC123"));
    assert!(!text.contains("/sdcard"));
    assert!(!text.contains("/home/me"));
    assert!(text.contains("INFO copy to <path> done for <serial>"));
}

#[test]
fn private_report_adds_serial_root_and_raw_log_lines() {
    let text = render(&report(Some(phone())), true);
    assert!(text.contains("Serial: ABC123"));
    assert!(text.contains("Root: /sdcard"));
    assert!(text.contains("copy to /home/me/Desktop done for ABC123"));
}

#[test]
fn report_without_a_phone_says_so_and_still_masks_paths() {
    let text = render(&report(None), false);
    assert!(text.contains("Phone: not connected"));
    assert!(!text.contains("Daemon:"));
    assert!(!text.contains("/home/me"));
}

#[test]
fn log_tail_reads_the_newest_file_and_keeps_the_last_lines() {
    let dir = std::env::temp_dir().join(format!("socketsweep-tail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{LOG_FILE_PREFIX}.2026-01-01")), "old\n").unwrap();
    std::fs::write(
        dir.join(format!("{LOG_FILE_PREFIX}.2026-01-02")),
        "a\nb\nc\n",
    )
    .unwrap();
    std::fs::write(dir.join("other.txt"), "zzz\n").unwrap();
    assert_eq!(log_tail(&dir, 2), ["b", "c"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn log_tail_of_a_missing_folder_is_empty() {
    assert!(log_tail(Path::new("no/such/folder"), 5).is_empty());
}

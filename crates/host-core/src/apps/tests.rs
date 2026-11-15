//! Package names, the catalog, and running the helper on a fake phone.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use socketsweep_gui_contract::AppsStatus;
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_transport::{Output, Transport};

use super::helper::*;
use super::*;

/// `{"v":1}` then one line per `(package, label, icon base64)`.
pub(crate) fn helper_output(apps: &[(&str, &str, Option<&str>)]) -> Output {
    let mut out = String::from("{\"v\":1}\n");
    for (pkg, label, icon) in apps {
        let icon = icon.map_or(String::new(), |b| format!(",\"icon\":\"{b}\""));
        out.push_str(&format!(
            "{{\"pkg\":\"{pkg}\",\"label\":\"{label}\",\"system\":false{icon}}}\n"
        ));
    }
    Output {
        stdout: out.into_bytes(),
        stderr: Vec::new(),
        exit: Some(0),
    }
}

fn phone() -> (Arc<FakeTransport>, Arc<dyn Transport>) {
    let fake = Arc::new(FakeTransport::new().with_device(FakeDevice::ready("P1")));
    let t: Arc<dyn Transport> = fake.clone();
    (fake, t)
}

fn fast() -> HelperOptions {
    HelperOptions {
        timeout: Duration::from_millis(200),
        ..HelperOptions::default()
    }
}

#[test]
fn package_names_are_recognised_and_odd_folders_are_not() {
    for ok in ["com.whatsapp", "a.b", "com.example_app.v2", "Com.X1"] {
        assert!(is_package_name(ok), "{ok}");
    }
    for bad in [
        ".nomedia", "cache", "com.", ".com.x", "com..x", "1com.x", "com.x-y", "com.x y", "a.b;rm",
        "",
    ] {
        assert!(!is_package_name(bad), "{bad}");
    }
}

#[test]
fn the_catalog_says_installed_until_the_phone_answers() {
    let pending = AppCatalog::default();
    assert_eq!(pending.status(), AppsStatus::Pending);
    assert!(pending.installed("gone.app"));
    assert!(AppCatalog::unavailable().installed("gone.app"));

    let mut ready = AppCatalog::ready(HashMap::from([("com.a".into(), "A".into())]));
    ready.add_icons(&["com.a".to_string()]);
    let tag = ready.tag("com.a");
    assert_eq!(tag.label.as_deref(), Some("A"));
    assert!(tag.installed && tag.has_icon);
    let gone = ready.tag("gone.app");
    assert!(!gone.installed && !gone.has_icon && gone.label.is_none());
    assert_eq!(ready.icon_count(), 1);
}

#[test]
fn the_command_names_the_helper_and_quotes_the_icon_list() {
    assert_eq!(
        helper_command(&[], 48),
        "CLASSPATH=/data/local/tmp/socketsweep-helper.dex app_process / dev.socketsweep.Helper apps"
    );
    let with_icons = helper_command(&["a.b".into(), "c.d".into()], 48);
    assert!(
        with_icons.ends_with(" apps --icons 'a.b,c.d' --icon-size 48"),
        "{with_icons}"
    );
}

#[test]
fn names_come_back_parsed() {
    let (fake, t) = phone();
    fake.respond(
        "P1",
        "CLASSPATH=",
        helper_output(&[("com.a", "App A", None)]),
    );
    let apps = list_apps(&t, "P1", &fast()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].label, "App A");
}

#[test]
fn a_failing_or_garbled_helper_is_an_error() {
    let (fake, t) = phone();
    fake.respond(
        "P1",
        "CLASSPATH=",
        Output {
            stdout: Vec::new(),
            stderr: b"boom".to_vec(),
            exit: Some(1),
        },
    );
    let err = list_apps(&t, "P1", &fast()).unwrap_err();
    assert!(
        matches!(err, HelperError::Exit { code: Some(1), .. }),
        "{err}"
    );

    fake.respond(
        "P1",
        "CLASSPATH=",
        Output {
            stdout: b"Error: no app_process\n".to_vec(),
            stderr: Vec::new(),
            exit: None,
        },
    );
    let err = list_apps(&t, "P1", &fast()).unwrap_err();
    assert!(matches!(err, HelperError::Output(_)), "{err}");
}

#[test]
fn a_hung_helper_is_given_up_and_killed() {
    let (fake, t) = phone();
    fake.on_shell("P1", |command| {
        command.starts_with("CLASSPATH=").then(|| {
            std::thread::sleep(Duration::from_secs(2));
            Output::default()
        })
    });
    let started = Instant::now();
    let err = list_apps(&t, "P1", &fast()).unwrap_err();
    assert!(matches!(err, HelperError::TimedOut(_)), "{err}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(fake.shell_log("P1").iter().any(|c| c == KILL_HELPER));
}

#[test]
fn icons_are_kept_only_for_the_packages_asked_for() {
    let (fake, t) = phone();
    fake.respond(
        "P1",
        "CLASSPATH=",
        helper_output(&[
            ("com.a", "A", Some("iVBORw0KGg==")),
            ("com.b", "B", None),
            ("com.c", "C", Some("iVBORw0KGg==")),
        ]),
    );
    let icons = fetch_icons(&t, "P1", &["com.a".into(), "com.b".into()], &fast()).unwrap();
    assert_eq!(icons.len(), 1);
    assert_eq!(icons["com.a"], b"\x89PNG\r\n\x1a");
}

#[test]
fn a_crashing_icon_run_is_an_error() {
    let (fake, t) = phone();
    fake.respond(
        "P1",
        "CLASSPATH=",
        Output {
            stdout: b"{\"v\":1}\n{\"pkg\":\"com.a\",\"lab".to_vec(),
            stderr: b"Segmentation fault".to_vec(),
            exit: Some(139),
        },
    );
    let err = fetch_icons(&t, "P1", &["com.a".into()], &fast()).unwrap_err();
    assert!(
        matches!(
            err,
            HelperError::Exit {
                code: Some(139),
                ..
            }
        ),
        "{err}"
    );
}

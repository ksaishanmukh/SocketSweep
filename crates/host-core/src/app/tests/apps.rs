//! The app helper through the app: names, icons in chunks, failures, time
//! limits, and cleaning up the phone.

use std::time::Duration;

use socketsweep_gui_contract::{AppsStatus, AppsUpdated, Commands, ViewFilter};
use socketsweep_transport::Output;

use super::android::android;
use super::rig::{row, Rig};
use crate::app::HostEvent;
use crate::apps::helper::KILL_HELPER;
use crate::apps::DEVICE_HELPER;
use crate::testkit::wait_for;

/// The base64 of a 7-byte PNG signature prefix.
const ICON: &str = "iVBORw0KGg==";

fn helper_says(apps: &[(&str, &str, Option<&str>)]) -> Output {
    crate::apps::tests::helper_output(apps)
}

fn start(rig: &Rig) {
    rig.app.connect(None, None).unwrap();
    rig.app.start_scan(None).unwrap();
}

fn wait_apps(rig: &Rig, mut pred: impl FnMut(&AppsUpdated) -> bool) -> AppsUpdated {
    match rig.wait_event(|e| matches!(e, HostEvent::AppsUpdated(a) if pred(a))) {
        HostEvent::AppsUpdated(a) => a,
        _ => unreachable!(),
    }
}

/// Wait until the scan is complete, by its file count.
fn wait_scanned(rig: &Rig) {
    assert!(wait_for(Duration::from_secs(5), || rig
        .app
        .overview(None)
        .is_ok_and(|o| o.totals.files == 4)));
}

/// Names for com.a and com.b; com.b's icon; a crash when com.a's is asked
/// for.
fn crash_on_com_a_icon(rig: &Rig) {
    rig.transport.on_shell("P1", |command| {
        if !command.starts_with("CLASSPATH=") {
            return None;
        }
        Some(if command.contains("--icons 'com.a'") {
            Output {
                stdout: b"{\"v\":1}\n".to_vec(),
                stderr: b"Segmentation fault".to_vec(),
                exit: Some(139),
            }
        } else if command.contains("--icons 'com.b'") {
            helper_says(&[("com.a", "App A", None), ("com.b", "App B", Some(ICON))])
        } else {
            helper_says(&[("com.a", "App A", None), ("com.b", "App B", None)])
        })
    });
}

#[test]
fn names_then_icons_arrive_and_a_crashing_icon_run_costs_only_its_chunk() {
    let rig = Rig::with_helper(android());
    crash_on_com_a_icon(&rig);
    start(&rig);
    let named = wait_apps(&rig, |a| a.status == AppsStatus::Ready);
    assert_eq!(named.icons, 0);
    let iconed = wait_apps(&rig, |a| a.icons == 1);
    assert_eq!(iconed.status, AppsStatus::Ready);
    wait_scanned(&rig);

    let app = &rig.app;
    assert!(app
        .app_icon("com.b".into())
        .unwrap()
        .is_some_and(|b| b == ICON));
    assert_eq!(
        app.app_icon("com.a".into()).unwrap(),
        None,
        "its run crashed"
    );

    let overview = app.overview(None).unwrap();
    assert_eq!(overview.apps_status, AppsStatus::Ready);
    let labels: Vec<_> = overview.apps.iter().map(|a| a.label.as_deref()).collect();
    assert_eq!(labels, [Some("App A"), Some("App B"), None]);
    assert_eq!(overview.leftovers.apps.len(), 1);
    assert_eq!(overview.leftovers.apps[0].package, "com.c");

    // Rows say which app a folder belongs to.
    let android = row(
        &app.get_view(None, None, None, None, None).unwrap(),
        "Android",
    );
    let data = row(
        &app.get_view(Some(android), None, None, None, None).unwrap(),
        "data",
    );
    let view = app
        .get_view(Some(data), None, None, None, Some(ViewFilter::default()))
        .unwrap();
    let b = view.rows.iter().find(|r| r.name == "com.b").unwrap();
    let tag = b.app.as_ref().unwrap();
    assert_eq!(tag.label.as_deref(), Some("App B"));
    assert!(tag.installed && tag.has_icon);

    // The helper went to the phone once, icons were asked for only the
    // installed packages, one per run, and com.c (uninstalled) never.
    assert!(rig.transport.file("P1", DEVICE_HELPER.as_bytes()).is_some());
    let log = rig.transport.shell_log("P1");
    let runs: Vec<_> = log.iter().filter(|c| c.starts_with("CLASSPATH=")).collect();
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert!(!runs.iter().any(|c| c.contains("com.c")));

    app.disconnect().unwrap();
    let log = rig.transport.shell_log("P1");
    assert!(log.iter().any(|c| c == KILL_HELPER));
    assert!(log
        .iter()
        .any(|c| c.starts_with("rm -f") && c.contains(DEVICE_HELPER)));
}

#[test]
fn a_failing_helper_leaves_package_names_and_no_leftovers() {
    let rig = Rig::with_helper(android());
    rig.transport.respond(
        "P1",
        "CLASSPATH=",
        Output {
            stdout: Vec::new(),
            stderr: b"java.lang.SecurityException".to_vec(),
            exit: Some(1),
        },
    );
    start(&rig);
    wait_apps(&rig, |a| a.status == AppsStatus::Unavailable);
    wait_scanned(&rig);
    let overview = rig.app.overview(None).unwrap();
    assert_eq!(overview.apps_status, AppsStatus::Unavailable);
    assert!(overview.leftovers.apps.is_empty());
    assert_eq!(overview.apps.len(), 3);
    assert!(overview
        .apps
        .iter()
        .all(|a| a.installed && a.label.is_none()));
}

#[test]
fn a_hung_helper_is_given_up_and_killed() {
    let rig = Rig::with_helper(android());
    rig.transport.on_shell("P1", |command| {
        command.starts_with("CLASSPATH=").then(|| {
            std::thread::sleep(Duration::from_secs(2));
            Output::default()
        })
    });
    start(&rig);
    wait_apps(&rig, |a| a.status == AppsStatus::Unavailable);
    assert!(rig
        .transport
        .shell_log("P1")
        .iter()
        .any(|c| c == KILL_HELPER));
}

#[test]
fn without_a_bundled_helper_apps_are_unavailable_at_once() {
    let rig = Rig::new(android());
    start(&rig);
    wait_scanned(&rig);
    assert_eq!(
        rig.app.overview(None).unwrap().apps_status,
        AppsStatus::Unavailable
    );
    let log = rig.transport.shell_log("P1");
    assert!(!log.iter().any(|c| c.starts_with("CLASSPATH=")));
    assert!(rig.transport.file("P1", DEVICE_HELPER.as_bytes()).is_none());
}

#[test]
fn a_rescan_starts_with_the_names_already_known() {
    let rig = Rig::with_helper(android());
    rig.transport
        .respond("P1", "CLASSPATH=", helper_says(&[("com.a", "App A", None)]));
    start(&rig);
    wait_apps(&rig, |a| a.status == AppsStatus::Ready);
    wait_scanned(&rig);
    rig.app.start_scan(None).unwrap();
    // Before the new scan's own helper run can have answered.
    let overview = rig.app.overview(None).unwrap();
    assert_eq!(overview.apps_status, AppsStatus::Ready);
}

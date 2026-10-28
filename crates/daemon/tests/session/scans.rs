//! Scans over the pipe: streams, focus and cancel.

use std::sync::Arc;
use std::time::Duration;

use socketsweep_engine::{Fs, StdFs};
use socketsweep_wire::{Caps, ClientMsg, DirId, Outcome, ScanEvent, ScanMode, ScanOpts, ServerMsg};

use super::support::*;

#[test]
fn a_scan_streams_every_directory_once_with_true_totals() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    let events = scan_events(&host, id, |f| panic!("unexpected {f:?}"));
    let names = check_stream(&events);
    let (outcome, totals) = finished(&events);
    assert_eq!(outcome, Outcome::Complete);
    let (files, dirs, apparent) = truth(&root);
    assert_eq!(totals.files, files);
    assert_eq!(totals.dirs, dirs);
    assert_eq!(
        names.len() as u32,
        dirs + 1,
        "every directory introduced once"
    );
    // Directory sizes are platform-defined; file bytes are exact.
    assert_eq!(totals.apparent, apparent);
    assert_eq!(totals.errors, 0);
    let done = events
        .iter()
        .filter(|e| matches!(e, ScanEvent::SubtreeDone { .. }))
        .count();
    assert_eq!(done as u32, dirs + 1, "every directory finished once");
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

/// `ScanOpts::mode: Plain` (the benchmark baseline) reaches the engine: no
/// skeleton, no deferred stats, the same totals.
#[test]
fn a_plain_scan_is_honoured() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts {
            mode: ScanMode::Plain,
            ..ScanOpts::default()
        },
    });
    let events = scan_events(&host, id, |f| panic!("unexpected {f:?}"));
    check_stream(&events);
    let (outcome, totals) = finished(&events);
    assert_eq!(outcome, Outcome::Complete);
    let (files, dirs, apparent) = truth(&root);
    assert_eq!((totals.files, totals.dirs), (files, dirs));
    assert_eq!(totals.apparent, apparent);
    for e in &events {
        assert!(
            !matches!(e, ScanEvent::SkeletonReady { .. } | ScanEvent::Stats { .. }),
            "plain mode sent {e:?}"
        );
    }
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

/// Frames until the root listing has arrived; the id of its subdirectory
/// `name`, and the events so far.
fn await_root_child(host: &Host, name: &[u8]) -> (DirId, Vec<ScanEvent>) {
    let mut events = Vec::new();
    loop {
        let f = host.recv();
        if let ServerMsg::Scan(batch) = f.msg {
            events.extend(batch);
            let found = events.iter().find_map(|e| match e {
                ScanEvent::Listing {
                    dir: 0, entries, ..
                } => entries
                    .iter()
                    .find(|en| en.name == name)
                    .and_then(|en| en.id),
                _ => None,
            });
            if let Some(a) = found {
                return (a, events);
            }
        }
    }
}

#[test]
fn focus_is_reported_and_cancel_ends_the_scan() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(40)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    // Wait for the root listing, then focus `a`.
    let (a, mut events) = await_root_child(&host, b"a");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ScanEvent::Finished { .. })),
        "finished too early: {events:?}"
    );
    host.send(ClientMsg::Focus {
        scan: id,
        dir: a,
        seq: 1,
    });
    let mut cancelled = false;
    loop {
        let f = host.recv();
        if let ServerMsg::Scan(batch) = f.msg {
            let focus = batch
                .iter()
                .any(|e| matches!(e, ScanEvent::FocusState { seq: 1, dir, .. } if *dir == a));
            let done = batch
                .iter()
                .any(|e| matches!(e, ScanEvent::Finished { .. }));
            events.extend(batch);
            if focus && !cancelled {
                host.send(ClientMsg::Cancel { target: id });
                cancelled = true;
            }
            if done {
                break;
            }
        }
    }
    assert!(cancelled, "a FocusState arrived");
    check_stream(&events);
    let (outcome, _) = finished(&events);
    // The scan may have finished on its own just before the cancel.
    assert!(matches!(outcome, Outcome::Cancelled | Outcome::Complete));
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

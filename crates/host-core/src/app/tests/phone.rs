//! The scripted daemon the app tests talk to.

use std::sync::{Arc, Mutex};

use socketsweep_wire::{
    ClientFrame, ClientMsg, CopyItem, Kind, Outcome, ScanEvent, ServerMsg, Totals,
};

use super::rig::{entry, totals, RESOLVED};
use crate::lock;
use crate::testkit::{FakeDaemon, Responder};

/// The running scan: its request id and where to answer.
type ScanSlot = Arc<Mutex<Option<(u32, Responder)>>>;

/// A daemon serving `/sdcard` = { DCIM/ { pic.jpg 1000 }, a.txt 100 }. The
/// scan stops after the root's listing until a `Focus` arrives (then
/// finishes) or a `Cancel` (then is cancelled).
pub(super) fn phone() -> FakeDaemon {
    let scan: ScanSlot = Arc::new(Mutex::new(None));
    FakeDaemon::new(
        move |frame: &ClientFrame, out: &Responder| match &frame.msg {
            ClientMsg::StartScan { .. } => start_scan(&scan, frame.id, out),
            ClientMsg::Focus { scan: id, dir, seq } => focus(&scan, *id, *dir, *seq, out),
            ClientMsg::Cancel { target } => cancel(&scan, *target, out),
            ClientMsg::Delete { .. } => {
                out.send(
                    frame.id,
                    ServerMsg::Deleted {
                        items: 1,
                        totals: totals(100, 1, 0),
                    },
                );
            }
            ClientMsg::PrepareCopy { path } => prepare_copy(frame.id, path, out),
            _ => {}
        },
    )
}

fn start_scan(scan: &ScanSlot, id: u32, out: &Responder) {
    *lock(scan) = Some((id, out.clone()));
    out.send(
        id,
        ServerMsg::Scan(vec![
            ScanEvent::Started {
                root: RESOLVED.to_vec(),
                dev: 1,
                workers: 4,
            },
            ScanEvent::Listing {
                dir: 0,
                part: 0,
                last: true,
                entries: vec![
                    entry("DCIM", Kind::Dir, 0, Some(1)),
                    entry("a.txt", Kind::File, 100, None),
                ],
            },
        ]),
    );
}

fn focus(scan: &ScanSlot, id: u32, dir: u32, seq: u32, out: &Responder) {
    assert_eq!(Some(id), lock(scan).as_ref().map(|s| s.0));
    out.send(
        id,
        ServerMsg::Scan(vec![
            ScanEvent::Listing {
                dir: 1,
                part: 0,
                last: true,
                entries: vec![entry("pic.jpg", Kind::File, 1000, None)],
            },
            ScanEvent::SubtreeDone {
                dir: 1,
                totals: totals(1000, 1, 0),
            },
            ScanEvent::FocusState {
                seq,
                dir,
                settled: true,
            },
            ScanEvent::SubtreeDone {
                dir: 0,
                totals: totals(1100, 2, 1),
            },
            ScanEvent::Finished {
                outcome: Outcome::Complete,
                totals: totals(1100, 2, 1),
                elapsed_ms: 5,
            },
        ]),
    );
}

fn cancel(scan: &ScanSlot, target: u32, out: &Responder) {
    if lock(scan).as_ref().is_some_and(|s| s.0 == target) {
        out.send(
            target,
            ServerMsg::Scan(vec![ScanEvent::Finished {
                outcome: Outcome::Cancelled,
                totals: Totals::default(),
                elapsed_ms: 5,
            }]),
        );
    }
}

fn prepare_copy(id: u32, path: &[u8], out: &Responder) {
    let item = |rel: &[u8], kind, size| CopyItem {
        rel: rel.to_vec(),
        kind,
        size,
        mode: 0o644,
        mtime: 0,
    };
    out.send(
        id,
        ServerMsg::CopyManifest {
            root: path.to_vec(),
            files: vec![item(b"", Kind::Dir, 0)],
            last: false,
        },
    );
    out.send(
        id,
        ServerMsg::CopyManifest {
            root: path.to_vec(),
            files: vec![
                item(b"pic.jpg", Kind::File, 4),
                item(b"link", Kind::Symlink, 0),
            ],
            last: true,
        },
    );
}

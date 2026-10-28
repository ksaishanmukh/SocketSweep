//! Pings, ids and requests refused before they start.

use std::sync::Arc;
use std::time::Duration;

use socketsweep_daemon::session::End;
use socketsweep_engine::{Fs, StdFs};
use socketsweep_wire::{Caps, ClientMsg, ErrCode, Outcome, ScanOpts, ServerFrame, ServerMsg};

use super::support::*;

#[test]
fn ping_shutdown_and_bad_ids() {
    let tmp = fixture();
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root_of(&tmp), Caps::all());
    let id = host.send(ClientMsg::Ping { nonce: 42 });
    assert_eq!(
        host.reply(),
        ServerFrame {
            id,
            msg: ServerMsg::Pong { nonce: 42 }
        }
    );

    host.send_as(
        0,
        ClientMsg::PrepareCopy {
            path: root_bytes(&root_of(&tmp)),
        },
    );
    let f = host.reply();
    assert!(
        matches!(
            f.msg,
            ServerMsg::Failed {
                code: ErrCode::BadRequest,
                ..
            }
        ),
        "{f:?}"
    );

    let id = host.send(ClientMsg::Focus {
        scan: 999,
        dir: 0,
        seq: 1,
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::BadRequest,
            ..
        }
    ));

    host.send(ClientMsg::Shutdown);
    assert_eq!(t.join().unwrap().unwrap(), End::Shutdown);
}

#[test]
fn a_duplicate_id_is_refused_while_the_first_runs() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(30)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let opts = ScanOpts::default();
    host.send_as(
        5,
        ClientMsg::StartScan {
            root: root_bytes(&root),
            opts: opts.clone(),
        },
    );
    host.send_as(
        5,
        ClientMsg::StartScan {
            root: root_bytes(&root),
            opts,
        },
    );
    let f = host.reply();
    assert_eq!(f.id, 5);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::DuplicateId,
            ..
        }
    ));
    host.send(ClientMsg::Cancel { target: 5 });
    let events = scan_events(&host, 5, |_| {});
    assert_eq!(finished(&events).0, Outcome::Cancelled);
    host.send(ClientMsg::Shutdown);
    assert_eq!(t.join().unwrap().unwrap(), End::Shutdown);
}

#[test]
fn a_scan_outside_the_root_fails_before_starting() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let (t, mut host) = connect(config(), Arc::new(StdFs), &root.join("a"), Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    let f = host.reply();
    assert_eq!(f.id, id);
    assert!(matches!(
        f.msg,
        ServerMsg::Failed {
            code: ErrCode::OutsideRoot,
            ..
        }
    ));
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

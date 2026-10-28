//! Heartbeats and connection loss.

use std::sync::Arc;
use std::time::Duration;

use socketsweep_daemon::outbox::OutboxConfig;
use socketsweep_daemon::session::End;
use socketsweep_engine::{Fs, StdFs};
use socketsweep_transport::Duplex;
use socketsweep_wire::{Caps, ClientMsg, ScanOpts, ServerFrame, ServerMsg, SESSION_LEVEL};

use super::support::*;

#[test]
fn an_idle_session_sends_heartbeats() {
    let tmp = fixture();
    let mut cfg = config();
    cfg.outbox = OutboxConfig {
        heartbeat: Duration::from_millis(30),
        ..OutboxConfig::default()
    };
    let (t, mut host) = connect(cfg, Arc::new(StdFs), &root_of(&tmp), Caps::all());
    for _ in 0..3 {
        assert_eq!(
            host.recv(),
            ServerFrame {
                id: SESSION_LEVEL,
                msg: ServerMsg::Heartbeat
            }
        );
    }
    host.send(ClientMsg::Shutdown);
    t.join().unwrap().unwrap();
}

#[test]
fn a_silent_host_is_dropped() {
    let tmp = fixture();
    let mut cfg = config();
    cfg.outbox = OutboxConfig {
        dead_after: Duration::from_millis(200),
        ..OutboxConfig::default()
    };
    let (t, host) = connect(cfg, Arc::new(StdFs), &root_of(&tmp), Caps::all());
    let end = t.join().unwrap().unwrap();
    assert!(
        matches!(&end, End::Closed(why) if why.contains("sent nothing")),
        "{end:?}"
    );
    drop(host);
}

#[test]
fn a_lost_connection_cancels_a_running_scan() {
    let tmp = fixture();
    let root = root_of(&tmp);
    let fs: Arc<dyn Fs> = Arc::new(SlowFs(Duration::from_millis(50)));
    let (t, mut host) = connect(config(), fs, &root, Caps::all());
    let id = host.send(ClientMsg::StartScan {
        root: root_bytes(&root),
        opts: ScanOpts::default(),
    });
    // The scan has started.
    loop {
        let f = host.recv();
        if f.id == id {
            break;
        }
    }
    host.out.shutdown().unwrap();
    drop(host);
    let end = t.join().unwrap().unwrap();
    assert!(matches!(end, End::Closed(_)), "{end:?}");
}

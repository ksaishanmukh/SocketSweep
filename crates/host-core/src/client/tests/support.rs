use super::super::*;
use crate::testkit::FakeDaemon;
use socketsweep_transport::fake::{FakeDevice, FakeTransport};
use socketsweep_transport::Transport;
use socketsweep_wire::{Caps, Outcome, ScanEvent, SessionId, Token, Totals, TOKEN_LEN};

pub(super) const TOKEN: Token = [7; TOKEN_LEN];
pub(super) const SESSION: SessionId = [1; 16];

pub(super) fn quick() -> ClientConfig {
    ClientConfig {
        ping_interval: Duration::from_millis(50),
        dead_after: Duration::from_millis(400),
        handshake_timeout: Duration::from_secs(5),
        tick: Duration::from_millis(10),
    }
}

/// Connect through `FakeTransport::open_tunnel` to `daemon`, which expects
/// `daemon_token`.
pub(super) fn connect_with(
    daemon: &FakeDaemon,
    daemon_token: Token,
    config: ClientConfig,
) -> Result<Client, ClientError> {
    let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
    let d = daemon.clone();
    t.on_tunnel("P1", "sock", move |end| d.serve(end, daemon_token));
    let stream = t.open_tunnel("P1", "sock").unwrap();
    Client::connect(stream, TOKEN, SESSION, Caps::all(), config)
}

pub(super) fn connect(daemon: &FakeDaemon) -> Client {
    connect_with(daemon, TOKEN, quick()).unwrap()
}

pub(super) fn idle() -> FakeDaemon {
    FakeDaemon::new(|_, _| {})
}

pub(super) fn finished(outcome: Outcome) -> ScanEvent {
    ScanEvent::Finished {
        outcome,
        totals: Totals::default(),
        elapsed_ms: 1,
    }
}

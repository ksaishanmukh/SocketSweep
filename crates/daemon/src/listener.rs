//! The abstract unix socket. Linux/Android only.
//!
//! Each connection is handshaken on a thread of its own, so one that connects
//! and says nothing cannot lock the host out. An **authenticated** new
//! connection replaces the current session (the host reconnecting after a
//! drop): the old connection is shut, which cancels everything it started.
//! A connection that fails the handshake changes nothing, and neither does
//! one that finishes its handshake after a newer connection already has: the
//! host retries while the daemon starts, so an abandoned early attempt can
//! complete late, and it must not shut the connection the host kept.

// Abstract-namespace sockets are a Linux kernel feature, but std files the
// extension trait under a per-OS module and Android is not `target_os = "linux"`.
#[cfg(target_os = "android")]
use std::os::android::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::log;
use crate::session::{self, Conn, End, HandshakeError, Services, SessionConfig};

/// How long a new connection has to send its `Hello`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Bind the abstract socket `name`.
pub fn bind(name: &str) -> std::io::Result<UnixListener> {
    let addr = SocketAddr::from_abstract_name(name.as_bytes())?;
    UnixListener::bind_addr(&addr)
}

fn conn_of(stream: &UnixStream) -> std::io::Result<Conn> {
    let reader = stream.try_clone()?;
    let writer = stream.try_clone()?;
    let ctl = stream.try_clone()?;
    Ok(Conn {
        reader: Box::new(reader),
        writer: Box::new(writer),
        shutdown: Arc::new(move || {
            let _ = ctl.shutdown(std::net::Shutdown::Both);
        }),
    })
}

/// The session being served: its generation and how to shut it.
type Active = Option<(u64, Arc<dyn Fn() + Send + Sync>)>;

/// The newest generation that ever became the session, and the session being
/// served now, if any.
type Current = Mutex<(u64, Active)>;

/// Whether a connection of generation `gen` that has just been authenticated
/// may become the session, given the newest generation that already did.
/// Generations count accepted connections, so a lower one is an older attempt
/// that finished late.
fn may_take_over(newest: u64, gen: u64) -> bool {
    gen >= newest
}

/// Accept connections on `listener` forever, serving one session at a time.
/// Returns when a session ends with `Shutdown`.
pub fn serve(listener: UnixListener, cfg: SessionConfig, services: Services) {
    let cfg = Arc::new(cfg);
    let current: Arc<Current> = Arc::new(Mutex::new((0, None)));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

    let accept = {
        let cfg = Arc::clone(&cfg);
        let current = Arc::clone(&current);
        let services = services.clone();
        std::thread::Builder::new()
            .name("ssd-accept".into())
            .spawn(move || {
                let mut generation = 0u64;
                for stream in listener.incoming() {
                    let stream = match stream {
                        Ok(s) => s,
                        Err(e) => {
                            log!("accept failed: {e}");
                            continue;
                        }
                    };
                    generation += 1;
                    let gen = generation;
                    let cfg = Arc::clone(&cfg);
                    let current = Arc::clone(&current);
                    let services = services.clone();
                    let done = done_tx.clone();
                    let spawned = std::thread::Builder::new()
                        .name("ssd-session".into())
                        .spawn(move || {
                            if connection(stream, gen, &cfg, services, &current)
                                == Some(End::Shutdown)
                            {
                                let _ = done.send(());
                            }
                        });
                    if let Err(e) = spawned {
                        log!("cannot start a session thread: {e}");
                    }
                }
            })
    };
    if let Err(e) = accept {
        log!("cannot start the accept thread: {e}");
        return;
    }
    // Wait for a `Shutdown`. The accept thread blocks in `accept` and is
    // ended with the process.
    let _ = done_rx.recv();
}

fn connection(
    stream: UnixStream,
    gen: u64,
    cfg: &SessionConfig,
    services: Services,
    current: &Current,
) -> Option<End> {
    // Socket options belong to the socket, so they cover every clone.
    if let Err(e) = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)) {
        log!("cannot set a socket timeout: {e}");
        return None;
    }
    let mut conn = match conn_of(&stream) {
        Ok(c) => c,
        Err(e) => {
            log!("cannot clone the connection: {e}");
            return None;
        }
    };
    let caps = match session::handshake(&mut conn, cfg) {
        Ok((_, caps)) => caps,
        Err(HandshakeError::Closed) => return None,
        Err(e) => {
            log!("connection {gen}: {e}");
            return None;
        }
    };
    // Silence is now the writer's job (DAEMON_DEAD_AFTER); a write may block
    // that long on a host that stopped reading.
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(Some(cfg.outbox.dead_after));

    let previous = {
        let mut slot = current.lock().unwrap_or_else(|e| e.into_inner());
        let (newest, active) = &mut *slot;
        if !may_take_over(*newest, gen) {
            log!("connection {gen} authenticated after newer connection {newest}; closing it");
            (conn.shutdown)();
            return None;
        }
        *newest = gen;
        active.replace((gen, Arc::clone(&conn.shutdown)))
    };
    if let Some((old, shut)) = previous {
        log!("connection {gen} replaces connection {old}");
        shut();
    }
    log!("connection {gen}: session started");
    let end = session::serve(conn, caps, cfg, services);
    log!("connection {gen}: {end:?}");
    let mut slot = current.lock().unwrap_or_else(|e| e.into_inner());
    if slot.1.as_ref().is_some_and(|(g, _)| *g == gen) {
        slot.1 = None;
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::may_take_over;

    #[test]
    fn a_newer_connection_takes_over() {
        assert!(may_take_over(0, 1));
        assert!(may_take_over(3, 7));
    }

    #[test]
    fn an_older_connection_that_finishes_late_does_not() {
        // The host gave up on attempt 2 and kept attempt 3; attempt 2's
        // handshake completing afterwards must not shut attempt 3.
        assert!(!may_take_over(3, 2));
    }
}

//! Opening a connection: `Hello`, the reply, and the threads that follow.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use socketsweep_transport::Duplex;
use socketsweep_wire::{
    encode_frame, read_frame, Caps, Hello, HelloReply, SessionId, Token, Welcome, WireError,
    PROTOCOL_VERSION,
};

use super::connection::{Inner, OnCloseSlot};
use super::threads::{keepalive_loop, reader_loop};
use super::{wire_err, Client, ClientConfig, ClientError};

impl Client {
    /// Send `Hello` on `stream`, read the reply, and start the reader and
    /// keepalive threads.
    ///
    /// A stream the daemon is not listening on yet ends at once, which is
    /// [`ClientError::Closed`], [`retryable`](ClientError::retryable).
    pub fn connect(
        stream: Box<dyn Duplex>,
        token: Token,
        session: SessionId,
        caps: Caps,
        config: ClientConfig,
    ) -> Result<Client, ClientError> {
        let closed = |e: std::io::Error| ClientError::Closed(e.to_string());
        stream
            .set_read_timeout(Some(config.handshake_timeout))
            .map_err(closed)?;
        stream
            .set_write_timeout(Some(config.dead_after))
            .map_err(closed)?;
        let mut writer = stream.try_clone().map_err(closed)?;
        let control = stream.try_clone().map_err(closed)?;
        let mut reader = BufReader::with_capacity(256 * 1024, stream);

        let hello = Hello::new(token, session, caps);
        writer
            .write_all(&encode_frame(&hello).map_err(wire_err)?)
            .and_then(|()| writer.flush())
            .map_err(closed)?;
        let welcome = read_welcome(&mut reader, control.as_ref(), session)?;
        reader.get_ref().set_read_timeout(None).map_err(closed)?;

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let now = Instant::now();
        let inner = Arc::new(Inner {
            writer: Mutex::new(writer),
            control: Mutex::new(control),
            routes: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            next_nonce: AtomicU32::new(1),
            last_rx: Mutex::new(now),
            last_tx: Mutex::new(now),
            closed: Mutex::new(None),
            deliberate: AtomicBool::new(false),
            on_close: Mutex::new(OnCloseSlot::default()),
            stop_keepalive: Mutex::new(Some(stop_tx)),
        });

        spawn_threads(&inner, reader, stop_rx, config)?;

        Ok(Client {
            inner,
            welcome: Arc::new(welcome),
        })
    }
}

/// Read the daemon's answer to `Hello` and check it is for this version and
/// `session`.
fn read_welcome(
    reader: &mut BufReader<Box<dyn Duplex>>,
    control: &dyn Duplex,
    session: SessionId,
) -> Result<Welcome, ClientError> {
    let reply = match read_frame::<_, HelloReply>(reader) {
        Ok(Some(reply)) => reply,
        Ok(None) => {
            return Err(ClientError::Closed(
                "the daemon closed the connection during the handshake".into(),
            ))
        }
        Err(WireError::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            let _ = control.shutdown();
            return Err(ClientError::Timeout);
        }
        Err(e) => return Err(wire_err(e)),
    };
    let welcome = match reply {
        HelloReply::Welcome(w) => w,
        HelloReply::Refused => return Err(ClientError::Refused),
        HelloReply::WrongVersion { version } => {
            return Err(ClientError::WrongVersion { daemon: version })
        }
    };
    if welcome.version != PROTOCOL_VERSION {
        return Err(ClientError::WrongVersion {
            daemon: welcome.version,
        });
    }
    if welcome.session != session {
        return Err(ClientError::SessionMismatch);
    }
    Ok(welcome)
}

/// Start the reader and keepalive threads of a new connection.
fn spawn_threads(
    inner: &Arc<Inner>,
    reader: BufReader<Box<dyn Duplex>>,
    stop_rx: Receiver<()>,
    config: ClientConfig,
) -> Result<(), ClientError> {
    let closed = |e: std::io::Error| ClientError::Closed(e.to_string());
    let r = Arc::clone(inner);
    std::thread::Builder::new()
        .name("socketsweep-reader".into())
        .spawn(move || reader_loop(r, reader))
        .map_err(closed)?;
    let k = Arc::clone(inner);
    std::thread::Builder::new()
        .name("socketsweep-keepalive".into())
        .spawn(move || keepalive_loop(k, stop_rx, config))
        .map_err(closed)?;
    Ok(())
}

//! The reader and keepalive threads of a connection.

use std::io::BufReader;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Instant;

use socketsweep_transport::Duplex;
use socketsweep_wire::{read_frame, ClientFrame, ClientMsg, ServerFrame};

use super::connection::Inner;
use super::ClientConfig;
use crate::lock;

pub(super) fn reader_loop(inner: Arc<Inner>, mut reader: BufReader<Box<dyn Duplex>>) {
    let why = loop {
        match read_frame::<_, ServerFrame>(&mut reader) {
            Ok(Some(frame)) => {
                *lock(&inner.last_rx) = Instant::now();
                inner.route(frame);
            }
            Ok(None) => break "the phone closed the connection".to_string(),
            Err(e) => break e.to_string(),
        }
    };
    // A reason recorded earlier (a deliberate close, a dead peer) wins.
    inner.close(why);
    lock(&inner.routes).clear();
    let why = lock(&inner.closed).clone().unwrap_or_default();
    if inner.deliberate.load(Ordering::SeqCst) {
        return;
    }
    let callback = {
        let mut slot = lock(&inner.on_close);
        slot.fired = Some(why.clone());
        slot.callback.take()
    };
    if let Some(callback) = callback {
        callback(why);
    }
}

pub(super) fn keepalive_loop(inner: Arc<Inner>, stop: Receiver<()>, config: ClientConfig) {
    loop {
        match stop.recv_timeout(config.tick) {
            Err(RecvTimeoutError::Timeout) => {}
            // Stopped, or the connection is gone.
            _ => return,
        }
        let silent = lock(&inner.last_rx).elapsed();
        if silent >= config.dead_after {
            inner.close(format!(
                "the phone has not answered for {} s",
                silent.as_secs()
            ));
            return;
        }
        if lock(&inner.last_tx).elapsed() >= config.ping_interval {
            let id = inner.next_id();
            let nonce = inner.next_nonce.fetch_add(1, Ordering::Relaxed);
            // Nobody waits for the Pong: any frame resets `last_rx`.
            if inner
                .send(&ClientFrame {
                    id,
                    msg: ClientMsg::Ping { nonce },
                })
                .is_err()
            {
                return;
            }
        }
    }
}

//! Serving a connection: the reader loop, the writer thread, and stopping
//! everything when the connection ends.

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use socketsweep_engine::Scan;
use socketsweep_wire::{read_frame, Caps, ClientFrame};

use super::{lock, Active, Conn, End, Services, SessionConfig, Shared};
use crate::outbox::{Outbox, ScanSink};

/// Serve an authenticated connection until the host sends `Shutdown`, the
/// connection ends, or the host goes silent. Cancels and waits for
/// everything the session started before returning, then shuts the
/// connection.
pub fn serve(conn: Conn, caps: Caps, cfg: &SessionConfig, services: Services) -> End {
    let Conn {
        reader,
        writer,
        shutdown,
    } = conn;
    let outbox = Outbox::new(cfg.outbox.clone());
    let shared = Arc::new(Shared {
        outbox: Arc::clone(&outbox),
        services,
        cfg: cfg.clone(),
        caps,
        active: Mutex::new(HashMap::new()),
        threads: Mutex::new(Vec::new()),
    });

    let writer_thread = match spawn_writer(&outbox, writer, &shutdown) {
        Ok(t) => t,
        Err(e) => {
            shutdown();
            return End::Closed(format!("cannot start the writer thread: {e}"));
        }
    };

    let end = read_frames(reader, &outbox, &shared);

    shared.cancel_all();
    let threads = std::mem::take(&mut *lock(&shared.threads));
    for t in threads {
        let _ = t.join();
    }
    match &end {
        // Send what is already queued (the cancelled scans' `Finished`),
        // then stop.
        End::Shutdown => outbox.finish(),
        End::Closed(why) => outbox.close(why.clone()),
    }
    let _ = writer_thread.join();
    shutdown();
    end
}

/// Start the writer thread; it calls `shutdown` when the host is gone.
fn spawn_writer(
    outbox: &Arc<Outbox>,
    writer: Box<dyn Write + Send>,
    shutdown: &Arc<dyn Fn() + Send + Sync>,
) -> std::io::Result<JoinHandle<String>> {
    let ob = Arc::clone(outbox);
    let on_dead = Arc::clone(shutdown);
    std::thread::Builder::new()
        .name("ssd-writer".into())
        .spawn(move || ob.run_writer(writer, move || on_dead()))
}

/// The reader: route frames until `Shutdown` or the connection ends.
fn read_frames(reader: Box<dyn Read + Send>, outbox: &Outbox, shared: &Arc<Shared>) -> End {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    loop {
        match read_frame::<_, ClientFrame>(&mut reader) {
            Ok(Some(frame)) => {
                outbox.touch_rx();
                if shared.dispatch(frame) {
                    break End::Shutdown;
                }
            }
            Ok(None) => {
                break End::Closed(
                    outbox
                        .closed()
                        .unwrap_or_else(|| "the host closed the connection".into()),
                )
            }
            Err(e) => break End::Closed(outbox.closed().unwrap_or_else(|| e.to_string())),
        }
    }
}

impl Shared {
    /// The connection is ending: stop everything it started.
    fn cancel_all(&self) {
        let scans: Vec<Scan<ScanSink>> = lock(&self.active)
            .values()
            .filter_map(|a| match a {
                Active::Scan(s) => Some(s.clone()),
                Active::Job(flag) => {
                    flag.store(true, Ordering::Relaxed);
                    None
                }
            })
            .collect();
        for s in scans {
            s.cancel();
        }
    }
}

use super::*;
use socketsweep_wire::{read_frame, Outcome, Totals, SESSION_LEVEL};
use std::io::{Cursor, Write};
use std::sync::mpsc;

fn ev(i: u32) -> ScanEvent {
    ScanEvent::Pruned { dir: i }
}

fn quick() -> OutboxConfig {
    OutboxConfig {
        heartbeat: Duration::from_millis(40),
        dead_after: Duration::from_secs(30),
        flush_every: Duration::from_millis(5),
        ..OutboxConfig::default()
    }
}

fn frames(bytes: &[u8]) -> Vec<ServerFrame> {
    let mut r = Cursor::new(bytes);
    let mut out = Vec::new();
    while let Some(f) = read_frame::<_, ServerFrame>(&mut r).unwrap() {
        out.push(f);
    }
    out
}

/// A writer target that shares what was written.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);
impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        lock(&self.0).extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_batch_frame_is_byte_identical_to_encode_frame() {
    for (id, n) in [(1u32, 0usize), (7, 1), (300, 5), (u32::MAX, 200)] {
        let events: Vec<ScanEvent> = (0..n as u32)
            .map(|i| {
                if i % 2 == 0 {
                    ev(i * 1000)
                } else {
                    ScanEvent::Finished {
                        outcome: Outcome::Complete,
                        totals: Totals::default(),
                        elapsed_ms: i,
                    }
                }
            })
            .collect();
        let encoded: Vec<Vec<u8>> = events
            .iter()
            .map(|e| postcard::to_allocvec(e).unwrap())
            .collect();
        let expected = encode_frame(&ServerFrame {
            id,
            msg: ServerMsg::Scan(events),
        })
        .unwrap();
        assert_eq!(scan_frame(id, &encoded), expected, "id {id}, {n} events");
    }
}

#[test]
fn control_goes_before_bulk_queued_earlier() {
    let ob = Outbox::new(quick());
    ob.bulk(5, (0..100).map(ev).collect());
    ob.reply(9, ServerMsg::Pong { nonce: 3 });
    ob.finish();
    let out = Shared::default();
    ob.run_writer(out.clone(), || {});
    let got = frames(&lock(&out.0));
    assert_eq!(
        got[0],
        ServerFrame {
            id: 9,
            msg: ServerMsg::Pong { nonce: 3 }
        }
    );
    let events: Vec<ScanEvent> = got[1..]
        .iter()
        .flat_map(|f| match &f.msg {
            ServerMsg::Scan(e) => {
                assert_eq!(f.id, 5);
                e.clone()
            }
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(events, (0..100).map(ev).collect::<Vec<_>>());
}

#[test]
fn batches_respect_the_size_bound_and_scan_boundaries() {
    let ob = Outbox::new(OutboxConfig {
        max_batch: 64,
        ..quick()
    });
    ob.bulk(1, (0..50).map(ev).collect());
    ob.bulk(2, (0..3).map(ev).collect());
    ob.bulk(1, (50..53).map(ev).collect());
    ob.finish();
    let out = Shared::default();
    ob.run_writer(out.clone(), || {});
    let got = frames(&lock(&out.0));
    let mut seq = Vec::new();
    for f in &got {
        let ServerMsg::Scan(events) = &f.msg else {
            panic!("unexpected {f:?}")
        };
        let bytes: usize = events
            .iter()
            .map(|e| postcard::to_allocvec(e).unwrap().len())
            .sum();
        assert!(bytes <= 64, "batch of {bytes} bytes");
        seq.extend(events.iter().map(|e| (f.id, e.clone())));
    }
    let mut expected: Vec<(ReqId, ScanEvent)> = (0..50).map(|i| (1, ev(i))).collect();
    expected.extend((0..3).map(|i| (2, ev(i))));
    expected.extend((50..53).map(|i| (1, ev(i))));
    assert_eq!(seq, expected);
    assert!(got.len() > 3);
}

#[test]
fn congestion_turns_on_above_high_water_and_off_below_low_water() {
    let ob = Outbox::new(OutboxConfig {
        high_water: 100,
        low_water: 50,
        ..quick()
    });
    // Each Pruned event is two or three bytes.
    ob.bulk(1, (0..40).map(ev).collect());
    assert!(!ob.congested());
    ob.bulk(1, (0..40).map(ev).collect());
    assert!(ob.congested());
    ob.finish();
    ob.run_writer(Shared::default(), || {});
    assert!(!ob.congested());
    assert_eq!(ob.bulk_bytes(), 0);
}

#[test]
fn a_heartbeat_is_sent_after_idle_time() {
    let ob = Outbox::new(quick());
    let out = Shared::default();
    let (tx, rx) = mpsc::channel();
    let o = Arc::clone(&ob);
    let w = out.clone();
    std::thread::spawn(move || tx.send(o.run_writer(w, || {})).unwrap());
    std::thread::sleep(Duration::from_millis(150));
    ob.close("test over");
    assert_eq!(rx.recv().unwrap(), "test over");
    let got = frames(&lock(&out.0));
    assert!(got.len() >= 2, "{got:?}");
    assert!(got
        .iter()
        .all(|f| f.id == SESSION_LEVEL && f.msg == ServerMsg::Heartbeat));
}

#[test]
fn a_silent_host_is_given_up() {
    let ob = Outbox::new(OutboxConfig {
        dead_after: Duration::from_millis(60),
        ..quick()
    });
    let dead = Arc::new(AtomicBool::new(false));
    let d = Arc::clone(&dead);
    let why = ob.run_writer(Shared::default(), move || d.store(true, Ordering::SeqCst));
    assert!(why.contains("sent nothing"), "{why}");
    assert!(dead.load(Ordering::SeqCst));
    // Nothing is queued after the end.
    ob.reply(1, ServerMsg::Pong { nonce: 1 });
    ob.bulk(1, vec![ev(1)]);
    assert_eq!(ob.bulk_bytes(), 0);
}

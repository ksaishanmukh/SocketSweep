//! A batch frame assembled from events already encoded one by one.

use socketsweep_wire::ReqId;

/// A whole `ServerFrame { id, msg: Scan(events) }` frame, length prefix
/// included, from events already encoded one by one.
pub fn scan_frame(id: ReqId, events: &[Vec<u8>]) -> Vec<u8> {
    let body_len: usize = events.iter().map(Vec::len).sum();
    let mut frame = Vec::with_capacity(body_len + 16);
    frame.extend_from_slice(&[0; 4]);
    push_varint(&mut frame, u64::from(id));
    // `ServerMsg::Scan` is variant 2: Pong, Heartbeat, Scan.
    push_varint(&mut frame, 2);
    push_varint(&mut frame, events.len() as u64);
    for e in events {
        frame.extend_from_slice(e);
    }
    let len = (frame.len() - 4) as u32;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    frame
}

fn push_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

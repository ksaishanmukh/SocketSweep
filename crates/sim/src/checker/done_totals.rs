//! Per-path totals of a finished stream.

use socketsweep_wire::ScanEvent;

/// The totals of every `SubtreeDone` in a stream, by path (built from the
/// `Started` root and the listings, as a host would).
pub fn done_totals<'a>(
    events: impl IntoIterator<Item = &'a ScanEvent>,
) -> Vec<(Vec<u8>, socketsweep_wire::Totals)> {
    let mut paths: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::new();
    for e in events {
        match e {
            ScanEvent::Started { root, .. } => paths = vec![root.clone()],
            ScanEvent::Listing { dir, entries, .. } => {
                let base = paths[*dir as usize].clone();
                for en in entries {
                    if let Some(id) = en.id {
                        let mut p = base.clone();
                        if p.last() != Some(&b'/') {
                            p.push(b'/');
                        }
                        p.extend_from_slice(&en.name);
                        if paths.len() <= id as usize {
                            paths.resize(id as usize + 1, Vec::new());
                        }
                        paths[id as usize] = p;
                    }
                }
            }
            ScanEvent::SubtreeDone { dir, totals } => {
                out.push((paths[*dir as usize].clone(), *totals));
            }
            _ => {}
        }
    }
    out
}

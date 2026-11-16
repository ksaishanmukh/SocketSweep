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
                for (id, name) in entries.iter().filter_map(|en| Some((en.id?, &en.name))) {
                    let id = id as usize;
                    paths.resize(paths.len().max(id + 1), Vec::new());
                    paths[id] = join_path(&base, name);
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

/// `base/name`, without doubling a trailing slash (the root may be `/`).
pub fn join_path(base: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = base.to_vec();
    if p.last() != Some(&b'/') {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

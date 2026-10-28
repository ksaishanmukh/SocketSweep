use super::super::*;
use socketsweep_wire::{DirId, Entry, ScanEvent, Stat, BLOCK_SIZE};

pub(super) const S: SessionId = 7;

pub(super) fn file(name: &str, apparent: u64) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: Kind::File,
        stat: Some(stat(apparent)),
        id: None,
    }
}

/// A file whose stat is deferred.
pub(super) fn bare(name: &str) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: Kind::File,
        stat: None,
        id: None,
    }
}

pub(super) fn dir(name: &str, id: DirId) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        kind: Kind::Dir,
        stat: Some(Stat {
            apparent: 0,
            blocks: 0,
            mtime: 0,
        }),
        id: Some(id),
    }
}

/// `apparent` bytes using whole 512-byte blocks.
pub(super) fn stat(apparent: u64) -> Stat {
    Stat {
        apparent,
        blocks: apparent.div_ceil(BLOCK_SIZE),
        mtime: 0,
    }
}

pub(super) fn listing(d: DirId, entries: Vec<Entry>) -> ScanEvent {
    ScanEvent::Listing {
        dir: d,
        part: 0,
        last: true,
        entries,
    }
}

pub(super) fn done(d: DirId, apparent: u64, files: u32, dirs: u32) -> ScanEvent {
    ScanEvent::SubtreeDone {
        dir: d,
        totals: Totals {
            apparent,
            disk: apparent,
            files,
            dirs,
            ..Totals::default()
        },
    }
}

pub(super) fn tree() -> Tree {
    Tree::new(S, b"/storage/emulated/0", "/sdcard")
}

pub(super) fn apply(t: &mut Tree, events: &[ScanEvent]) {
    for e in events {
        t.ingest(e).unwrap_or_else(|err| panic!("{e:?}: {err}"));
    }
}

pub(super) fn child(t: &Tree, parent: NodeId, name: &str) -> NodeId {
    t.view(parent, 1000, SizeMetric::Apparent)
        .unwrap()
        .rows
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .node
        .id
}

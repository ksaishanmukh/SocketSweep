//! The threaded pool over the real file system (`StdFs`) on a temporary
//! tree: every directory listed once, totals match what was written.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use socketsweep_engine::{Fs, RecordingSink, Scan, StdFs};
use socketsweep_wire::{Outcome, ScanEvent, ScanOpts};

struct TempTree(PathBuf);

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `width` subdirectories per level, `depth` levels, `files` files of
/// `size` bytes each in every directory. Returns (dirs below root, files,
/// apparent bytes).
fn build(dir: &Path, depth: u32, width: u32, files: u32, size: usize) -> (u32, u32, u64) {
    let (mut d, mut f, mut b) = (0, 0, 0);
    for i in 0..files {
        fs::write(dir.join(format!("f{i}.bin")), vec![7u8; size]).unwrap();
        f += 1;
        b += size as u64;
    }
    if depth > 0 {
        for i in 0..width {
            let sub = dir.join(format!("d{i}"));
            fs::create_dir(&sub).unwrap();
            d += 1;
            let (sd, sf, sb) = build(&sub, depth - 1, width, files, size);
            d += sd;
            f += sf;
            b += sb;
        }
    }
    (d, f, b)
}

#[test]
fn stdfs_scan_matches_what_was_written() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("socketsweep-engine-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let tree = TempTree(root.clone());
    let (dirs, files, bytes) = build(&tree.0, 3, 3, 4, 1000);

    let path = root.to_string_lossy().replace('\\', "/").into_bytes();
    let rootinfo = StdFs.root(&path).unwrap();
    let sink = Arc::new(RecordingSink::new());
    let opts = ScanOpts {
        workers: Some(4),
        ..ScanOpts::default()
    };
    let scan = Scan::new(rootinfo, &opts, Arc::clone(&sink));
    assert_eq!(scan.run(&StdFs), Outcome::Complete);

    let events = sink.events();
    let Some(ScanEvent::Finished { totals, .. }) = events.last() else {
        panic!("no Finished");
    };
    assert_eq!(totals.files, files);
    assert_eq!(totals.dirs, dirs);
    assert_eq!(totals.errors, 0);
    // Directories' own sizes are platform-dependent; files are exact.
    assert!(totals.apparent >= bytes, "{} < {bytes}", totals.apparent);
    let lists = events
        .iter()
        .filter(|e| matches!(e, ScanEvent::Listing { last: true, .. }))
        .count();
    assert_eq!(lists as u32, dirs + 1, "every directory listed once");
    let done = events
        .iter()
        .filter(|e| matches!(e, ScanEvent::SubtreeDone { .. }))
        .count();
    assert_eq!(done as u32, dirs + 1, "every directory finished once");
}

#[test]
fn stdfs_root_errors() {
    assert!(StdFs.root(b"/definitely/not/here/socketsweep").is_err());
}

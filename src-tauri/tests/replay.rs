//! Replays a recorded device frame stream through the arena.
//!
//! Ignored by default because it needs a capture from a real phone, which CI
//! does not have. It exists because the unit tests cannot model the thing that
//! actually broke: on a device `/sdcard` is a symlink chain to
//! `/storage/emulated/0`, the daemon canonicalises before walking, and every
//! frame arrives keyed on the resolved path. A host that built its index from
//! the requested path placed exactly zero frames.
//!
//! Capture a stream with the scratch probe, then:
//!
//!   SOCKETSWEEP_FRAMES=/path/to/frames.bin \
//!     cargo test -p socketsweep --test replay -- --ignored --nocapture

use std::io::BufReader;
use std::time::Instant;

use socketsweep_lib::arena::{Arena, ROOT};
use socketsweep_protocol::{read_msg, Frame};

#[test]
#[ignore = "needs a frame capture from a real device; set SOCKETSWEEP_FRAMES"]
fn a_recorded_device_scan_builds_a_consistent_tree() {
    let path = std::env::var("SOCKETSWEEP_FRAMES")
        .expect("set SOCKETSWEEP_FRAMES to a captured frame stream");
    let file = std::fs::File::open(&path).expect("open the capture");
    let mut reader = BufReader::with_capacity(1 << 20, file);

    let mut tree: Option<Arena> = None;
    let mut reported = None;
    let mut applied = 0u64;

    let started = Instant::now();
    while let Some(frame) = read_msg::<_, Frame>(&mut reader).expect("decode frame") {
        match frame {
            Frame::ScanStarted { root } => {
                println!("root: {}", String::from_utf8_lossy(&root));
                tree = Some(Arena::new(&root));
            }
            Frame::Dir { path, entries } => {
                let arena = tree
                    .as_mut()
                    .expect("ScanStarted must arrive before any Dir frame");
                // The failure this test exists for shows up here, as OrphanFrame.
                arena
                    .apply_dir(&path, &entries)
                    .unwrap_or_else(|e| panic!("frame {applied} rejected: {e}"));
                applied += 1;
            }
            Frame::ScanDone(stats) => {
                reported = Some(stats);
                tree.as_mut().expect("tree").finish();
            }
            other => panic!("unexpected frame in a scan stream: {other:?}"),
        }
    }
    let ingest = started.elapsed();

    let arena = tree.expect("stream contained no ScanStarted");
    let reported = reported.expect("stream contained no ScanDone");
    let stats = arena.stats();

    println!("applied {applied} directory frames in {ingest:?}");
    println!(
        "arena: {} bytes, {} files, {} dirs",
        stats.size, stats.files, stats.dirs
    );

    // The host aggregates independently of the daemon's own counters, so these
    // agreeing is a real cross-check rather than a tautology.
    assert_eq!(stats.size, reported.total_size, "total size disagrees");
    assert_eq!(
        u64::from(stats.files),
        reported.files,
        "file count disagrees"
    );
    assert_eq!(
        u64::from(stats.dirs),
        reported.dirs,
        "directory count disagrees"
    );
    assert!(
        !arena.scanning(),
        "every announced directory should be listed"
    );

    // The root view must be sorted largest-first and sum to no more than the total.
    let view = arena.view(ROOT, 500).expect("root view");
    let mut previous = u64::MAX;
    for row in &view.rows {
        assert!(row.size <= previous, "rows are not largest-first");
        previous = row.size;
    }
    assert_eq!(view.size, stats.size, "root view disagrees with the total");

    // Cross-tree queries against real data.
    let largest = arena.largest_files(20);
    assert!(!largest.is_empty(), "a 100k-file device has largest files");
    assert!(
        largest.iter().all(|r| !r.is_dir && r.parent.is_some()),
        "largest files must be files, and must say where they live"
    );

    let types = arena.type_breakdown();
    let by_type: u64 = types.iter().map(|g| g.size).sum();
    assert_eq!(
        by_type, stats.size,
        "every byte must land in exactly one category"
    );

    println!(
        "largest file: {} ({} bytes)",
        largest[0].name, largest[0].size
    );
    for g in &types {
        println!(
            "  {:<10} {:>14} bytes  {:>7} files",
            g.label, g.size, g.files
        );
    }
}

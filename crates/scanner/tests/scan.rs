//! Scan engine tests.
//!
//! These run against a temp directory on any development machine, which is why
//! the traversal logic lives in its own crate rather than inside the daemon.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use socketsweep_protocol::{read_msg, Entry, EntryKind, Frame, ScanStats};
use socketsweep_scanner::{scan, ScanConfig, ScanError};

/// `scan` takes an owned `'static` sink, so tests pass a handle to a shared
/// buffer they can read back once the walk finishes.
#[derive(Clone)]
struct SharedSink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl SharedSink {
    fn new() -> Self {
        SharedSink(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
    fn take(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl std::io::Write for SharedSink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn decode(bytes: &[u8]) -> Vec<Frame> {
    let mut cursor = bytes;
    let mut frames = Vec::new();
    while let Some(f) = read_msg::<_, Frame>(&mut cursor).expect("decode failed") {
        frames.push(f);
    }
    frames
}

fn scan_with(cfg: &ScanConfig) -> (ScanStats, Vec<Frame>) {
    let sink = SharedSink::new();
    let stats = scan(cfg, sink.clone()).expect("scan failed");
    (stats, decode(&sink.take()))
}

/// Run a scan and decode every frame it produced.
fn run(root: &Path, threads: usize) -> (ScanStats, Vec<Frame>) {
    scan_with(&ScanConfig {
        root: root.to_path_buf(),
        threads,
        max_depth: 64,
    })
}

fn dir_frames(frames: &[Frame]) -> HashMap<PathBuf, Vec<Entry>> {
    frames
        .iter()
        .map(|f| match f {
            Frame::Dir { path, entries } => (
                PathBuf::from(String::from_utf8_lossy(path).into_owned()),
                entries.clone(),
            ),
            other => panic!("unexpected frame {other:?}"),
        })
        .collect()
}

/// A tree with known contents:
///   root/
///     a.txt                 10 bytes
///     .hidden               5 bytes
///     DCIM/
///       Camera/
///         img.jpg           100 bytes
///     empty/
///     Download/
///       big.bin             1000 bytes
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path();

    fs::create_dir_all(r.join("DCIM/Camera")).unwrap();
    fs::create_dir_all(r.join("empty")).unwrap();
    fs::create_dir_all(r.join("Download")).unwrap();

    fs::write(r.join("a.txt"), vec![b'x'; 10]).unwrap();
    fs::write(r.join(".hidden"), vec![b'x'; 5]).unwrap();
    fs::write(r.join("DCIM/Camera/img.jpg"), vec![b'x'; 100]).unwrap();
    fs::write(r.join("Download/big.bin"), vec![b'x'; 1000]).unwrap();

    tmp
}

#[test]
fn counts_and_totals_are_exact() {
    let tmp = fixture();
    let (stats, _) = run(tmp.path(), 1);

    assert_eq!(stats.files, 4, "a.txt, .hidden, img.jpg, big.bin");
    assert_eq!(stats.dirs, 5, "root, DCIM, DCIM/Camera, empty, Download");
    assert_eq!(stats.total_size, 10 + 5 + 100 + 1000);
    assert_eq!(stats.errors, 0);
}

#[test]
fn hidden_files_are_counted() {
    // They occupy real storage, so a storage analyser that ignores them lies.
    let tmp = fixture();
    let (_, frames) = run(tmp.path(), 1);
    let dirs = dir_frames(&frames);
    let root = &dirs[&PathBuf::from(tmp.path().to_string_lossy().into_owned())];
    assert!(root.iter().any(|e| e.name == b".hidden"));
}

#[test]
fn every_directory_produces_exactly_one_frame() {
    let tmp = fixture();
    let (_, frames) = run(tmp.path(), 4);
    assert_eq!(
        frames.len(),
        5,
        "one frame per directory including the root"
    );

    let dirs = dir_frames(&frames);
    assert_eq!(dirs.len(), 5, "no duplicate directory frames");
}

#[test]
fn empty_directories_still_produce_a_frame() {
    let tmp = fixture();
    let (_, frames) = run(tmp.path(), 1);
    let dirs = dir_frames(&frames);
    let empty = dirs
        .get(&tmp.path().join("empty"))
        .expect("the empty directory must still be reported");
    assert!(empty.is_empty());
}

#[test]
fn directory_entries_carry_zero_size_and_files_carry_real_size() {
    let tmp = fixture();
    let (_, frames) = run(tmp.path(), 1);
    let dirs = dir_frames(&frames);

    let download = &dirs[&tmp.path().join("Download")];
    let big = download.iter().find(|e| e.name == b"big.bin").unwrap();
    assert_eq!(big.kind, EntryKind::File);
    assert_eq!(big.size, 1000);

    let root = &dirs[&PathBuf::from(tmp.path().to_string_lossy().into_owned())];
    let dcim = root.iter().find(|e| e.name == b"DCIM").unwrap();
    assert_eq!(dcim.kind, EntryKind::Dir);
    assert_eq!(
        dcim.size, 0,
        "subtree totals are the host's job, not the daemon's"
    );
}

/// The host builds its tree incrementally and never buffers, which is only
/// sound if a directory is announced by its parent before its own frame arrives.
/// Asserted at high parallelism, where the ordering would break if it could.
#[test]
fn a_directory_is_always_announced_by_its_parent_first() {
    let tmp = tempfile::tempdir().unwrap();
    // Wide and deep enough that the work spreads across threads.
    for i in 0..12 {
        for j in 0..12 {
            let d = tmp
                .path()
                .join(format!("d{i}"))
                .join(format!("s{j}"))
                .join("leaf");
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("f.bin"), vec![b'x'; 16]).unwrap();
        }
    }

    for threads in [1, 2, 8, 16] {
        let (_, frames) = run(tmp.path(), threads);

        let mut announced: HashSet<PathBuf> = HashSet::new();
        announced.insert(tmp.path().to_path_buf()); // the root is implied by the request

        for frame in &frames {
            let Frame::Dir { path, entries } = frame else {
                panic!()
            };
            let path = PathBuf::from(String::from_utf8_lossy(path).into_owned());

            assert!(
                announced.contains(&path),
                "frame for {} arrived before its parent announced it (threads={threads})",
                path.display(),
            );

            for e in entries {
                if e.kind == EntryKind::Dir {
                    let name = String::from_utf8_lossy(&e.name).into_owned();
                    announced.insert(path.join(name));
                }
            }
        }
    }
}

#[test]
fn results_are_identical_regardless_of_thread_count() {
    let tmp = fixture();
    let (baseline, base_frames) = run(tmp.path(), 1);
    let base_dirs = dir_frames(&base_frames);

    for threads in [2, 4, 8, 16] {
        let (stats, frames) = run(tmp.path(), threads);
        assert_eq!(stats.files, baseline.files, "threads={threads}");
        assert_eq!(stats.dirs, baseline.dirs, "threads={threads}");
        assert_eq!(stats.total_size, baseline.total_size, "threads={threads}");

        // Same directories, same contents — only frame order may differ.
        let dirs = dir_frames(&frames);
        assert_eq!(dirs.len(), base_dirs.len(), "threads={threads}");
        for (path, mut entries) in dirs {
            let mut expected = base_dirs[&path].clone();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            expected.sort_by(|a, b| a.name.cmp(&b.name));
            assert_eq!(entries, expected, "contents of {} differ", path.display());
        }
    }
}

#[test]
fn a_missing_root_is_reported_not_silently_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ScanConfig::new(tmp.path().join("does-not-exist"));
    let err = scan(&cfg, Vec::new()).unwrap_err();
    assert!(matches!(err, ScanError::BadRoot { .. }), "got {err:?}");
}

#[test]
fn a_file_as_root_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("f.txt");
    fs::write(&file, b"x").unwrap();

    let cfg = ScanConfig::new(&file);
    let err = scan(&cfg, Vec::new()).unwrap_err();
    assert!(matches!(err, ScanError::BadRoot { .. }), "got {err:?}");
}

#[test]
fn max_depth_is_respected() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("a/b/c/d/e")).unwrap();
    fs::write(tmp.path().join("a/b/c/d/e/deep.txt"), b"x").unwrap();

    let cfg = ScanConfig {
        root: tmp.path().to_path_buf(),
        threads: 1,
        max_depth: 2,
    };
    let (stats, _) = scan_with(&cfg);

    assert_eq!(stats.files, 0, "deep.txt sits below the depth limit");
    assert!(stats.dirs < 6);
}

#[cfg(unix)]
#[test]
fn symlinks_are_skipped_rather_than_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("f.bin"), vec![b'x'; 42]).unwrap();

    // A link back to the parent would loop forever if followed.
    std::os::unix::fs::symlink(tmp.path(), tmp.path().join("loop")).unwrap();
    std::os::unix::fs::symlink(real.join("f.bin"), tmp.path().join("alias")).unwrap();

    let (stats, frames) = run(tmp.path(), 4);

    assert_eq!(stats.files, 1, "the aliased file must not be counted twice");
    assert_eq!(stats.total_size, 42);

    let dirs = dir_frames(&frames);
    let root = &dirs[&tmp.path().to_path_buf()];
    assert!(!root.iter().any(|e| e.name == b"loop"));
    assert!(!root.iter().any(|e| e.name == b"alias"));
}

#[cfg(unix)]
#[test]
fn non_utf8_filenames_survive_the_scan() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let tmp = tempfile::tempdir().unwrap();
    let raw = OsStr::from_bytes(&[b'b', b'a', b'd', 0xFF, 0xFE]);
    fs::write(tmp.path().join(raw), vec![b'x'; 7]).unwrap();

    let (stats, frames) = run(tmp.path(), 1);
    assert_eq!(stats.files, 1);

    let Frame::Dir { entries, .. } = &frames[0] else {
        panic!()
    };
    assert_eq!(entries[0].name, vec![b'b', b'a', b'd', 0xFF, 0xFE]);
}

#[test]
fn a_wide_directory_is_handled_in_one_frame() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..5_000 {
        fs::write(tmp.path().join(format!("f{i:05}.bin")), b"xx").unwrap();
    }

    let (stats, frames) = run(tmp.path(), 8);
    assert_eq!(stats.files, 5_000);
    assert_eq!(stats.total_size, 10_000);
    assert_eq!(frames.len(), 1);

    let Frame::Dir { entries, .. } = &frames[0] else {
        panic!()
    };
    assert_eq!(entries.len(), 5_000);
}

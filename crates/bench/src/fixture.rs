//! The seeded fixture tree of `docs/architecture.md`, section 6: 50,000
//! files in 3,000 directories, one of which holds 30,000 files, shaped like
//! a small `/sdcard`.
//!
//! The tree is built with the simulator's [`TreeBuilder`] and [`Rng`], so the
//! same seed gives the same tree on the device (pushed as a tar archive and
//! extracted there), in [`SimFs`] for offline tests, and in the end-to-end
//! test's expectations. File contents are pseudo-random bytes derived from
//! the seed and the path, so a copy can be checked byte for byte without
//! keeping the archive.

use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use socketsweep_sim::{NodeId, Rng, SimFs, TreeBuilder};
use socketsweep_transport::Transport;
use socketsweep_wire::Kind;

/// The fixture's folder name, created under the parent (`/sdcard`).
pub const FIXTURE_DIR: &str = "ssfixture";
/// Where the archive is staged on the phone before extraction.
pub const DEVICE_TAR: &str = "/data/local/tmp/ssfixture.tar";

pub const FILES: u32 = 50_000;
/// Directories below the fixture root (the root itself not counted).
pub const DIRS: u32 = 3_000;
/// Files in the one big directory (`Movies/.thumbcache`).
pub const BIG_FILES: u32 = 30_000;
/// The levels of the `Documents/deep` chain.
const CHAIN: u32 = 30;

/// What a complete scan of the fixture must report, apart from the
/// directories' own sizes (which depend on the phone's file system).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    pub files: u32,
    pub dirs: u32,
    /// The sum of the files' lengths.
    pub file_bytes: u64,
}

/// Counts what [`build`] creates.
struct Builder {
    b: TreeBuilder,
    dirs: u32,
    files: u32,
}

impl Builder {
    fn dir(&mut self, parent: NodeId, name: String) -> NodeId {
        self.dirs += 1;
        self.b.dir(parent, name)
    }

    fn file(&mut self, parent: NodeId, name: String, size: u64) {
        self.files += 1;
        self.b.file(parent, name, size);
    }
}

/// The fixture for `seed`, rooted at `root` (e.g. `/sdcard/ssfixture`).
pub fn build(seed: u64, root: &[u8]) -> SimFs {
    let mut rng = Rng::new(seed);
    let mut b = Builder {
        b: TreeBuilder::new(),
        dirs: 0,
        files: 0,
    };
    // Directories that get a share of the randomly placed files.
    let mut pool: Vec<NodeId> = Vec::new();

    let android = b.dir(0, "Android".into());
    let data = b.dir(android, "data".into());
    for p in 0..200 {
        let pkg = b.dir(data, format!("com.example.app{p:03}"));
        let cache = b.dir(pkg, "cache".into());
        let files = b.dir(pkg, "files".into());
        pool.extend([cache, files]);
    }

    let movies = b.dir(0, "Movies".into());
    let thumbs = b.dir(movies, ".thumbcache".into());
    for f in 0..BIG_FILES {
        let s = rng.range(100, 1_500);
        b.file(thumbs, format!("t{f:05}"), s);
    }
    for f in 0..5 {
        let s = rng.range(1_000_000, 3_000_000);
        b.file(movies, format!("movie{f}.mp4"), s);
    }

    let dcim = b.dir(0, "DCIM".into());
    let cam = b.dir(dcim, "Camera".into());
    for f in 0..4_000 {
        let s = rng.range(1_000, 6_000);
        b.file(cam, format!("IMG_{f:05}.jpg"), s);
    }

    let music = b.dir(0, "Music".into());
    for a in 0..40 {
        let artist = b.dir(music, format!("Artist{a:02}"));
        for al in 0..2 {
            let album = b.dir(artist, format!("Album{al}"));
            for t in 0..12 {
                let s = rng.range(2_000, 8_000);
                b.file(album, format!("{t:02}.mp3"), s);
            }
        }
    }

    let dl = b.dir(0, "Download".into());
    for f in 0..300 {
        let s = rng.size(4_000, 1_000_000);
        b.file(dl, format!("download{f:03}.bin"), s);
    }
    for d in 0..30 {
        let x = b.dir(dl, format!("folder{d:02}"));
        for f in 0..20 {
            let s = rng.size(3_000, 200_000);
            b.file(x, format!("f{f:02}"), s);
        }
    }

    let docs = b.dir(0, "Documents".into());
    pool.push(docs);
    let mut deep = b.dir(docs, "deep".into());
    for l in 0..CHAIN {
        b.file(deep, format!("level{l}.txt"), 100 + u64::from(l));
        deep = b.dir(deep, format!("l{l:02}"));
    }

    let pics = b.dir(0, "Pictures".into());
    for a in 0..40 {
        let d = b.dir(pics, format!("Album{a:02}"));
        pool.push(d);
    }

    // Leftovers of uninstalled apps: empty directories, up to the total.
    let hoard = b.dir(0, ".leftovers".into());
    for e in 0..DIRS - b.dirs {
        b.dir(hoard, format!("e{e:04}"));
    }

    // The rest of the files, spread over the pool.
    let mut n = 0u32;
    while b.files < FILES {
        let d = pool[rng.below(pool.len() as u64) as usize];
        let s = rng.size(1_500, 256_000);
        b.file(d, format!("r{n:05}.dat"), s);
        n += 1;
    }
    debug_assert_eq!((b.files, b.dirs), (FILES, DIRS));
    b.b.build(root)
}

/// The totals a scan of `fs`'s root must report.
pub fn expected(fs: &SimFs) -> Expected {
    let mut e = Expected {
        files: 0,
        dirs: 0,
        file_bytes: 0,
    };
    for id in 1..fs.len() as NodeId {
        if fs.is_deleted(id) {
            continue;
        }
        let n = fs.node(id);
        match n.kind {
            Kind::Dir => e.dirs += 1,
            _ => {
                e.files += 1;
                e.file_bytes += n.apparent;
            }
        }
    }
    e
}

/// The totals of the subtree at `id` (itself excluded), as for [`expected`].
pub fn expected_below(fs: &SimFs, id: NodeId) -> Expected {
    let mut e = Expected {
        files: 0,
        dirs: 0,
        file_bytes: 0,
    };
    let mut stack: Vec<NodeId> = fs.children(id).collect();
    while let Some(x) = stack.pop() {
        let n = fs.node(x);
        if n.kind == Kind::Dir {
            e.dirs += 1;
            stack.extend(fs.children(x));
        } else {
            e.files += 1;
            e.file_bytes += n.apparent;
        }
    }
    e
}

/// The path of `id` relative to the fixture root (empty for the root).
pub fn rel_path(fs: &SimFs, id: NodeId) -> Vec<u8> {
    let full = fs.path_of(id);
    let root = fs.root_path();
    full.strip_prefix(root)
        .map(|r| r.strip_prefix(b"/").unwrap_or(r).to_vec())
        .unwrap_or(full)
}

/// The content of a fixture file: `size` pseudo-random bytes from the seed
/// and its path relative to the fixture root.
pub fn content(seed: u64, rel: &[u8], size: u64) -> Vec<u8> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
    for &b in rel {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut rng = Rng::new(h);
    let mut out = Vec::with_capacity(size as usize);
    while (out.len() as u64) < size {
        let word = rng.next_u64().to_le_bytes();
        let take = (size - out.len() as u64).min(8) as usize;
        out.extend_from_slice(&word[..take]);
    }
    out
}

// ── tar ─────────────────────────────────────────────────────────────────────

/// Every timestamp in the archive (the files' own mtimes are irrelevant to
/// the tests, and a fixed one keeps the archive reproducible).
const MTIME: u64 = 1_700_000_000;

/// Write `fs` as a ustar archive whose top folder is the root's basename,
/// parents before children. Toybox `tar -x` on the phone extracts it.
pub fn write_tar(fs: &SimFs, seed: u64, out: &mut impl Write) -> io::Result<u64> {
    let root = fs.root_path();
    let top = root.rsplit(|&b| b == b'/').next().unwrap_or(root).to_vec();
    let mut written = 0u64;
    let mut stack: Vec<NodeId> = vec![0];
    while let Some(id) = stack.pop() {
        let n = fs.node(id);
        let rel = rel_path(fs, id);
        let mut name = top.clone();
        if !rel.is_empty() {
            name.push(b'/');
            name.extend_from_slice(&rel);
        }
        if n.kind == Kind::Dir {
            name.push(b'/');
            out.write_all(&header(&name, 0o775, 0, b'5')?)?;
            written += 512;
            let mut kids: Vec<NodeId> = fs.children(id).collect();
            kids.reverse();
            stack.extend(kids);
        } else {
            out.write_all(&header(&name, 0o664, n.apparent, b'0')?)?;
            let body = content(seed, &rel, n.apparent);
            out.write_all(&body)?;
            let pad = (512 - body.len() % 512) % 512;
            out.write_all(&vec![0u8; pad])?;
            written += 512 + (body.len() + pad) as u64;
        }
    }
    out.write_all(&[0u8; 1024])?;
    Ok(written + 1024)
}

fn octal(field: &mut [u8], v: u64) {
    let digits = field.len() - 1;
    let s = format!("{v:0digits$o}");
    field[..digits].copy_from_slice(s.as_bytes());
    field[digits] = 0;
}

/// One ustar header block. Names longer than 100 bytes are split into the
/// prefix field at a `/`.
fn header(name: &[u8], mode: u32, size: u64, kind: u8) -> io::Result<[u8; 512]> {
    let mut h = [0u8; 512];
    let (prefix, base) = if name.len() <= 100 {
        (&name[..0], name)
    } else {
        let split = name[..name.len() - 1]
            .iter()
            .enumerate()
            .rev()
            .filter(|&(i, &b)| b == b'/' && i <= 155 && name.len() - i - 1 <= 100)
            .map(|(i, _)| i)
            .next()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("path too long for ustar: {}", String::from_utf8_lossy(name)),
                )
            })?;
        (&name[..split], &name[split + 1..])
    };
    h[..base.len()].copy_from_slice(base);
    octal(&mut h[100..108], u64::from(mode));
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], MTIME);
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[345..345 + prefix.len()].copy_from_slice(prefix);
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

/// Build the fixture for `seed`, write it to `tar`, push it to `serial` and
/// extract it under `parent` (replacing any earlier fixture there). Returns
/// the tree and the archive size.
pub fn push(
    transport: &dyn Transport,
    serial: &str,
    seed: u64,
    parent: &str,
    tar: &Path,
) -> Result<(SimFs, u64), String> {
    let root = format!("{parent}/{FIXTURE_DIR}");
    let fs = build(seed, root.as_bytes());
    let file = std::fs::File::create(tar).map_err(|e| format!("{}: {e}", tar.display()))?;
    let mut w = io::BufWriter::new(file);
    let size = write_tar(&fs, seed, &mut w)
        .and_then(|n| w.flush().map(|()| n))
        .map_err(|e| format!("writing {}: {e}", tar.display()))?;
    drop(w);
    transport
        .push(serial, tar, DEVICE_TAR, 0o644)
        .map_err(|e| format!("pushing the fixture: {e}"))?;
    run_detached(
        transport,
        serial,
        "extracting the fixture",
        &format!("rm -rf '{root}' && cd '{parent}' && tar -xf {DEVICE_TAR}"),
    )?;
    transport
        .shell(serial, &format!("rm -f {DEVICE_TAR}"))
        .map_err(|e| format!("removing the pushed archive: {e}"))?;
    Ok((fs, size))
}

/// Files a detached job leaves behind: its exit status, stdout and stderr.
const JOB_DONE: &str = "/data/local/tmp/ssbench.done";
const JOB_OUT: &str = "/data/local/tmp/ssbench.out";
const JOB_LOG: &str = "/data/local/tmp/ssbench.log";

/// How long a detached job may take. Tens of thousands of files through
/// `/sdcard`'s FUSE layer take minutes on an emulator.
const JOB_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// Run `script` on the phone and return its stdout.
///
/// A shell call that stays silent longer than the transport's read timeout is
/// treated as a lost device, and `tar` or `find -exec … +` over a large tree
/// can be silent for minutes. So the script runs detached, writing its output
/// and exit status to files, and is polled for. `script` must not contain
/// double quotes.
fn run_detached(
    transport: &dyn Transport,
    serial: &str,
    what: &str,
    script: &str,
) -> Result<String, String> {
    let launch = format!(
        "rm -f {JOB_DONE} {JOB_DONE}.tmp {JOB_OUT} {JOB_LOG}; \
         nohup sh -c \"{script}; echo \\$? > {JOB_DONE}.tmp; mv {JOB_DONE}.tmp {JOB_DONE}\" \
         </dev/null >{JOB_OUT} 2>{JOB_LOG} &"
    );
    let out = transport
        .shell(serial, &launch)
        .map_err(|e| format!("{what}: starting: {e}"))?;
    if !out.success() {
        return Err(format!(
            "{what}: could not start ({:?}): {}",
            out.exit,
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let deadline = Instant::now() + JOB_DEADLINE;
    let status = loop {
        std::thread::sleep(Duration::from_secs(1));
        let status = transport
            .shell(serial, &format!("cat {JOB_DONE} 2>/dev/null"))
            .map_err(|e| format!("{what}: waiting: {e}"))?
            .stdout_text();
        let status = status.trim().to_string();
        if !status.is_empty() {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{what}: took longer than {} minutes",
                JOB_DEADLINE.as_secs() / 60
            ));
        }
    };

    let read = |file: &str| {
        transport
            .shell(serial, &format!("cat {file} 2>/dev/null"))
            .map(|o| o.stdout_text())
            .map_err(|e| format!("{what}: reading its output: {e}"))
    };
    let stdout = read(JOB_OUT)?;
    let stderr = read(JOB_LOG)?;
    transport
        .shell(serial, &format!("rm -f {JOB_DONE} {JOB_OUT} {JOB_LOG}"))
        .ok();
    if status != "0" {
        return Err(format!("{what}: failed (exit {status}): {}", stderr.trim()));
    }
    Ok(stdout)
}

/// What is under `path` on the phone, by `find` and `stat`: (files, dirs,
/// apparent bytes of everything below, file bytes). Slow on big trees; for
/// checks.
pub fn measure_on_device(
    transport: &dyn Transport,
    serial: &str,
    path: &str,
) -> Result<(u32, u32, u64, u64), String> {
    let out = run_detached(
        transport,
        serial,
        &format!("measuring {path}"),
        &format!("find '{path}' -mindepth 1 -exec stat -c '%F|%s' {{}} +"),
    )?;
    let (mut files, mut dirs, mut bytes, mut file_bytes) = (0u32, 0u32, 0u64, 0u64);
    for line in out.lines() {
        let Some((kind, size)) = line.rsplit_once('|') else {
            continue;
        };
        let size: u64 = size
            .trim()
            .parse()
            .map_err(|_| format!("odd stat line {line:?}"))?;
        bytes += size;
        if kind == "directory" {
            dirs += 1;
        } else {
            files += 1;
            file_bytes += size;
        }
    }
    Ok((files, dirs, bytes, file_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_has_the_specified_shape() {
        let fs = build(1, b"/sdcard/ssfixture");
        let e = expected(&fs);
        assert_eq!((e.files, e.dirs), (FILES, DIRS));
        let thumbs = fs.lookup(b"/sdcard/ssfixture/Movies/.thumbcache").unwrap();
        assert_eq!(fs.children(thumbs).count() as u32, BIG_FILES);
        // Small enough to push in seconds.
        assert!(e.file_bytes < 200 << 20, "{} bytes", e.file_bytes);
        // The same seed gives the same tree; another seed another one.
        assert_eq!(expected(&build(1, b"/x")), e);
        assert_ne!(expected(&build(2, b"/x")).file_bytes, e.file_bytes);
    }

    /// Parse the archive back: every entry's header checksum, name, size and
    /// content match the tree.
    #[test]
    fn the_archive_round_trips() {
        let fs = build(3, b"/sdcard/ssfixture");
        let mut tar = Vec::new();
        let n = write_tar(&fs, 3, &mut tar).unwrap();
        assert_eq!(n as usize, tar.len());
        assert_eq!(tar.len() % 512, 0);
        let mut at = 0;
        let (mut files, mut dirs) = (0u32, 0u32);
        let field = |h: &[u8], r: std::ops::Range<usize>| {
            let s = std::str::from_utf8(&h[r]).unwrap();
            u64::from_str_radix(s.trim_end_matches(['\0', ' ']), 8).unwrap()
        };
        loop {
            let h = &tar[at..at + 512];
            if h.iter().all(|&b| b == 0) {
                break;
            }
            let mut blank = h.to_vec();
            blank[148..156].copy_from_slice(b"        ");
            let sum: u64 = blank.iter().map(|&b| u64::from(b)).sum();
            assert_eq!(field(h, 148..154), sum, "checksum");
            let cstr = |r: std::ops::Range<usize>| {
                let f = &h[r];
                f[..f.iter().position(|&b| b == 0).unwrap_or(f.len())].to_vec()
            };
            let (prefix, base) = (cstr(345..500), cstr(0..100));
            let mut name = prefix.clone();
            if !prefix.is_empty() {
                name.push(b'/');
            }
            name.extend_from_slice(&base);
            let name = name.strip_suffix(b"/").unwrap_or(&name).to_vec();
            let rel = name
                .strip_prefix(b"ssfixture")
                .map(|r| r.strip_prefix(b"/").unwrap_or(r))
                .unwrap()
                .to_vec();
            let mut full = b"/sdcard/ssfixture".to_vec();
            if !rel.is_empty() {
                full.push(b'/');
                full.extend_from_slice(&rel);
            }
            let id = fs
                .find(&full)
                .unwrap_or_else(|| panic!("unknown entry {}", String::from_utf8_lossy(&full)));
            let size = field(h, 124..136) as usize;
            at += 512;
            match h[156] {
                b'5' => {
                    assert_eq!(fs.node(id).kind, Kind::Dir);
                    dirs += 1;
                }
                b'0' => {
                    assert_eq!(size as u64, fs.node(id).apparent);
                    if files % 997 == 0 {
                        assert_eq!(&tar[at..at + size], &content(3, &rel, size as u64)[..]);
                    }
                    files += 1;
                    at += size.div_ceil(512) * 512;
                }
                k => panic!("type {k}"),
            }
        }
        assert_eq!((files, dirs), (FILES, DIRS + 1));
    }

    #[test]
    fn content_is_deterministic_per_path() {
        assert_eq!(content(1, b"a/b", 13), content(1, b"a/b", 13));
        assert_ne!(content(1, b"a/b", 13), content(1, b"a/c", 13));
        assert_ne!(content(1, b"a/b", 13), content(2, b"a/b", 13));
        assert_eq!(content(1, b"a/b", 13).len(), 13);
        assert!(content(1, b"a", 0).is_empty());
    }
}

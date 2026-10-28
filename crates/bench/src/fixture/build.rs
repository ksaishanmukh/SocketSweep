//! Building the fixture tree and the totals it must scan to.

use socketsweep_sim::{NodeId, Rng, SimFs, TreeBuilder};
use socketsweep_wire::Kind;

use super::{Expected, BIG_FILES, CHAIN, DIRS, FILES};

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

    android(&mut b, &mut pool);
    movies(&mut b, &mut rng);
    dcim(&mut b, &mut rng);
    music(&mut b, &mut rng);
    downloads(&mut b, &mut rng);
    documents(&mut b, &mut pool);
    pictures(&mut b, &mut pool);
    leftovers(&mut b);
    spread_remaining_files(&mut b, &mut rng, &pool);
    debug_assert_eq!((b.files, b.dirs), (FILES, DIRS));
    b.b.build(root)
}

fn android(b: &mut Builder, pool: &mut Vec<NodeId>) {
    let android = b.dir(0, "Android".into());
    let data = b.dir(android, "data".into());
    for p in 0..200 {
        let pkg = b.dir(data, format!("com.example.app{p:03}"));
        let cache = b.dir(pkg, "cache".into());
        let files = b.dir(pkg, "files".into());
        pool.extend([cache, files]);
    }
}
fn movies(b: &mut Builder, rng: &mut Rng) {
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
}
fn dcim(b: &mut Builder, rng: &mut Rng) {
    let dcim = b.dir(0, "DCIM".into());
    let cam = b.dir(dcim, "Camera".into());
    for f in 0..4_000 {
        let s = rng.range(1_000, 6_000);
        b.file(cam, format!("IMG_{f:05}.jpg"), s);
    }
}
fn music(b: &mut Builder, rng: &mut Rng) {
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
}
fn downloads(b: &mut Builder, rng: &mut Rng) {
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
}
fn documents(b: &mut Builder, pool: &mut Vec<NodeId>) {
    let docs = b.dir(0, "Documents".into());
    pool.push(docs);
    let mut deep = b.dir(docs, "deep".into());
    for l in 0..CHAIN {
        b.file(deep, format!("level{l}.txt"), 100 + u64::from(l));
        deep = b.dir(deep, format!("l{l:02}"));
    }
}
fn pictures(b: &mut Builder, pool: &mut Vec<NodeId>) {
    let pics = b.dir(0, "Pictures".into());
    for a in 0..40 {
        let d = b.dir(pics, format!("Album{a:02}"));
        pool.push(d);
    }
}
fn leftovers(b: &mut Builder) {
    // Leftovers of uninstalled apps: empty directories, up to the total.
    let hoard = b.dir(0, ".leftovers".into());
    for e in 0..DIRS - b.dirs {
        b.dir(hoard, format!("e{e:04}"));
    }
}
fn spread_remaining_files(b: &mut Builder, rng: &mut Rng, pool: &[NodeId]) {
    // The rest of the files, spread over the pool.
    let mut n = 0u32;
    while b.files < FILES {
        let d = pool[rng.below(pool.len() as u64) as usize];
        let s = rng.size(1_500, 256_000);
        b.file(d, format!("r{n:05}.dat"), s);
        n += 1;
    }
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

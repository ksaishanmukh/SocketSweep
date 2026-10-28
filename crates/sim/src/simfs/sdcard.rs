//! The standard `/sdcard`-shaped tree.

use super::random::random_subtree;
use super::{NodeId, SimFs, TreeBuilder};
use crate::rng::Rng;

impl SimFs {
    /// The standard `/sdcard`-shaped tree: 300 packages
    /// under `Android/data`, a 5,000-file camera folder, one 60,000-file
    /// folder (`Movies/.thumbcache`), a 40-level chain under
    /// `Documents/deep`, 3,000 empty directories (`.leftovers`), and the
    /// usual media folders (WhatsApp, Telegram, Pictures, Music,
    /// Download, Documents). By disk usage `Android` is the largest top-level folder
    /// (game data) and `Android/data` its largest child.
    pub fn standard(seed: u64) -> SimFs {
        Self::sdcard(seed, true)
    }

    /// [`SimFs::standard`], optionally without the 60,000-file folder (so the
    /// total is not one directory's stat time).
    pub fn sdcard(seed: u64, huge_folder: bool) -> SimFs {
        let mut rng = Rng::new(seed);
        let mut b = TreeBuilder::new();
        let root = 0;
        android(&mut b, &mut rng, root);
        leftovers(&mut b, root);
        whatsapp(&mut b, &mut rng, root);
        dcim(&mut b, &mut rng, root);
        pictures(&mut b, &mut rng, root);
        telegram(&mut b, &mut rng, root);
        movies(&mut b, &mut rng, root, huge_folder);
        downloads(&mut b, &mut rng, root);
        documents(&mut b, &mut rng, root);
        music(&mut b, &mut rng, root);
        b.build(b"/sdcard")
    }
}

fn android(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let android = b.dir(root, "Android");
    let data = b.dir(android, "data");
    for p in 0..300 {
        let pkg = b.dir(data, format!("com.example.app{p:03}"));
        let cache = b.dir(pkg, "cache");
        for f in 0..rng.below(30) {
            let s = rng.size(30_000, 5_000_000);
            b.file(cache, format!("c{f}"), s);
        }
        if rng.chance(0.2) {
            let sub = b.dir(cache, "image_manager_disk_cache");
            for f in 0..rng.below(100) {
                let s = rng.size(50_000, 2_000_000);
                b.file(sub, format!("{f:x}.0"), s);
            }
        }
        let files = b.dir(pkg, "files");
        random_subtree(b, rng, files, 2, 16);
        if p % 15 == 0 {
            // Games keep their assets here.
            b.file(files, "assets.pak", rng.range(300_000_000, 1_500_000_000));
        }
        if rng.chance(0.5) {
            b.dir(pkg, "no_backup");
        }
    }
    let obb = b.dir(android, "obb");
    for g in 0..10 {
        let d = b.dir(obb, format!("com.example.game{g:02}"));
        let s = rng.range(200_000_000, 1_000_000_000);
        b.file(d, format!("main.{g}.obb"), s);
    }
    let media = b.dir(android, "media");
    for p in 0..10 {
        let d = b.dir(media, format!("com.example.media{p}"));
        for f in 0..rng.below(30) {
            b.file(d, format!("m{f}"), rng.size(500_000, 20_000_000));
        }
    }
}

fn leftovers(b: &mut TreeBuilder, root: NodeId) {
    // Leftovers of uninstalled apps: thousands of empty directories.
    let hoard = b.dir(root, ".leftovers");
    for e in 0..3000 {
        b.dir(hoard, format!("e{e:04}"));
    }
}

fn whatsapp(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let wa = b.dir(root, "WhatsApp");
    let wam = b.dir(wa, "Media");
    for (name, n, typ) in [
        ("WhatsApp Images", 5_000, 200_000),
        ("WhatsApp Video", 600, 15_000_000),
        ("WhatsApp Documents", 1_500, 400_000),
        ("WhatsApp Audio", 3_000, 1_000_000),
        (".Statuses", 60, 1_000_000),
    ] {
        let d = b.dir(wam, name);
        for f in 0..n {
            let s = rng.size(typ, 500_000_000);
            b.file(d, format!("WA-{f:05}"), s);
        }
        let sent = b.dir(d, "Sent");
        for f in 0..n / 5 {
            let s = rng.size(typ, 500_000_000);
            b.file(sent, format!("WA-{f:05}"), s);
        }
    }
    let notes = b.dir(wam, "WhatsApp Voice Notes");
    for w in 0..200 {
        let d = b.dir(notes, format!("2024{w:03}"));
        for f in 0..rng.below(20) {
            b.file(d, format!("PTT-{f:03}.opus"), rng.size(30_000, 2_000_000));
        }
    }
    let backups = b.dir(wa, "Backups");
    b.file(backups, "chatsettingsbackup.db.crypt14", 2_000);
    let dbs = b.dir(wa, "Databases");
    for f in 0..10 {
        b.file(
            dbs,
            format!("msgstore-{f}.db.crypt14"),
            rng.range(50_000_000, 150_000_000),
        );
    }
}

fn dcim(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let dcim = b.dir(root, "DCIM");
    let cam = b.dir(dcim, "Camera");
    for f in 0..5000 {
        let s = rng.range(1_500_000, 5_000_000);
        b.file(cam, format!("IMG_{f:05}.jpg"), s);
    }
    let th = b.dir(dcim, ".thumbnails");
    for f in 0..800 {
        b.file(th, format!("{f}.jpg"), rng.range(5_000, 30_000));
    }
}

fn pictures(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let pics = b.dir(root, "Pictures");
    let shots = b.dir(pics, "Screenshots");
    for f in 0..2500 {
        b.file(
            shots,
            format!("Screenshot_{f:04}.png"),
            rng.range(100_000, 900_000),
        );
    }
    for (name, n) in [("Instagram", 2000), ("Facebook", 1500), ("Twitter", 800)] {
        let d = b.dir(pics, name);
        for f in 0..n {
            b.file(d, format!("{name}_{f:05}.jpg"), rng.range(50_000, 600_000));
        }
    }
    for a in 0..40 {
        let d = b.dir(pics, format!("Album{a:02}"));
        for f in 0..rng.below(200) {
            b.file(d, format!("p{f}.jpg"), rng.range(200_000, 4_000_000));
        }
    }
}

fn telegram(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let tg = b.dir(root, "Telegram");
    for (name, n, typ) in [
        ("Telegram Images", 2500, 150_000),
        ("Telegram Video", 600, 10_000_000),
        ("Telegram Documents", 1500, 2_000_000),
        ("Telegram Audio", 1200, 1_500_000),
    ] {
        let d = b.dir(tg, name);
        for f in 0..n {
            b.file(d, format!("{f:05}"), rng.size(typ, 300_000_000));
        }
    }
}

fn movies(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId, huge_folder: bool) {
    let movies = b.dir(root, "Movies");
    for f in 0..10 {
        b.file(
            movies,
            format!("movie{f}.mp4"),
            rng.range(100_000_000, 1_500_000_000),
        );
    }
    if huge_folder {
        let thumbs = b.dir(movies, ".thumbcache");
        for f in 0..60_000 {
            b.file(thumbs, format!("t{f:05}"), rng.range(2_000, 20_000));
        }
    }
}

fn downloads(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let dl = b.dir(root, "Download");
    for f in 0..2000 {
        let s = rng.size(1_000_000, 300_000_000);
        b.file(dl, format!("download{f:04}.bin"), s);
    }
    for d in 0..40 {
        let x = b.dir(dl, format!("folder{d}"));
        for f in 0..40 {
            b.file(x, format!("f{f}"), rng.size(100_000, 10_000_000));
        }
    }
}

fn documents(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let docs = b.dir(root, "Documents");
    for f in 0..500 {
        b.file(
            docs,
            format!("doc{f:03}.pdf"),
            rng.size(300_000, 30_000_000),
        );
    }
    for p in 0..30 {
        let d = b.dir(docs, format!("Project{p:02}"));
        for f in 0..rng.below(120) {
            b.file(d, format!("file{f:03}"), rng.size(100_000, 10_000_000));
        }
    }
    let mut deep = b.dir(docs, "deep");
    for l in 0..40 {
        b.file(deep, format!("level{l}.txt"), 100 + l);
        deep = b.dir(deep, format!("l{l}"));
    }
}

fn music(b: &mut TreeBuilder, rng: &mut Rng, root: NodeId) {
    let music = b.dir(root, "Music");
    for a in 0..60 {
        let artist = b.dir(music, format!("Artist{a:02}"));
        for al in 0..3 {
            let album = b.dir(artist, format!("Album{al}"));
            for t in 0..12 {
                b.file(
                    album,
                    format!("{t:02}.mp3"),
                    rng.range(3_000_000, 9_000_000),
                );
            }
        }
    }
    for name in [
        "Alarms",
        "Notifications",
        "Ringtones",
        "Podcasts",
        "Audiobooks",
        "Recordings",
    ] {
        b.dir(root, name);
    }
}

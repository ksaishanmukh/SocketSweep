//! Bytes per category kept in step through listings, deferred stats, deletes
//! and prunes; unreadable folders; app folders and leftovers.

use std::collections::HashMap;

use super::super::*;
use super::support::*;
use crate::apps::AppCatalog;
use socketsweep_gui_contract::{AppsStatus, Category};
use socketsweep_wire::{FsOp, ScanEvent};

fn bytes(t: &Tree, c: Category) -> u64 {
    t.categories(SizeMetric::Apparent)
        .iter()
        .find(|u| u.category == c)
        .map_or(0, |u| u.size.apparent)
}

#[test]
fn categories_add_up_through_listings_and_deferred_stats() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), file("x.zip", 70)]),
            listing(1, vec![bare("song.mp3"), file("pic.png", 30)]),
        ],
    );
    assert_eq!(bytes(&t, Category::Archive), 70);
    assert_eq!(bytes(&t, Category::Photo), 30);
    assert_eq!(bytes(&t, Category::Audio), 0, "its stat is still owed");
    let a = child(&t, ROOT, "A");
    assert_eq!(t.category(a, SizeMetric::Apparent), Category::Photo);

    apply(
        &mut t,
        &[ScanEvent::Stats {
            dir: 1,
            start: 0,
            stats: vec![Some(stat(100))],
        }],
    );
    assert_eq!(bytes(&t, Category::Audio), 100);
    assert_eq!(t.category(a, SizeMetric::Apparent), Category::Audio);
    // Largest first.
    let order: Vec<Category> = t
        .categories(SizeMetric::Apparent)
        .iter()
        .map(|u| u.category)
        .collect();
    assert_eq!(order, [Category::Audio, Category::Archive, Category::Photo]);

    // The daemon's final figure changes the totals, not the categories.
    apply(&mut t, &[done(1, 130, 2, 0)]);
    assert_eq!(bytes(&t, Category::Audio), 100);
}

#[test]
fn deletes_and_prunes_take_their_bytes_out_of_every_ancestor() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), dir("P", 3)]),
            listing(1, vec![dir("B", 2), file("big.mp4", 500)]),
            listing(2, vec![file("tune.mp3", 300), file("doc.pdf", 50)]),
            listing(3, vec![file("pic.jpg", 40)]),
        ],
    );
    let a = child(&t, ROOT, "A");
    let b = child(&t, a, "B");
    assert_eq!(bytes(&t, Category::Audio), 300);

    // A file: its own category only.
    t.remove(child(&t, a, "big.mp4")).unwrap();
    assert_eq!(bytes(&t, Category::Video), 0);
    assert_eq!(t.category(a, SizeMetric::Apparent), Category::Audio);

    // A folder: everything below it.
    t.remove(b).unwrap();
    assert_eq!(bytes(&t, Category::Audio), 0);
    assert_eq!(bytes(&t, Category::Doc), 0);
    assert_eq!(t.category(a, SizeMetric::Apparent), Category::Other);

    apply(&mut t, &[ScanEvent::Pruned { dir: 3 }]);
    assert_eq!(bytes(&t, Category::Photo), 0);
    assert!(t.categories(SizeMetric::Apparent).is_empty());
}

#[test]
fn unreadable_folders_are_remembered_until_deleted() {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("A", 1), dir("B", 2)]),
            ScanEvent::DirFailed {
                dir: 2,
                op: FsOp::Open,
                errno: 13,
            },
            ScanEvent::DirFailed {
                dir: 1,
                op: FsOp::Open,
                errno: 13,
            },
        ],
    );
    let paths: Vec<String> = t.unreadable_folders().into_iter().map(|u| u.path).collect();
    assert_eq!(paths, ["/sdcard/B", "/sdcard/A"]);
    t.remove(child(&t, ROOT, "B")).unwrap();
    let left = t.unreadable_folders();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].node, t.node_ref(child(&t, ROOT, "A")));
}

/// `Android/{data,media}` with three package folders and a stray file.
fn android() -> Tree {
    let mut t = tree();
    apply(
        &mut t,
        &[
            listing(0, vec![dir("Android", 1), dir("com.not.app", 7)]),
            listing(1, vec![dir("data", 2), dir("media", 3)]),
            listing(
                2,
                vec![dir("com.kept", 4), dir("com.gone", 5), file(".nomedia", 0)],
            ),
            listing(3, vec![dir("com.gone", 6)]),
            listing(4, vec![file("k", 10)]),
            listing(5, vec![file("g", 100)]),
            listing(6, vec![file("m", 1000)]),
        ],
    );
    t
}

#[test]
fn app_folders_are_tagged_and_named_once_the_phone_answers() {
    let mut t = android();
    let data = child(&t, child(&t, ROOT, "Android"), "data");
    let kept = child(&t, data, "com.kept");
    let row = |t: &Tree, id| {
        t.view(data, 10, SizeMetric::Disk)
            .unwrap()
            .rows
            .into_iter()
            .find(|r| r.node.id == id)
            .unwrap()
    };

    let tag = row(&t, kept).app.unwrap();
    assert_eq!(tag.package, "com.kept");
    assert!(tag.installed && tag.label.is_none() && !tag.has_icon);
    // Only directly under the app areas, and only folders.
    let elsewhere = t.view(ROOT, 10, SizeMetric::Disk).unwrap();
    assert!(elsewhere.rows.iter().all(|r| r.app.is_none()));
    assert!(
        t.leftovers(SizeMetric::Disk).apps.is_empty(),
        "not known yet"
    );

    let mut catalog = AppCatalog::ready(HashMap::from([("com.kept".into(), "Kept".into())]));
    catalog.add_icons(&["com.kept".to_string()]);
    t.set_apps(catalog);
    let tag = row(&t, kept).app.unwrap();
    assert_eq!(tag.label.as_deref(), Some("Kept"));
    assert!(tag.installed && tag.has_icon);
    assert_eq!(t.apps().status(), AppsStatus::Ready);

    let apps = t.app_breakdown(10, SizeMetric::Apparent);
    assert_eq!(apps[0].package, "com.gone");
    assert!(!apps[0].installed);
    assert_eq!(apps[1].label.as_deref(), Some("Kept"));
    assert_eq!(
        t.packages_by_size(SizeMetric::Apparent),
        ["com.gone", "com.kept"]
    );
}

#[test]
fn leftovers_group_an_uninstalled_packages_folders() {
    let mut t = android();
    t.set_apps(AppCatalog::ready(HashMap::from([(
        "com.kept".into(),
        "Kept".into(),
    )])));
    let left = t.leftovers(SizeMetric::Apparent);
    assert_eq!(left.apps.len(), 1);
    let gone = &left.apps[0];
    assert_eq!(gone.package, "com.gone");
    assert_eq!(gone.size.apparent, 1100);
    assert_eq!(left.total.apparent, 1100);
    // Largest folder first: the media one.
    let media = child(&t, child(&t, ROOT, "Android"), "media");
    assert_eq!(gone.folders[0], t.node_ref(child(&t, media, "com.gone")));
    assert_eq!(gone.folders.len(), 2);

    t.set_apps(AppCatalog::unavailable());
    assert!(t.leftovers(SizeMetric::Apparent).apps.is_empty());
}

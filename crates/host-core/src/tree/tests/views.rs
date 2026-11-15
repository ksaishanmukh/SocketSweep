//! Sorting and filtering folder views, change buckets, and search with a
//! filter, an order and a scope.

use super::super::*;
use super::support::*;
use socketsweep_gui_contract::{
    Category, ChangedBucket, KindFilter, SearchFilter, SortKey, ViewFilter, ViewSort,
};
use socketsweep_wire::Entry;

const DAY: i64 = 24 * 60 * 60;
/// The scan started at day 1000.
const NOW: i64 = 1000 * DAY;

fn at(mut e: Entry, days_ago: i64) -> Entry {
    if let Some(s) = e.stat.as_mut() {
        s.mtime = NOW - days_ago * DAY;
    }
    e
}

/// `/sdcard` = { Music/ { a.mp3 500, b.flac 300 }, clip.mp4 400, notes.txt
/// 10, photo.JPG 200, Docs/ (empty) }, with known change times.
fn sample() -> Tree {
    let mut t = tree();
    t.set_scan_time(NOW);
    apply(
        &mut t,
        &[
            listing(
                0,
                vec![
                    at(dir("Music", 1), 400),
                    at(file("clip.mp4", 400), 10),
                    at(file("notes.txt", 10), 100),
                    at(file("photo.JPG", 200), 300),
                    at(dir("Docs", 2), 900),
                ],
            ),
            listing(
                1,
                vec![at(file("a.mp3", 500), 5), at(file("b.flac", 300), 200)],
            ),
            listing(2, vec![]),
        ],
    );
    t
}

fn names(rows: &[socketsweep_gui_contract::Row]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

fn query(sort: ViewSort, filter: ViewFilter, limit: u32) -> ViewQuery {
    ViewQuery {
        limit,
        by: SizeMetric::Apparent,
        sort,
        filter,
    }
}

fn sorted(key: SortKey, descending: bool) -> ViewSort {
    ViewSort { key, descending }
}

#[test]
fn changes_fall_in_buckets_counted_from_the_scan_start() {
    assert_eq!(bucket_of(NOW, NOW + DAY), ChangedBucket::Last30d);
    assert_eq!(bucket_of(NOW, NOW - 30 * DAY), ChangedBucket::Last30d);
    assert_eq!(bucket_of(NOW, NOW - 31 * DAY), ChangedBucket::Months1To6);
    assert_eq!(bucket_of(NOW, NOW - 182 * DAY), ChangedBucket::Months1To6);
    assert_eq!(bucket_of(NOW, NOW - 183 * DAY), ChangedBucket::Months6To12);
    assert_eq!(bucket_of(NOW, NOW - 365 * DAY), ChangedBucket::Months6To12);
    assert_eq!(bucket_of(NOW, NOW - 366 * DAY), ChangedBucket::OverYear);
}

#[test]
fn rows_carry_category_change_time_and_folders_take_their_largest_category() {
    let t = sample();
    let view = t.view(ROOT, 10, SizeMetric::Apparent).unwrap();
    let row = |name: &str| view.rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!(row("photo.JPG").category, Category::Photo);
    assert_eq!(row("clip.mp4").category, Category::Video);
    assert_eq!(row("notes.txt").category, Category::Doc);
    assert_eq!(row("Music").category, Category::Audio);
    assert_eq!(row("Docs").category, Category::Other, "nothing inside");
    assert_eq!(row("clip.mp4").changed, Some(NOW - 10 * DAY));
    // A folder is as new as the newest thing inside it.
    assert_eq!(row("Music").changed, Some(NOW - 5 * DAY));
    assert_eq!(view.category, Category::Audio);
    assert_eq!(view.total_children, 5);
    assert_eq!(view.total_matching, 5);
}

#[test]
fn every_sort_key_orders_both_ways() {
    let t = sample();
    let rows = |sort| {
        t.view_query(ROOT, &query(sort, ViewFilter::default(), 10))
            .unwrap()
            .rows
    };
    assert_eq!(
        names(&rows(sorted(SortKey::Size, true))),
        ["Music", "clip.mp4", "photo.JPG", "notes.txt", "Docs"]
    );
    assert_eq!(
        names(&rows(sorted(SortKey::Name, false))),
        ["clip.mp4", "Docs", "Music", "notes.txt", "photo.JPG"]
    );
    assert_eq!(
        names(&rows(sorted(SortKey::Name, true))),
        ["photo.JPG", "notes.txt", "Music", "Docs", "clip.mp4"]
    );
    // Folders by their file counts, then files, which count as fewer than
    // any folder; equal ones by name.
    assert_eq!(
        names(&rows(sorted(SortKey::Count, true))),
        ["Music", "Docs", "clip.mp4", "notes.txt", "photo.JPG"]
    );
    assert_eq!(
        names(&rows(sorted(SortKey::Changed, true))),
        ["Music", "clip.mp4", "notes.txt", "photo.JPG", "Docs"]
    );
    assert_eq!(
        names(&rows(sorted(SortKey::Changed, false)))[0],
        "Docs",
        "oldest first"
    );
}

#[test]
fn filters_match_any_option_in_a_group_and_every_group() {
    let t = sample();
    let view = |filter: ViewFilter, limit| {
        t.view_query(ROOT, &query(ViewSort::default(), filter, limit))
            .unwrap()
    };

    let media = view(
        ViewFilter {
            categories: vec![Category::Video, Category::Audio],
            ..ViewFilter::default()
        },
        10,
    );
    assert_eq!(names(&media.rows), ["Music", "clip.mp4"]);
    assert_eq!((media.total_matching, media.total_children), (2, 5));

    let media_files = view(
        ViewFilter {
            categories: vec![Category::Video, Category::Audio],
            kinds: vec![KindFilter::File],
            ..ViewFilter::default()
        },
        10,
    );
    assert_eq!(names(&media_files.rows), ["clip.mp4"]);

    let recent_or_old = view(
        ViewFilter {
            changed: vec![ChangedBucket::Last30d, ChangedBucket::OverYear],
            ..ViewFilter::default()
        },
        10,
    );
    assert_eq!(names(&recent_or_old.rows), ["Music", "clip.mp4", "Docs"]);

    let by_name = view(
        ViewFilter {
            query: Some("  O  ".into()),
            ..ViewFilter::default()
        },
        1,
    );
    assert_eq!(names(&by_name.rows), ["photo.JPG"]);
    assert_eq!(by_name.total_matching, 3, "photo, notes, Docs");
    assert_eq!(by_name.hidden, 2, "matches past the limit");
}

#[test]
fn search_takes_a_filter_an_order_and_a_scope() {
    let t = sample();
    let music = child(&t, ROOT, "Music");
    let audio = SearchFilter {
        categories: vec![Category::Audio],
        ..SearchFilter::default()
    };
    let everywhere = t.search_with(&SearchQuery {
        query: String::new(),
        limit: 10,
        by: SizeMetric::Apparent,
        filter: audio.clone(),
        sort: ViewSort {
            key: SortKey::Name,
            descending: false,
        },
        within: None,
    });
    assert_eq!(names(&everywhere), ["a.mp3", "b.flac", "Music"]);
    assert_eq!(everywhere[0].parent.as_deref(), Some("/sdcard/Music"));

    let inside = t.search_with(&SearchQuery {
        query: "a".into(),
        limit: 10,
        by: SizeMetric::Apparent,
        within: Some(music),
        ..SearchQuery::default()
    });
    assert_eq!(names(&inside), ["a.mp3", "b.flac"], "not Music itself");

    let nothing = t.search_with(&SearchQuery {
        limit: 10,
        ..SearchQuery::default()
    });
    assert!(nothing.is_empty(), "no query and no filter finds nothing");
}

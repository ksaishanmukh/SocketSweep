use super::*;

fn file(name: &str, size: u64) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        size,
        kind: EntryKind::File,
    }
}

fn dir(name: &str) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        size: 0,
        kind: EntryKind::Dir,
    }
}

/// The tree from the project's own README screenshot, shrunk:
///
/// /sdcard              (root)
///   Android/           29 GB in one file
///   DCIM/
///     Camera/          2 files, 3 GB
///   Download/          4 GB in one file
///   note.txt           10 B
fn populated() -> Arena {
    let mut a = Arena::new(b"/sdcard");
    a.apply_dir(
        b"/sdcard",
        &[
            dir("Android"),
            dir("DCIM"),
            dir("Download"),
            file("note.txt", 10),
        ],
    )
    .unwrap();
    a.apply_dir(b"/sdcard/Android", &[file("big.obb", 29_000_000_000)])
        .unwrap();
    a.apply_dir(b"/sdcard/DCIM", &[dir("Camera")]).unwrap();
    a.apply_dir(
        b"/sdcard/DCIM/Camera",
        &[file("a.jpg", 1_000_000_000), file("b.jpg", 2_000_000_000)],
    )
    .unwrap();
    a.apply_dir(b"/sdcard/Download", &[file("iso.img", 4_000_000_000)])
        .unwrap();
    a
}

fn id_of(a: &Arena, path: &[u8]) -> NodeId {
    *a.dir_index.get(path).expect("directory not indexed")
}

fn row_named<'r>(rows: &'r [Row], name: &str) -> &'r Row {
    rows.iter().find(|r| r.name == name).expect("row not found")
}

#[test]
fn sizes_aggregate_up_the_whole_ancestry() {
    let a = populated();
    let s = a.stats();
    assert_eq!(s.size, 29_000_000_000 + 3_000_000_000 + 4_000_000_000 + 10);
    assert_eq!(s.files, 5);
    assert_eq!(
        s.dirs, 5,
        "the root plus Android, DCIM, DCIM/Camera, Download"
    );
}

#[test]
fn an_intermediate_directory_carries_its_whole_subtree() {
    let a = populated();
    let dcim = a.view(id_of(&a, b"/sdcard/DCIM"), 10).unwrap();
    assert_eq!(dcim.size, 3_000_000_000, "DCIM totals what is under Camera");
}

#[test]
fn totals_climb_as_frames_arrive_rather_than_appearing_at_the_end() {
    // The behaviour the progressive UI depends on.
    let mut a = Arena::new(b"/sdcard");
    a.apply_dir(b"/sdcard", &[dir("A")]).unwrap();
    assert_eq!(a.stats().size, 0);
    assert!(a.scanning(), "A has been announced but not listed");

    a.apply_dir(b"/sdcard/A", &[file("one", 100), dir("B")])
        .unwrap();
    assert_eq!(a.stats().size, 100);
    assert!(a.scanning(), "B is still outstanding");

    a.apply_dir(b"/sdcard/A/B", &[file("two", 500)]).unwrap();
    assert_eq!(a.stats().size, 600);

    // Still "scanning" until the daemon says otherwise. The arena cannot infer
    // completion: a directory at the walker depth limit is announced and then
    // never read, so pending counts do not reliably reach zero.
    assert!(a.scanning(), "completion is reported, not inferred");
    a.finish();
    assert!(!a.scanning());
}

#[test]
fn a_duplicate_frame_does_not_double_count() {
    let mut a = Arena::new(b"/sdcard");
    a.apply_dir(b"/sdcard", &[file("f", 100)]).unwrap();
    a.apply_dir(b"/sdcard", &[file("f", 100)]).unwrap();
    assert_eq!(a.stats().size, 100);
    assert_eq!(a.stats().files, 1);
}

#[test]
fn a_frame_no_parent_announced_is_rejected() {
    let mut a = Arena::new(b"/sdcard");
    let err = a.apply_dir(b"/sdcard/ghost", &[]).unwrap_err();
    assert!(matches!(err, ArenaError::OrphanFrame(_)), "got {err:?}");
}

#[test]
fn a_root_with_a_trailing_slash_still_joins_child_paths_correctly() {
    let mut a = Arena::new(b"/");
    a.apply_dir(b"/", &[dir("sdcard")]).unwrap();
    a.apply_dir(b"/sdcard", &[file("f", 1)]).unwrap();
    assert_eq!(a.stats().size, 1, "no doubled slash broke the path index");
}

#[test]
fn views_are_sorted_largest_first() {
    let a = populated();
    let v = a.view(ROOT, 10).unwrap();
    let names: Vec<&str> = v.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Android", "Download", "DCIM", "note.txt"]);
}

#[test]
fn equal_sizes_keep_a_stable_order_between_refreshes() {
    // Sizes change under a running scan; ties must not shuffle, or rows and
    // treemap tiles jitter for no reason.
    let mut a = Arena::new(b"/r");
    a.apply_dir(b"/r", &[file("b", 5), file("a", 5), file("c", 5)])
        .unwrap();

    let first: Vec<String> = a
        .view(ROOT, 10)
        .unwrap()
        .rows
        .iter()
        .map(|r| r.name.clone())
        .collect();
    for _ in 0..20 {
        let again: Vec<String> = a
            .view(ROOT, 10)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.name.clone())
            .collect();
        assert_eq!(again, first);
    }
    assert_eq!(first, ["a", "b", "c"], "ties fall back to name order");
}

#[test]
fn a_view_reports_how_many_rows_it_withheld() {
    let mut a = Arena::new(b"/r");
    let entries: Vec<Entry> = (0..500).map(|i| file(&format!("f{i:03}"), i)).collect();
    a.apply_dir(b"/r", &entries).unwrap();

    let v = a.view(ROOT, 100).unwrap();
    assert_eq!(v.rows.len(), 100);
    assert_eq!(v.hidden, 400);
}

#[test]
fn breadcrumbs_run_root_first() {
    let a = populated();
    let camera = id_of(&a, b"/sdcard/DCIM/Camera");
    let names: Vec<String> = a
        .breadcrumbs(camera)
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["sdcard", "DCIM", "Camera"]);
}

#[test]
fn paths_rebuild_exactly_from_ids() {
    let a = populated();
    let camera = id_of(&a, b"/sdcard/DCIM/Camera");
    assert_eq!(a.path_of(camera).unwrap(), b"/sdcard/DCIM/Camera".to_vec());

    let v = a.view(camera, 10).unwrap();
    let b_jpg = row_named(&v.rows, "b.jpg");
    assert_eq!(
        a.path_of(b_jpg.id).unwrap(),
        b"/sdcard/DCIM/Camera/b.jpg".to_vec()
    );
}

/// Deletes travel by id precisely so a name that is not valid UTF-8 can still be
/// displayed lossily without the byte path being corrupted on the way back.
#[test]
fn a_non_utf8_name_displays_lossily_but_its_path_stays_byte_exact() {
    let mut a = Arena::new(b"/sdcard");
    let raw = vec![b'v', b'i', b'd', 0xFF, 0xFE];
    a.apply_dir(
        b"/sdcard",
        &[Entry {
            name: raw.clone(),
            size: 42,
            kind: EntryKind::File,
        }],
    )
    .unwrap();

    let v = a.view(ROOT, 10).unwrap();
    assert_eq!(v.rows.len(), 1);
    assert!(v.rows[0].name.contains('\u{FFFD}'), "display name is lossy");

    let mut expected = b"/sdcard/".to_vec();
    expected.extend_from_slice(&raw);
    assert_eq!(a.path_of(v.rows[0].id).unwrap(), expected);
}

#[test]
fn removing_a_file_discounts_it_from_every_ancestor() {
    let mut a = populated();
    let camera = id_of(&a, b"/sdcard/DCIM/Camera");
    let b_jpg = row_named(&a.view(camera, 10).unwrap().rows, "b.jpg").id;

    let before = a.stats();
    a.remove(b_jpg).unwrap();
    let after = a.stats();

    assert_eq!(after.size, before.size - 2_000_000_000);
    assert_eq!(after.files, before.files - 1);
    assert_eq!(a.view(camera, 10).unwrap().size, 1_000_000_000);
}

#[test]
fn removing_a_directory_discounts_its_whole_subtree() {
    let mut a = populated();
    let dcim = id_of(&a, b"/sdcard/DCIM");

    let before = a.stats();
    a.remove(dcim).unwrap();
    let after = a.stats();

    assert_eq!(after.size, before.size - 3_000_000_000);
    assert_eq!(after.files, before.files - 2);
    assert_eq!(after.dirs, before.dirs - 2, "DCIM and DCIM/Camera both go");

    let names: Vec<String> = a
        .view(ROOT, 10)
        .unwrap()
        .rows
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert!(!names.contains(&"DCIM".to_string()));
}

#[test]
fn removing_the_first_last_and_middle_child_all_keep_the_sibling_list_intact() {
    for victim in ["a", "b", "c"] {
        let mut a = Arena::new(b"/r");
        a.apply_dir(b"/r", &[file("a", 1), file("b", 2), file("c", 3)])
            .unwrap();

        let target = row_named(&a.view(ROOT, 10).unwrap().rows, victim).id;
        a.remove(target).unwrap();

        let left: Vec<String> = a
            .view(ROOT, 10)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.name.clone())
            .collect();
        assert_eq!(
            left.len(),
            2,
            "removing {victim} lost or kept too many siblings"
        );
        assert!(!left.contains(&victim.to_string()));
    }
}

#[test]
fn a_node_can_still_be_appended_after_the_last_child_was_removed() {
    // Guards the last_child bookkeeping in unlink().
    let mut a = Arena::new(b"/r");
    a.apply_dir(b"/r", &[file("a", 1), dir("d")]).unwrap();
    let d = id_of(&a, b"/r/d");
    a.remove(d).unwrap();

    let rows = a.view(ROOT, 10).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "a");
}

#[test]
fn removing_twice_is_a_no_op_rather_than_double_counting() {
    let mut a = populated();
    let dcim = id_of(&a, b"/sdcard/DCIM");
    a.remove(dcim).unwrap();
    let after_first = a.stats();
    a.remove(dcim).unwrap();
    assert_eq!(a.stats().size, after_first.size);
}

#[test]
fn the_root_cannot_be_removed() {
    let mut a = populated();
    assert!(a.remove(ROOT).is_err());
}

#[test]
fn largest_files_crosses_directory_boundaries() {
    let a = populated();
    let top = a.largest_files(3);
    let names: Vec<&str> = top.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["big.obb", "iso.img", "b.jpg"]);
    assert!(top.iter().all(|r| !r.is_dir), "directories are not files");
}

#[test]
fn search_matches_case_insensitively_and_ranks_by_size() {
    let a = populated();
    let hits = a.search("JP", 10);
    let names: Vec<&str> = hits.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["b.jpg", "a.jpg"]);

    assert!(
        a.search("", 10).is_empty(),
        "an empty query matches nothing"
    );
    assert!(a.search("nothing-here", 10).is_empty());
}

#[test]
fn search_and_largest_files_skip_removed_nodes() {
    let mut a = populated();
    let android = id_of(&a, b"/sdcard/Android");
    a.remove(android).unwrap();

    assert!(!a.largest_files(10).iter().any(|r| r.name == "big.obb"));
    assert!(a.search("obb", 10).is_empty());
}

#[test]
fn a_wide_directory_builds_in_linear_time() {
    // The append path used to be O(n^2) if it walked the sibling list each time.
    let mut a = Arena::new(b"/r");
    let entries: Vec<Entry> = (0..50_000).map(|i| file(&format!("f{i:06}"), 1)).collect();

    let start = std::time::Instant::now();
    a.apply_dir(b"/r", &entries).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(a.stats().files, 50_000);
    assert_eq!(a.stats().size, 50_000);
    assert!(
        elapsed.as_secs() < 2,
        "building 50k children took {elapsed:?}, which suggests quadratic appends"
    );
}

#[test]
fn a_deep_chain_aggregates_all_the_way_to_the_root() {
    let mut a = Arena::new(b"/r");
    let mut path = b"/r".to_vec();
    a.apply_dir(b"/r", &[dir("d0")]).unwrap();

    for i in 0..60 {
        path.extend_from_slice(format!("/d{i}").as_bytes());
        let next = if i < 59 {
            vec![dir(&format!("d{}", i + 1)), file("leaf", 10)]
        } else {
            vec![file("leaf", 10)]
        };
        a.apply_dir(&path, &next).unwrap();
    }

    assert_eq!(
        a.stats().size,
        600,
        "every level contributed one 10-byte leaf"
    );
    assert_eq!(a.stats().files, 60);
}

#[test]
fn treemap_nests_to_the_requested_depth() {
    let a = populated();
    let t = a.treemap(ROOT, 2).unwrap();

    let dcim = t.children.iter().find(|c| c.name == "DCIM").unwrap();
    assert_eq!(dcim.children.len(), 1, "Camera is one level down");
    assert_eq!(dcim.children[0].name, "Camera");
    assert!(
        dcim.children[0].children.is_empty(),
        "depth 2 stops before Camera's files"
    );
}

#[test]
fn treemap_children_are_largest_first() {
    let a = populated();
    let t = a.treemap(ROOT, 1).unwrap();
    let names: Vec<&str> = t.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Android", "Download", "DCIM", "note.txt"]);
}

#[test]
fn treemap_omits_zero_sized_entries_and_caps_fan_out() {
    let mut a = Arena::new(b"/r");
    let mut entries: Vec<Entry> = (0..200).map(|i| file(&format!("f{i:03}"), i + 1)).collect();
    entries.push(file("empty", 0));
    a.apply_dir(b"/r", &entries).unwrap();

    let t = a.treemap(ROOT, 1).unwrap();
    assert!(
        t.children.len() <= 48,
        "fan-out is capped for the top level"
    );
    assert!(
        !t.children.iter().any(|c| c.name == "empty"),
        "a zero-byte entry has no tile to draw"
    );
}

#[test]
fn treemap_rejects_an_unknown_node() {
    let a = populated();
    assert!(a.treemap(9999, 1).is_err());
}

// ── File-type breakdown ─────────────────────────────────────────────────────

fn typed() -> Arena {
    let mut a = Arena::new(b"/sdcard");
    a.apply_dir(
        b"/sdcard",
        &[
            file("holiday.JPG", 3_000_000),
            file("clip.mp4", 900_000_000),
            file("song.flac", 40_000_000),
            file("game.obb", 2_000_000_000),
            file("notes.pdf", 500_000),
            file("backup.zip", 10_000_000),
            file("mystery", 7),
            file(".gitignore", 3),
            file("archive.tar.gz", 1_000_000),
            dir("sub"),
        ],
    )
    .unwrap();
    a.apply_dir(b"/sdcard/sub", &[file("second.mp4", 100_000_000)])
        .unwrap();
    a
}

fn group<'g>(groups: &'g [TypeGroup], label: &str) -> &'g TypeGroup {
    groups.iter().find(|g| g.label == label).expect(label)
}

#[test]
fn type_breakdown_classifies_by_extension_case_insensitively() {
    let groups = typed().type_breakdown();

    assert_eq!(group(&groups, "Photos").size, 3_000_000, ".JPG matches jpg");
    assert_eq!(group(&groups, "Audio").files, 1);
    assert_eq!(group(&groups, "Apps").size, 2_000_000_000);
    assert_eq!(group(&groups, "Documents").files, 1);
}

#[test]
fn type_breakdown_sums_a_category_across_directories() {
    let groups = typed().type_breakdown();
    let video = group(&groups, "Video");
    assert_eq!(video.files, 2, "both mp4s, in different folders");
    assert_eq!(video.size, 1_000_000_000);
}

#[test]
fn type_breakdown_is_ordered_largest_first() {
    let groups = typed().type_breakdown();
    let labels: Vec<&str> = groups.iter().map(|g| g.label).collect();
    assert_eq!(labels[0], "Apps");
    assert_eq!(labels[1], "Video");
}

#[test]
fn a_hidden_file_is_not_treated_as_having_an_extension() {
    // ".gitignore" is a dotfile, not a file of type "gitignore".
    let groups = typed().type_breakdown();
    let other = group(&groups, "Other");
    assert_eq!(other.files, 2, ".gitignore and the extensionless 'mystery'");
}

#[test]
fn a_double_extension_is_classified_by_the_last_one() {
    let groups = typed().type_breakdown();
    assert_eq!(
        group(&groups, "Archives").files,
        2,
        "backup.zip and archive.tar.gz"
    );
}

#[test]
fn type_breakdown_omits_empty_categories_and_skips_removed_files() {
    let mut a = typed();
    assert!(a.type_breakdown().iter().all(|g| g.files > 0));

    let obb = a
        .view(ROOT, 50)
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.name == "game.obb")
        .unwrap();
    a.remove(obb.id).unwrap();

    let groups = a.type_breakdown();
    assert!(
        !groups.iter().any(|g| g.label == "Apps"),
        "the only app was deleted, so the category should disappear"
    );
}

#[test]
fn type_breakdown_totals_match_the_scan_total() {
    let a = typed();
    let summed: u64 = a.type_breakdown().iter().map(|g| g.size).sum();
    assert_eq!(
        summed,
        a.stats().size,
        "every byte lands in exactly one category"
    );
}

// ── Parent paths on cross-tree rows ─────────────────────────────────────────

#[test]
fn largest_files_and_search_carry_the_containing_folder() {
    let a = populated();

    let top = a.largest_files(3);
    let b_jpg = top.iter().find(|r| r.name == "b.jpg").unwrap();
    assert_eq!(b_jpg.parent.as_deref(), Some("/sdcard/DCIM/Camera"));

    let hit = a.search("iso", 5).into_iter().next().unwrap();
    assert_eq!(hit.parent.as_deref(), Some("/sdcard/Download"));
}

#[test]
fn a_directory_listing_omits_the_parent_it_would_repeat_on_every_row() {
    let a = populated();
    let rows = a.view(ROOT, 10).unwrap().rows;
    assert!(rows.iter().all(|r| r.parent.is_none()));
}

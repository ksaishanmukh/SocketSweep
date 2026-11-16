//! The focus rule, driven by hand-made listings.

use socketsweep_wire::{DirId, Kind, ScanEvent, ScanMode, ScanOpts, Stat, ROOT_DIR};

use super::{Pick, ScanState, Task, TaskKind, TaskOutput};
use crate::fs::{DirRead, FsEntry, FsStat, RootInfo};

const A: DirId = 1;
const B: DirId = 2;

fn state(workers: u8) -> ScanState {
    let root = RootInfo {
        path: b"/r".to_vec(),
        dev: 1,
    };
    ScanState::new(root, &ScanOpts::default(), workers, 0)
}

/// `n` subdirectories. With `early` each is already stat'ed (as for
/// `DT_UNKNOWN`), so a `Names` listing owes no deferred stats.
fn subdirs(n: usize, early: bool) -> DirRead {
    let stat = FsStat {
        kind: Kind::Dir,
        stat: Stat {
            apparent: 4096,
            blocks: 8,
            mtime: 1,
        },
        dev: 1,
        ino: 0,
        nlink: 1,
    };
    let entries = (0..n)
        .map(|i| FsEntry {
            name: format!("d{i}").into_bytes(),
            kind: Kind::Dir,
            stat: early.then_some(Ok(stat)),
        })
        .collect();
    DirRead { dev: 1, entries }
}

fn run(st: &mut ScanState, w: usize) -> Task {
    match st.pick(w, 1) {
        Pick::Run(t) => t,
        other => panic!("worker {w} got {other:?}"),
    }
}

/// Worker 0 takes the next task, which must be `dir`, and lists it.
fn list(st: &mut ScanState, dir: DirId, read: DirRead) {
    let t = run(st, 0);
    assert_eq!(t.dir, dir);
    st.apply(0, TaskOutput::Listing(read), 1);
    st.validate().unwrap();
}

/// Root with folders A and B, listed with `a` and `b` subfolders.
fn two_folders(workers: u8, a: usize, b: usize) -> ScanState {
    let mut st = state(workers);
    list(&mut st, ROOT_DIR, subdirs(2, true));
    list(&mut st, A, subdirs(a, true));
    list(&mut st, B, subdirs(b, true));
    st
}

/// Every worker picks once; returns the tasks handed out, in order.
fn pick_all(st: &mut ScanState) -> Vec<Task> {
    let tasks = (0..st.workers.len())
        .filter_map(|w| match st.pick(w, 1) {
            Pick::Run(t) => Some(t),
            _ => None,
        })
        .collect();
    st.validate().unwrap();
    tasks
}

fn count_in(st: &ScanState, tasks: &[Task], d: DirId) -> usize {
    tasks.iter().filter(|t| st.within(t.dir, d)).count()
}

#[test]
fn background_floor() {
    let floor = |w| state(w).bg_floor();
    assert_eq!([1, 2, 4, 7, 8, 16].map(floor), [0, 1, 1, 1, 2, 4]);
}

#[test]
fn with_both_sides_ready_exactly_the_floor_goes_to_the_background() {
    let mut st = two_folders(8, 20, 20);
    assert!(st.skeleton_ready_at().is_some());
    assert!(st.set_focus(1, A, 1));
    let tasks = pick_all(&mut st);
    assert_eq!(tasks.len(), 8);
    assert_eq!(count_in(&st, &tasks, B), 2);
    assert_eq!(count_in(&st, &tasks, A), 6);
    // The floor is filled first, so the background never waits for a turn.
    assert!(tasks[..2].iter().all(|t| st.within(t.dir, B)));
    assert!(tasks.iter().all(|t| t.kind == TaskKind::Full));
}

#[test]
fn without_focused_work_every_worker_goes_to_the_background() {
    // Three focused tasks: the floor, then those three, then the rest.
    let mut st = two_folders(8, 3, 40);
    assert!(st.set_focus(1, A, 1));
    let tasks = pick_all(&mut st);
    assert_eq!(count_in(&st, &tasks, A), 3);
    assert_eq!(count_in(&st, &tasks, B), 5);

    // A folder already measured has nothing to steer to.
    let mut st = two_folders(8, 0, 40);
    assert!(st.set_focus(1, A, 1));
    let tasks = pick_all(&mut st);
    assert_eq!(count_in(&st, &tasks, B), 8);
}

#[test]
fn clearing_the_focus_restores_the_unfocused_walk() {
    let mut cleared = two_folders(8, 20, 20);
    assert!(cleared.set_focus(1, A, 1));
    assert!(cleared.clear_focus(2, 1));
    let mut never = two_folders(8, 20, 20);
    let a = pick_all(&mut cleared);
    let b = pick_all(&mut never);
    assert_eq!(a, b);
    // Spread over the root's folders, fewest workers first.
    assert_eq!(count_in(&never, &b, A), 4);
}

#[test]
fn an_unread_focus_is_read_at_once_with_its_stats_even_in_the_skeleton() {
    let mut st = state(8);
    list(&mut st, ROOT_DIR, subdirs(4, true));
    assert!(st.skeleton_ready_at().is_none());
    let f = 3;
    assert!(st.set_focus(1, f, 1));
    let tasks = pick_all(&mut st);
    let kinds: Vec<_> = tasks.iter().map(|t| (t.dir, t.kind)).collect();
    // Two skeleton reads for the floor, then the focus with `Full`, then the
    // remaining background folder: nobody idles.
    assert_eq!(
        kinds[..4],
        [
            (1, TaskKind::Names),
            (2, TaskKind::Names),
            (f, TaskKind::Full),
            (4, TaskKind::Names),
        ]
    );
}

#[test]
fn the_parents_deferred_stats_count_as_focused_work() {
    // The root is listed by `Names` and owes its stats; a child cannot
    // finish before them.
    let mut st = state(8);
    list(&mut st, ROOT_DIR, subdirs(4, false));
    assert!(st.set_focus(1, B, 1));
    let tasks = pick_all(&mut st);
    let kinds: Vec<_> = tasks.iter().map(|t| (t.dir, t.kind)).collect();
    assert_eq!(
        kinds[..4],
        [
            (A, TaskKind::Names),
            (3, TaskKind::Names),
            (ROOT_DIR, TaskKind::StatDir),
            (B, TaskKind::Full),
        ]
    );
}

#[test]
fn plain_mode_acknowledges_the_focus_but_is_not_steered() {
    let plain = || {
        let opts = ScanOpts {
            workers: None,
            mode: ScanMode::Plain,
        };
        let root = RootInfo {
            path: b"/r".to_vec(),
            dev: 1,
        };
        let mut st = ScanState::new(root, &opts, 8, 0);
        list(&mut st, ROOT_DIR, subdirs(2, true));
        list(&mut st, A, subdirs(20, true));
        st
    };
    let (mut focused, mut never) = (plain(), plain());
    assert!(focused.set_focus(1, B, 1));
    assert!(matches!(
        focused.take_events().last(),
        Some(ScanEvent::FocusState { dir: B, .. })
    ));
    assert_eq!(pick_all(&mut focused), pick_all(&mut never));
}

//! A jwalk-like baseline engine on the same model, for comparison.

use std::collections::{HashMap, VecDeque};

use socketsweep_engine::{DirRead, Fs, Interrupt, Micros, TaskKind, TaskOutput};
use socketsweep_wire::{Kind, ScanOpts};

use crate::latency::{Latency, Storage};
use crate::simfs::SimFs;

/// The result of a simulated baseline scan.
#[derive(Debug, Clone, Default)]
pub struct BaselineRun {
    pub finished_at: Micros,
    /// Completion time of each directory's subtree, per path.
    pub done_at: HashMap<Vec<u8>, Micros>,
    pub tasks: u64,
    pub work: f64,
}

struct BDir {
    path: Vec<u8>,
    parent: usize,
    depth: u16,
    open: u32,
    listed: bool,
}

/// A jwalk-like engine: W workers, each with its own deque; a worker reads a
/// directory with names and stats together, pushes the subdirectories onto
/// its own deque, and takes the newest from it (LIFO), or steals the oldest
/// from another worker when its own is empty. No focus, no deferred stats.
pub fn run_baseline(fs: &SimFs, workers: u8, lat: Latency, opts: &ScanOpts) -> BaselineRun {
    let w = usize::from(workers.max(1));
    let root = fs.root(fs.root_path()).expect("sim root");
    let mut dirs = vec![BDir {
        path: root.path.clone(),
        parent: usize::MAX,
        depth: 0,
        open: 0,
        listed: false,
    }];
    let mut deques: Vec<VecDeque<usize>> = vec![Default::default(); w];
    deques[0].push_back(0);
    let mut storage = Storage::new(lat, w);
    let mut inflight: Vec<Option<(usize, TaskOutput)>> = vec![None; w];
    let mut run = BaselineRun::default();
    let mut now: Micros = 0;
    let mut root_done = false;
    while !root_done {
        for wi in 0..w {
            if inflight[wi].is_some() {
                continue;
            }
            let next = deques[wi]
                .pop_back()
                .or_else(|| (1..w).find_map(|k| deques[(wi + k) % w].pop_front()));
            if let Some(d) = next {
                let out: TaskOutput = fs.list_full(&dirs[d].path, Interrupt::NEVER).into();
                let work = lat.cost(TaskKind::Full, &out, 0);
                storage.start(wi, work);
                run.tasks += 1;
                inflight[wi] = Some((d, out));
            }
        }
        let (wi, t) = storage.next_done(now).expect("baseline deadlock");
        storage.advance(t - now);
        now = t;
        storage.finish(wi);
        let (d, out) = inflight[wi].take().expect("in flight");
        dirs[d].listed = true;
        if let TaskOutput::Listing(read) = out {
            let other_fs = d != 0 && opts.one_filesystem && read.dev != root.dev;
            if !other_fs && dirs[d].depth < opts.max_depth {
                push_subdirs(&mut dirs, &mut deques[wi], d, read);
            }
        }
        root_done = complete_cascade(&mut dirs, &mut run, d, now);
    }
    run.finished_at = now;
    run.work = storage.work_done;
    run
}

/// Add the subdirectories of `d` to `dirs` and to the worker's deque.
fn push_subdirs(dirs: &mut Vec<BDir>, deque: &mut VecDeque<usize>, d: usize, read: DirRead) {
    for e in read.entries {
        if e.kind == Kind::Dir {
            let mut p = dirs[d].path.clone();
            p.push(b'/');
            p.extend_from_slice(&e.name);
            let id = dirs.len();
            dirs.push(BDir {
                path: p,
                parent: d,
                depth: dirs[d].depth + 1,
                open: 0,
                listed: false,
            });
            dirs[d].open += 1;
            deque.push_back(id);
        }
    }
}

/// Completion cascade from `d` upwards; true when the root is done.
fn complete_cascade(dirs: &mut [BDir], run: &mut BaselineRun, d: usize, now: Micros) -> bool {
    let mut x = d;
    while dirs[x].listed && dirs[x].open == 0 {
        run.done_at.insert(dirs[x].path.clone(), now);
        if x == 0 {
            return true;
        }
        let p = dirs[x].parent;
        dirs[p].open -= 1;
        x = p;
    }
    false
}

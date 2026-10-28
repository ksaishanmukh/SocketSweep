//! Random trees for property tests.

use socketsweep_engine::errno;
use socketsweep_wire::Kind;

use super::{NodeId, SimFs, TreeBuilder};
use crate::rng::Rng;

/// Shape of [`SimFs::random`] trees.
#[derive(Debug, Clone, Copy)]
pub struct RandomShape {
    pub max_dirs: u32,
    pub max_files_per_dir: u32,
    pub max_depth: u32,
    /// Inject unreadable directories, failing stats, `DT_UNKNOWN`, symlinks
    /// and a mount point.
    pub faults: bool,
    /// Add hardlinks (root totals only are then comparable).
    pub hardlinks: bool,
}

impl Default for RandomShape {
    fn default() -> Self {
        RandomShape {
            max_dirs: 200,
            max_files_per_dir: 20,
            max_depth: 8,
            faults: true,
            hardlinks: false,
        }
    }
}

impl SimFs {
    /// A random tree, for property tests.
    pub fn random(seed: u64, shape: RandomShape) -> SimFs {
        let mut rng = Rng::new(seed);
        let mut b = TreeBuilder::new();
        let mut frontier = vec![(0 as NodeId, 0u32)];
        let mut dirs = 1u32;
        let mut files_made: Vec<NodeId> = Vec::new();
        let mut i = 0;
        while i < frontier.len() {
            let (d, depth) = frontier[i];
            i += 1;
            random_files(&mut b, &mut rng, shape, d, &mut files_made);
            if depth < shape.max_depth {
                random_child_dirs(
                    &mut b,
                    &mut rng,
                    shape,
                    (d, depth),
                    &mut dirs,
                    &mut frontier,
                );
            }
        }
        if shape.faults && frontier.len() > 3 {
            let (d, _) = frontier[1 + rng.below(frontier.len() as u64 - 1) as usize];
            if b.node(d).fail_open.is_none() {
                b.mount(d, 2);
            }
        }
        b.build(b"/sdcard")
    }
}

/// Files (and sometimes one hardlink) of directory `d`.
fn random_files(
    b: &mut TreeBuilder,
    rng: &mut Rng,
    shape: RandomShape,
    d: NodeId,
    files_made: &mut Vec<NodeId>,
) {
    let nf = rng.below(u64::from(shape.max_files_per_dir) + 1);
    for k in 0..nf {
        let f = if shape.faults && rng.chance(0.03) {
            b.symlink(d, format!("l{k}"), rng.range(5, 60))
        } else {
            let size = rng.size(20_000, 50_000_000);
            b.file(d, format!("f{k}"), size)
        };
        if shape.faults {
            if rng.chance(0.03) {
                b.node_mut(f).unknown_dtype = true;
            }
            if rng.chance(0.02) {
                b.node_mut(f).fail_stat = Some(errno::EACCES);
            }
        }
        if b.node(f).kind == Kind::File && b.node(f).fail_stat.is_none() {
            files_made.push(f);
        }
    }
    if shape.hardlinks && !files_made.is_empty() && rng.chance(0.2) {
        let of = files_made[rng.below(files_made.len() as u64) as usize];
        b.hardlink(d, "hl", of);
    }
}

/// Subdirectories of `parent` (a directory and its depth), added to the
/// frontier.
fn random_child_dirs(
    b: &mut TreeBuilder,
    rng: &mut Rng,
    shape: RandomShape,
    parent: (NodeId, u32),
    dirs: &mut u32,
    frontier: &mut Vec<(NodeId, u32)>,
) {
    let (d, depth) = parent;
    let fan = if depth == 0 {
        rng.range(2, 8)
    } else {
        rng.below(5)
    };
    for k in 0..fan {
        if *dirs >= shape.max_dirs {
            break;
        }
        *dirs += 1;
        let c = b.dir(d, format!("d{k}"));
        if shape.faults {
            if rng.chance(0.03) {
                b.node_mut(c).fail_open = Some(errno::EACCES);
            } else if rng.chance(0.03) {
                b.node_mut(c).unknown_dtype = true;
            } else if rng.chance(0.02) {
                b.node_mut(c).fail_stat = Some(errno::EACCES);
            }
        }
        frontier.push((c, depth + 1));
    }
}

pub(super) fn random_subtree(
    b: &mut TreeBuilder,
    rng: &mut Rng,
    d: NodeId,
    depth: u32,
    files: u64,
) {
    for f in 0..rng.below(files + 1) {
        let s = rng.size(40_000, 20_000_000);
        b.file(d, format!("f{f}"), s);
    }
    if depth == 0 {
        return;
    }
    for k in 0..rng.below(3) {
        let c = b.dir(d, format!("d{k}"));
        random_subtree(b, rng, c, depth - 1, files / 2);
    }
}

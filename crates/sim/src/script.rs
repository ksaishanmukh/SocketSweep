//! Navigation scripts and random scripts for property tests.

use socketsweep_engine::Micros;
use socketsweep_wire::Kind;

use crate::des::{Action, Script};
use crate::rng::Rng;
use crate::simfs::{NodeId, SimFs};

/// The live child directory of `d` using the most disk ("largest", as the
/// GUI sorts by default), ties to the first by name.
pub fn largest_child(fs: &SimFs, d: NodeId) -> Option<NodeId> {
    let mut best: Option<(u64, NodeId)> = None;
    for c in fs.children(d) {
        if fs.node(c).kind != Kind::Dir {
            continue;
        }
        let n = fs.subtree_disk(c);
        if best.is_none_or(|(bn, _)| n > bn) {
            best = Some((n, c));
        }
    }
    best.map(|b| b.1)
}

/// (a) No navigation.
pub fn script_a() -> Script {
    Script::default()
}

/// (b) Open the largest top-level folder at 300 ms, then drill into its
/// largest child at 2 s. Returns the script and the final folder's path.
pub fn script_b(fs: &SimFs) -> (Script, Vec<u8>) {
    let top = largest_child(fs, 0).expect("a top-level folder");
    let child = largest_child(fs, top).unwrap_or(top);
    let target = fs.path_of(child);
    let script = Script {
        steps: vec![
            (300_000, Action::Focus(fs.path_of(top))),
            (2_000_000, Action::Focus(target.clone())),
        ],
    };
    (script, target)
}

/// (c) Ten clicks within one second, among the top two levels: a random
/// top-level folder, then half the time one of its subfolders.
pub fn script_c(fs: &SimFs, seed: u64) -> Script {
    let mut rng = Rng::new(seed);
    let dirs = |d: NodeId| -> Vec<NodeId> {
        fs.children(d)
            .filter(|&c| fs.node(c).kind == Kind::Dir)
            .collect()
    };
    let top = dirs(0);
    let steps = (0..10)
        .map(|i| {
            let mut d = top[rng.below(top.len() as u64) as usize];
            let kids = dirs(d);
            if !kids.is_empty() && rng.chance(0.5) {
                d = kids[rng.below(kids.len() as u64) as usize];
            }
            (100_000 + i * 100_000, Action::Focus(fs.path_of(d)))
        })
        .collect();
    Script { steps }
}

/// A random script over `horizon`: focus changes (with a 1-in-6 chance of a
/// clear), and with `deletes`, some directory deletes.
pub fn random_script(fs: &SimFs, seed: u64, n: usize, horizon: Micros, deletes: bool) -> Script {
    let mut rng = Rng::new(seed);
    let dirs: Vec<NodeId> = (0..fs.len() as NodeId)
        .filter(|&i| fs.node(i).kind == Kind::Dir)
        .collect();
    let mut steps: Vec<(Micros, Action)> = (0..n)
        .map(|_| {
            let t = rng.below(horizon.max(1));
            let d = dirs[rng.below(dirs.len() as u64) as usize];
            let a = if deletes && d != 0 && rng.chance(0.25) {
                Action::Delete(fs.path_of(d))
            } else if rng.chance(0.15) {
                Action::ClearFocus
            } else {
                Action::Focus(fs.path_of(d))
            };
            (t, a)
        })
        .collect();
    steps.sort_by_key(|s| s.0);
    Script { steps }
}

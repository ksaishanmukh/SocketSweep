//! The navigation scripts and the reference tree that picks their folders.

use std::time::Duration;

use serde::Serialize;
use socketsweep_sim::Rng;
use socketsweep_wire::ScanMode;

use crate::record::Recorder;

/// One navigation script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScriptKind {
    /// (a) No navigation.
    A,
    /// (b) Open the largest top-level folder at 300 ms, then its largest
    /// child at 2 s.
    B,
    /// (c) Ten opens of random top-level folders within 1 s.
    C,
}

impl ScriptKind {
    pub fn name(self) -> &'static str {
        match self {
            ScriptKind::A => "a",
            ScriptKind::B => "b",
            ScriptKind::C => "c",
        }
    }

    pub fn parse(s: &str) -> Option<ScriptKind> {
        match s {
            "a" => Some(ScriptKind::A),
            "b" => Some(ScriptKind::B),
            "c" => Some(ScriptKind::C),
            _ => None,
        }
    }
}

pub fn mode_name(mode: ScanMode) -> &'static str {
    match mode {
        ScanMode::Focus => "focus",
        ScanMode::Plain => "plain",
    }
}

/// A folder opened at a time after `StartScan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    pub at: Duration,
    /// The folder's full path on the phone.
    pub path: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub kind: ScriptKind,
    pub clicks: Vec<Click>,
}

/// The tree from a complete reference scan, to choose the folders the
/// scripts open: the same ones in every run and both modes.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    /// Top-level folders with their disk usage (subtree plus own entry is
    /// not needed for ranking: the subtree's totals).
    pub top: Vec<(Vec<u8>, u64)>,
    /// For each top-level folder, its child folders with their disk usage.
    pub children: Vec<Vec<(Vec<u8>, u64)>>,
}

impl Reference {
    pub fn from_scan(r: &Recorder) -> Reference {
        let mut dirs: Vec<(Vec<u8>, u32, u64)> = r
            .finished_dirs()
            .map(|(p, d, t)| (p.to_vec(), d, t.disk))
            .collect();
        dirs.sort();
        let top: Vec<(Vec<u8>, u64)> = dirs
            .iter()
            .filter(|d| d.1 == 1)
            .map(|d| (d.0.clone(), d.2))
            .collect();
        let children = top
            .iter()
            .map(|(tp, _)| {
                dirs.iter()
                    .filter(|d| d.1 == 2 && parent_of(&d.0) == tp.as_slice())
                    .map(|d| (d.0.clone(), d.2))
                    .collect()
            })
            .collect();
        Reference { top, children }
    }

    fn largest(v: &[(Vec<u8>, u64)]) -> Option<usize> {
        // Largest by disk, ties to the first by path.
        let mut best: Option<usize> = None;
        for (i, (_, n)) in v.iter().enumerate() {
            if best.is_none_or(|b| *n > v[b].1) {
                best = Some(i);
            }
        }
        best
    }

    /// The script `kind`; `seed` drives (c).
    pub fn script(&self, kind: ScriptKind, seed: u64) -> Script {
        let ms = Duration::from_millis;
        let clicks = match kind {
            ScriptKind::A => Vec::new(),
            ScriptKind::B => match Self::largest(&self.top) {
                None => Vec::new(),
                Some(t) => {
                    let mut v = vec![Click {
                        at: ms(300),
                        path: self.top[t].0.clone(),
                    }];
                    if let Some(c) = Self::largest(&self.children[t]) {
                        v.push(Click {
                            at: ms(2000),
                            path: self.children[t][c].0.clone(),
                        });
                    }
                    v
                }
            },
            ScriptKind::C if self.top.is_empty() => Vec::new(),
            ScriptKind::C => {
                let mut rng = Rng::new(seed);
                (0..10)
                    .map(|i| Click {
                        at: ms(100 + i * 100),
                        path: self.top[rng.below(self.top.len() as u64) as usize]
                            .0
                            .clone(),
                    })
                    .collect()
            }
        };
        Script { kind, clicks }
    }
}

fn parent_of(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|&b| b == b'/') {
        Some(i) => &p[..i],
        None => &p[..0],
    }
}

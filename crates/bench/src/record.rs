//! What one scan looked like from the host: every event's arrival time,
//! the directory table rebuilt from the stream, and derived counts.
//!
//! Times are taken when the scan's reply channel hands a batch over, which
//! is as soon as host-core's reader thread has decoded the frame.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use socketsweep_wire::{DirId, Entry, Outcome, ReqId, ScanEvent, ServerMsg, Totals, ROOT_DIR};

/// The `SkeletonReady` event as received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skeleton {
    pub at: Duration,
    /// The daemon's own figures.
    pub listed: u32,
    pub frontier: u32,
    pub elapsed_ms: u32,
    /// Levels completely listed when it arrived (the root is level 0, so 2
    /// means the root and every top-level folder).
    pub levels: u32,
}

/// The end of the scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    pub at: Duration,
    pub outcome: Outcome,
    pub totals: Totals,
    pub elapsed_ms: u32,
}

/// One scan's stream, as it arrives.
#[derive(Debug)]
pub struct Recorder {
    t0: Instant,
    /// Indexed by `DirId`.
    paths: Vec<Vec<u8>>,
    depth: Vec<u32>,
    done_at: Vec<Option<Duration>>,
    totals: Vec<Option<Totals>>,
    /// Entries and `None` stats seen in a directory's listing so far.
    listing: HashMap<DirId, (u64, u64)>,
    by_path: HashMap<Vec<u8>, DirId>,
    unlisted_by_depth: Vec<u32>,
    pub first_listing: Option<Duration>,
    pub skeleton: Option<Skeleton>,
    pub end: Option<End>,
    /// Focus `seq` to when `FocusState { settled: true }` arrived.
    pub settled: HashMap<u32, Duration>,
    /// Encoded bytes of every frame of this scan, length prefixes included.
    pub bytes: u64,
    pub frames: u64,
    pub events: u64,
    /// Directories listed (each exactly once).
    pub listed: u64,
    /// Listings that left their stats for later: each costs the phone one
    /// more directory open (a `StatDir`).
    pub deferred: u64,
    /// Entries listed; each is stat'ed exactly once in either mode.
    pub entries: u64,
}

impl Recorder {
    /// A recorder whose clock starts at `t0` (when `StartScan` was sent).
    pub fn new(t0: Instant) -> Self {
        Recorder {
            t0,
            paths: Vec::new(),
            depth: Vec::new(),
            done_at: Vec::new(),
            totals: Vec::new(),
            listing: HashMap::new(),
            by_path: HashMap::new(),
            unlisted_by_depth: Vec::new(),
            first_listing: None,
            skeleton: None,
            end: None,
            settled: HashMap::new(),
            bytes: 0,
            frames: 0,
            events: 0,
            listed: 0,
            deferred: 0,
            entries: 0,
        }
    }

    pub fn elapsed(&self, at: Instant) -> Duration {
        at.saturating_duration_since(self.t0)
    }

    pub fn is_done(&self) -> bool {
        self.end.is_some()
    }

    /// The directory at `path` (as the daemon spells it: the resolved root
    /// joined with names by `/`), once introduced.
    pub fn dir(&self, path: &[u8]) -> Option<DirId> {
        self.by_path.get(path).copied()
    }

    /// When `dir`'s `SubtreeDone` arrived.
    pub fn done_at(&self, dir: DirId) -> Option<Duration> {
        self.done_at.get(dir as usize).copied().flatten()
    }

    /// Every finished directory: (path, depth, totals).
    pub fn finished_dirs(&self) -> impl Iterator<Item = (&[u8], u32, Totals)> + '_ {
        self.totals
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.map(|t| (self.paths[i].as_slice(), self.depth[i], t)))
    }

    pub fn root_path(&self) -> Option<&[u8]> {
        self.paths.first().map(Vec::as_slice)
    }

    /// Take one message from scan `req`'s channel, received at `at`.
    pub fn on_msg(&mut self, req: ReqId, msg: &ServerMsg, at: Instant) -> Result<(), String> {
        let id_len = postcard::to_allocvec(&req).map_or(1, |v| v.len());
        let body = postcard::to_allocvec(msg).map_err(|e| e.to_string())?;
        self.bytes += (4 + id_len + body.len()) as u64;
        self.frames += 1;
        match msg {
            ServerMsg::Scan(batch) => {
                let t = self.elapsed(at);
                for e in batch {
                    self.on_event(e, t)?;
                }
                Ok(())
            }
            ServerMsg::Failed { code, message, .. } => {
                Err(format!("the scan failed: {code:?}: {message}"))
            }
            other => Err(format!("unexpected reply to StartScan: {other:?}")),
        }
    }

    fn introduce(&mut self, id: DirId, path: Vec<u8>, depth: u32) -> Result<(), String> {
        if id as usize != self.paths.len() {
            return Err(format!(
                "dir {id} introduced out of order ({} known)",
                self.paths.len()
            ));
        }
        self.by_path.insert(path.clone(), id);
        self.paths.push(path);
        self.depth.push(depth);
        self.done_at.push(None);
        self.totals.push(None);
        let d = depth as usize;
        if self.unlisted_by_depth.len() <= d {
            self.unlisted_by_depth.resize(d + 1, 0);
        }
        self.unlisted_by_depth[d] += 1;
        Ok(())
    }

    fn known(&self, dir: DirId) -> Result<usize, String> {
        let i = dir as usize;
        if i < self.paths.len() {
            Ok(i)
        } else {
            Err(format!("G1: dir {dir} referenced before it was introduced"))
        }
    }

    fn on_listing(
        &mut self,
        dir: DirId,
        entries: &[Entry],
        last: bool,
        t: Duration,
    ) -> Result<(), String> {
        let i = self.known(dir)?;
        if dir == ROOT_DIR {
            self.first_listing.get_or_insert(t);
        }
        let depth = self.depth[i];
        let base = self.paths[i].clone();
        let seen = self.listing.entry(dir).or_default();
        seen.0 += entries.len() as u64;
        seen.1 += entries.iter().filter(|e| e.stat.is_none()).count() as u64;
        self.entries += entries.len() as u64;
        for en in entries {
            if let Some(id) = en.id {
                let mut p = base.clone();
                if p.last() != Some(&b'/') {
                    p.push(b'/');
                }
                p.extend_from_slice(&en.name);
                self.introduce(id, p, depth + 1)?;
            }
        }
        if last {
            self.listed += 1;
            self.unlisted_by_depth[depth as usize] -= 1;
            let (n, none) = self.listing.remove(&dir).unwrap_or_default();
            // A skeleton (`Names`) listing has no stats at all, apart
            // from entries `d_type` could not classify; a full one
            // has `None` only where `lstat` failed.
            if n > 0 && none * 2 > n {
                self.deferred += 1;
            }
        }
        Ok(())
    }

    fn on_skeleton_ready(&mut self, listed: u32, frontier: u32, elapsed_ms: u32, t: Duration) {
        let levels = self
            .unlisted_by_depth
            .iter()
            .position(|&n| n > 0)
            .unwrap_or(self.unlisted_by_depth.len()) as u32;
        self.skeleton = Some(Skeleton {
            at: t,
            listed,
            frontier,
            elapsed_ms,
            levels,
        });
    }

    fn on_event(&mut self, e: &ScanEvent, t: Duration) -> Result<(), String> {
        self.events += 1;
        match e {
            ScanEvent::Started { root, .. } => self.introduce(ROOT_DIR, root.clone(), 0)?,
            ScanEvent::Listing {
                dir, entries, last, ..
            } => self.on_listing(*dir, entries, *last, t)?,
            ScanEvent::Stats { dir, .. } | ScanEvent::DirFailed { dir, .. } => {
                self.known(*dir)?;
            }
            ScanEvent::SubtreeDone { dir, totals } => {
                let i = self.known(*dir)?;
                self.done_at[i] = Some(t);
                self.totals[i] = Some(*totals);
            }
            ScanEvent::Pruned { dir } => {
                self.known(*dir)?;
            }
            ScanEvent::SkeletonReady {
                listed,
                frontier,
                elapsed_ms,
            } => self.on_skeleton_ready(*listed, *frontier, *elapsed_ms, t),
            ScanEvent::FocusState {
                seq, settled: true, ..
            } => {
                self.settled.entry(*seq).or_insert(t);
            }
            ScanEvent::FocusState { .. } | ScanEvent::Progress { .. } => {}
            ScanEvent::Finished {
                outcome,
                totals,
                elapsed_ms,
            } => {
                self.end = Some(End {
                    at: t,
                    outcome: *outcome,
                    totals: *totals,
                    elapsed_ms: *elapsed_ms,
                });
            }
        }
        Ok(())
    }
}

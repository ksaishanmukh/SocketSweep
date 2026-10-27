# SocketSweep architecture (v4)

This document is the specification for the v4 rebuild. Every module is built
against it. When code and this document disagree, fix one of them in the same
change.

## 1. Layers and contracts

```
React GUI ──(gui-contract: generated TS bindings)──► src-tauri (thin shell)
                                                         │
                                               host-core (no Tauri dependency)
                                     tree index · session client · focus debounce · actions
                                                         │
                                        (Transport trait)│        (wire v4 over the tunnel)
                                       transport-adb ────┴──────────► phone daemon
                                                                       listener · session · sched
                                                                       aggregate · pool · fswalk · actions
```

Each layer talks only to its immediate neighbour, and only through a contract
crate. A module's internals may change freely as long as its contract holds.
Every contract has a **fake** implementation, so each side can be tested alone.

| Boundary | Contract | How it is enforced | Fake |
|---|---|---|---|
| GUI ↔ host | `crates/gui-contract`: command and event types | `tauri-specta` generates `src/bindings.ts`; CI fails if it is stale. React code imports only `src/api/` | `src/api` mock in vitest |
| host ↔ transport | `crates/transport`: `trait Transport`, `trait Duplex` | Rust trait; host-core depends on the trait only | `FakeTransport` (in-memory device) |
| host ↔ phone | `crates/wire`: protocol v4 | One crate for both ends; golden byte tests; the version must match exactly | `FakeDaemon` (in `crates/sim`) |
| daemon internals | `trait Fs`, `trait EventSink`; `sched` is a pure state machine | Rust traits | `SimFs`, a recording sink |

### Workspace layout

| Path | Role |
|---|---|
| `crates/wire` | Host ↔ phone protocol types, framing, handshake |
| `crates/gui-contract` | GUI ↔ host command and event types (specta) |
| `crates/transport` | `Transport` trait and `FakeTransport` |
| `crates/transport-adb` | adb implementation: smart sockets, shell v2, sync SEND/RECV2, USB and wireless |
| `crates/host-core` | Tree index, session client, focus debounce, delete/copy orchestration, history |
| `crates/daemon` | Phone binary: `listener`, `session`, `sched`, `aggregate`, `pool`, `fswalk`, `actions` |
| `crates/sim` | Dev-only: `SimFs`, discrete-event simulator, jwalk-like baseline, `FakeDaemon` |
| `src-tauri` | Thin shell: maps generated commands to host-core and emits events |
| `src/` | React features that import only `src/bindings.ts`, through `src/api/` |

The daemon is built for `aarch64-linux-android` (phones) and
`x86_64-linux-android` (emulator, for end-to-end tests), at API 30 (Android 11)
and above. One phone at a time, but every contract carries a session id.

## 2. Wire protocol v4 (host ↔ phone)

### Framing
A 4-byte little-endian length followed by a postcard body. At most 16 MiB per
frame. Large directory listings are split into parts of at most 4,096 entries.

### Handshake
The first frame on every connection is a standalone `Hello`. Its first fields
are **frozen forever**, so any future daemon can read the version with
`postcard::take_from_bytes`.

```rust
pub const PROTOCOL_VERSION: u16 = 4;
pub type ReqId = u32;      // chosen by the host, never 0; 0 = session-level
pub type DirId = u32;      // assigned by the daemon per scan, dense, root = 0
pub type SessionId = [u8; 16];

pub struct Hello   { magic: [u8; 4] /* b"SSWP" */, version: u16, token: Token,
                     session: SessionId, caps: Caps }
pub struct Welcome { version: u16, session: SessionId, caps: Caps,
                     daemon_build: String, workers: u8, limits: Limits }
// Caps (u64 bitflags): DISK_USAGE, LISTING_PARTS, DEFERRED_STAT, COPY_MANIFEST, DIR_MTIME, RESUME
```

- **Order of checks:** token (constant-time comparison) → version (must match
  exactly; the daemon and host ship together) → capabilities.
- **Wrong token:** the daemon replies `Refused` and reveals nothing else.
- **Capabilities** describe what the phone can do, not compatibility between
  releases.
- **Token:** 32 random bytes per session, passed to the daemon in the
  `SOCKETSWEEP_TOKEN` environment variable, never on argv.

### Messages

```rust
pub struct ClientFrame { id: ReqId, msg: ClientMsg }
pub enum ClientMsg {
    Ping { nonce: u32 },
    StartScan { root: Vec<u8>, opts: ScanOpts },
    Focus { scan: ReqId, dir: DirId, seq: u32 },     // hint, no reply
    ClearFocus { scan: ReqId, seq: u32 },
    Cancel { target: ReqId },
    Delete { path: Vec<u8>, hint: Option<(ReqId, DirId)> },
    PrepareCopy { path: Vec<u8> },
    Shutdown,
}
pub struct ScanOpts { workers: Option<u8>, max_depth: u16, skeleton_budget_ms: u16,
                      skeleton_min_depth: u8, skeleton_max_depth: u8,
                      one_filesystem: bool, tiers: [TierCfg; 3], ramp_ms: u16 }

pub struct ServerFrame { id: ReqId, msg: ServerMsg }  // id 0 = session-level
pub enum ServerMsg {
    Pong { nonce: u32 }, Heartbeat,
    Scan(Vec<ScanEvent>),                              // batched, at most 64 KiB
    DeleteProgress { items: u64 }, Deleted { items: u64, totals: Totals },
    CopyManifest { root: Vec<u8>, files: Vec<CopyItem>, last: bool },
    Failed { code: ErrCode, errno: Option<i32>, message: String },  // ends the request
}
pub enum ScanEvent {
    Started { root: Vec<u8>, dev: u64, workers: u8 },               // introduces DirId 0
    Listing { dir: DirId, part: u16, last: bool, entries: Vec<Entry> },
    Stats   { dir: DirId, start: u32, stats: Vec<Option<Stat>> },    // deferred stat results
    DirFailed { dir: DirId, op: FsOp, errno: i32 },
    SubtreeDone { dir: DirId, totals: Totals },
    SkeletonReady { listed: u32, frontier: u32, elapsed_ms: u32 },
    FocusState { seq: u32, dir: DirId, settled: bool },
    Pruned { dir: DirId },
    Progress { listed: u32, known: u32, files: u64, disk: u64, pending: u32,
               alloc: [u8; 3], stalled: Vec<(DirId, u32 /* ms */)> },
    Finished { outcome: Outcome, totals: Totals, elapsed_ms: u32 },  // ends the scan
}
pub struct Entry  { name: Vec<u8>, kind: Kind, stat: Option<Stat>, id: Option<DirId> }
pub enum   Kind   { File, Dir, Symlink, Other }
pub struct Stat   { apparent: u64, blocks: u64, mtime: i64 }   // disk usage = blocks * 512
pub struct Totals { apparent: u64, disk: u64, files: u32, dirs: u32, errors: u32, flags: u8 }
```

- `Entry.id` is `Some` exactly when the entry is a directory that will be walked.
- `Entry.stat` is `None` in a skeleton listing. The stats arrive later in a
  `Stats` event, which refers to entries by their position in the listing.

### Ordering guarantees (the host relies on these)
- **G1 – introduced before referenced.** A `DirId` is introduced in its
  parent's `Listing` (the root in `Started`) before any event refers to it.
- **G2 – fixed order per directory.** For directory D: all `Listing(D)` parts,
  then `Stats(D)`, then each child's `SubtreeDone`, then `SubtreeDone(D)`.
- **G3 – each directory is reported once.** Each `DirId` is listed once, and is
  finished once or `Pruned`.

All three come from one rule. Every event is added to the bulk queue **while
holding the scheduler lock, at the moment the state changes**, and one writer
thread sends that queue in order. Child directories become available to
workers only after the event that introduces them has been queued. An unknown
`DirId` is a protocol violation and fails the scan. The host needs no reorder
buffer.

### Back-pressure, keepalive, cancellation
- **Back-pressure:** two queues. The bulk queue has a high-water mark of 4 MiB
  and a low-water mark of 2 MiB. Above the high mark, workers stop taking new
  tasks; adding to the queue never blocks, so the lock is never held while
  waiting. The control queue (`Pong`, `Deleted`, `Failed`) is always sent first.
- **Keepalive:** the writer thread sends `Heartbeat` after 2 s with nothing
  sent. The host sends `Ping` after 5 s idle. The host treats 15 s of silence as
  dead; the daemon gives up after 30 s. `Progress.stalled` lists directories
  being read for more than 3 s.
- **Cancellation:** `Cancel` sets a flag. Workers check it at task boundaries
  and every 256 entries. The scan always ends with
  `Finished { outcome: Cancelled }`. A lost connection cancels everything.

### Identity
- `DirId` is used only within one scan: in events, in `Focus`, and in the
  host's dense `Vec<NodeId>` lookup.
- **Path bytes are the lasting identity:** delete, copy, and the future
  snapshot cache. `Delete.hint` lets the scheduler prune that subtree; the
  daemon checks it against the path the guard resolved before using it.
- The session id replaces the v3 `gen` check: an id from another session is
  refused.

## 3. Daemon internals

std threads only, no async runtime: one reader thread, one writer thread, and
W worker threads.

| Module | Responsibility | Contract |
|---|---|---|
| `listener` | Bind the abstract socket and accept connections | An authenticated new connection replaces the old one |
| `session` | Handshake, reader thread, writer thread with two queues | `trait EventSink { fn bulk(&self, Vec<ScanEvent>); fn control(&self, ServerFrame); fn congested(&self) -> bool; }` |
| `fswalk` | Read a directory and stat its entries | `trait Fs { fn read_names(..); fn stat_all(..); fn list_full(..); }`. The real version uses rustix: `openat(O_DIRECTORY\|O_NOFOLLOW)`, `getdents64`, `statat(AT_SYMLINK_NOFOLLOW)` |
| `sched` | Directory table, tiers, allocation, choosing work. Pure: no threads, time passed in | inputs → `Vec<ScanEvent>` |
| `aggregate` | Running totals, open-child counts, the completion cascade | Lives in `ScanState` under the same lock as `sched` |
| `pool` | Worker threads | `loop { lock; pick; unlock; do task; lock; apply; unlock }` |
| `actions` | Delete (symlink-safe guard: canonicalise the parent, never follow the final component), copy manifest | — |

### Tasks
```rust
enum Task { Names(DirId) /* getdents + d_type only */,
            StatDir(DirId) /* deferred stats */,
            Full(DirId) /* getdents + stat together */ }
```
Workers are reassigned only between tasks; no task is interrupted part-way. A
directory entry whose type comes back as `DT_UNKNOWN` is stat'ed immediately.

### Directory table
```rust
struct DirRec {
    parent: DirId, name: Box<[u8]>, depth: u8, first_child: DirId, next_sibling: DirId,
    state: St,            // Pending | InFlight | NamesOnly | Listed | Done | Pruned
    stat_pending: bool,
    pending_desc: u32,    // runnable tasks in this subtree, including self
    active_desc: u16,     // workers currently inside this subtree
    open_children: u32,   // child directories not yet Done
    totals: Totals,
}
```
- **Counter upkeep:** every state change updates the counters along the chain of
  parents (at most 64 steps).
- **When a directory is Done:** it is listed, its stats are done, and
  `open_children == 0`.
- **Completion cascade (a loop, not recursion):** finishing directory D emits
  `SubtreeDone(D)`, adds D's totals to its parent, and decrements the parent's
  `open_children`.
- **Failures and limits:** directories that fail to open, or lie beyond
  `max_depth`, complete immediately with an error flag.

### Skeleton phase (breadth-first without barriers)
- **Ordering:** the Back tier hands out tasks shallowest-first, using per-depth
  buckets. No worker ever waits for a level to finish.
- **Phase end:** the skeleton phase ends when one of these holds: the frontier
  holds at least 4 × W directories **and** the depth reached is at least
  `skeleton_min_depth` (2), the 1 s budget runs out, or the depth reaches
  `skeleton_max_depth` (4).
- **After the skeleton:** remaining work is depth-first (preferring the highest
  `DirId`), using `Full` tasks. Skeleton directories still owe a `StatDir`.

### Proximity allocation (gradual hiring)
The three tiers are defined relative to the open folder F and its parent P. Each
tier's pending work is O(1), from the counters:

| Tier | Contents | Pending work |
|---|---|---|
| `Open` | subtree(F) | `pending_desc(F)` |
| `Near` | the subtrees of F's siblings | `pending_desc(P) − pending_desc(F)` |
| `Back` | everything else | `pending_desc(root) − pending_desc(P)` |

With no focus, or with the focus at the root, everything is `Open`. The
scheduler then behaves like a plain parallel walk.

```rust
struct TierCfg { weight: f32, floor: u8, cap: u8 }
// Defaults: Open { 3.0, 0, MAX } (ramps from 1.0), Near { 1.0, 1, 2 },
//           Back { 4.0, max(1, W/4), MAX }
```

**Dwell ramp.** `w_open(t) = 1 + 2 · min(1, dwell / ramp)`, with `ramp` = 400 ms.
A folder the user has just opened gets one worker; it gets its full share only
if the user stays.

**Seats (D'Hondt), recomputed in O(W) on every pick:**
1. Give the floors first.
2. Give each remaining seat to the tier with the highest
   `weight / (seats + 1)`, among tiers that are below their cap and still have
   work. Ties go to the nearer tier.
3. Seats a tier cannot use go to the other tiers, so no worker sits idle while
   any work exists.

| W | Steady state (open / near / back) | First 400 ms after a click | Focus fully measured |
|---|---|---|---|
| 8 (default) | 3 / 1 / 4 | 1 / 1 / 6 | 0 / 2 / 6 |

As the user moves closer to a folder X, X's share grows: about 0.3 → 1 → 1–2 →
3 workers.

**`pick(worker)`:**
1. Release the worker's seat. Wait if the queue is congested or the scan is
   cancelled.
2. Choose the tier with the largest `target − active`. Ties go first to the
   worker's previous tier, then to the nearer tier.
3. **Anchor.** Keep the previous anchor if it is still in that tier and has
   work. Otherwise:
   - Open: among F's children, the one with the fewest workers, then the least
     work.
   - Near: among the siblings, the one with the fewest workers.
   - Back: outside P, the subtree with the fewest workers, then the **most**
     work, so the biggest jobs start first and the scan's tail stays short.
4. **Descent:** shallowest-first for skeleton work in Back, otherwise
   depth-first.

**Focus change.** Only the focus is replaced. Each in-flight worker's tier is
recomputed from where it currently is. Workers move one at a time as they
finish tasks, until the counts match the targets. Each task moves from waiting
to in-progress exactly once, under the lock, so nothing is lost or done twice.

**Avoiding churn:**
- the host combines focus changes over 150 ms and sends only the last;
- the dwell ramp;
- workers keep their tier and anchor when they can;
- `seq` ordering discards stale focus messages;
- moves happen only at task boundaries.

**Worker count:** `W = clamp(2 × cores, 4, 8)`, which `ScanOpts` can override.
The limit is the phone's storage service (MediaProvider), not the CPU.

### Delete during a scan
- The subtree is marked `Pruned` and its counts are removed.
- If the deleted node was finished, or is a file, its totals are subtracted
  from each parent up to and including the first parent that is not yet
  finished.
- If it was not finished, the parent's `open_children` goes down by one, which
  may let the parent complete.
- Results from workers still inside the deleted subtree are discarded.
- The daemon emits `Pruned`, then deletes the files and replies `Deleted`.

### Copy
1. `PrepareCopy` resolves the path through the guard and streams a
   `CopyManifest` listing `{ rel, size, mode, mtime }`. Symlinks are listed as
   skipped.
2. The host pulls the bytes with `Transport::pull`: adb sync `RECV2`, falling
   back to `RECV`, over 2–3 sync connections in parallel.
3. Mapping Android file names to valid Windows/macOS names is the host's job.

## 4. Host side

- **Session client:** one reader thread decodes frames and routes them by
  `ReqId`. Scan events go to the tree ingest channel; replies go to one-shot
  channels. Writes go through a mutex.
- **Size states per directory:**
  - `Unknown`: not listed yet; the GUI shows "measuring…".
  - `Partial`: listed. The number is a lower bound that climbs as file sizes
    are added up the chain of parents.
  - `Final`: set on `SubtreeDone`. The host applies the difference up the chain
    of parents, and the daemon's figure wins.

  A file is unknown until its `Stats` arrive. Disk usage is the default display;
  apparent size is a GUI setting.
- **Focus:** opening a folder (by click, breadcrumb, treemap drill-down, or "go
  to" from search) maps it to a `DirId`. A file maps to its parent folder. The
  first change is sent immediately; later changes are combined over 150 ms.
  Hover and scrolling never change focus. `FocusState.settled` clears the
  folder's spinner.

## 5. Testing

- **Simulator (`crates/sim`):**
  - `SimFs` generates seeded `/sdcard`-shaped trees: about 300 packages under
    `Android/data`, a 5,000-file camera folder, one 60,000-file folder, deep
    chains, and thousands of empty directories.
  - The latency model has separate costs for listing names and for stats,
    multiplied by a contention factor.
  - A discrete-event simulator with a virtual clock and W virtual workers.
  - A jwalk-like baseline engine for comparison.
- **Properties (proptest):**
  - each directory is listed and stat'ed exactly once;
  - totals match the ground truth, including after deletes;
  - G1–G3 hold;
  - after the ramp, each tier's active count is within 1 of its target within
    one task length;
  - the Back floor holds whenever Back has work;
  - `SkeletonReady` arrives within 1 s of virtual time.

  Golden traces catch scheduling regressions.
- **Integration:**
  - the threaded pool runs against `RealFs` and a `SlowFs` wrapper that adds
    delays;
  - host-core runs against `FakeDaemon` and `FakeTransport`;
  - an emulator end-to-end CI job uses the x86_64 daemon and a seeded fixture
    tree.

## 6. Benchmark and acceptance criteria

- **Engines:** the daemon gets `--engine {jwalk,v4}`. The jwalk engine is the
  v3 walk, emitting v4 events with no focus.
- **Targets:**
  - the emulator with a fixture of 50,000 files, 3,000 directories and one
    30,000-file directory;
  - a Pixel and a Samsung scanning their real `/sdcard`;
  - `SimFs` in CI.
- **Runs:** 10 warm runs per configuration, interleaved A/B, plus one cold run.
  Report the median and the 90th percentile.
- **Scripts:**
  - (a) no navigation;
  - (b) open the largest top-level folder at 300 ms, then drill into its
    largest child at 2 s;
  - (c) 10 clicks within 1 s.

Acceptance criteria:
1. Directory opens and stats match the baseline, with at most 1% extra
   (handles reopened for deferred stats).
2. Script (a): total time ≤ 1.05× jwalk at the median, ≤ 1.10× at the 90th
   percentile.
3. Script (c): total time ≤ 1.10× jwalk, and the Back floor always holds.
4. `SkeletonReady` within 1.0 s, covering at least 2 levels.
5. Script (b): the open folder's answer at ≤ 0.5× the baseline's time.
6. Daemon CPU ≤ 1.2× the baseline; peak memory ≤ 32 MB at 100,000
   directories; scheduler time < 1% of worker time.

If criterion 2 fails, try these in order: turn off deferred stats outside the
Open tier, retune the weights, then revisit the design.

## 7. Open risks
- The scan may stop getting faster beyond some worker count, because the
  phone's storage service handles a limited number of requests at once.
  Measure W on real phones.
- Whether `d_type` and `st_blocks` are reliable on shared storage. If
  `blocks == 0` while the size is not, round the size up to 4 KiB and set a
  capability flag.
- One huge directory pins one worker. A 60,000-file directory takes about 6 s.
  Splitting it into chunks is a later option.
- Hardlinks are counted once per `(dev, ino)` when the link count is above 1.
  `one_filesystem` handles bind mounts.
- A dropped wireless adb connection cancels the scan. Resuming it is the later
  `RESUME` capability.
- A snapshot cache comes later: it would key on the path relative to the root,
  plus the directory's `mtime` and `(dev, ino)`.

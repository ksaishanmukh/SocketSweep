# SocketSweep architecture (v4)

This document is the specification for the v4 rebuild. Every module is built
against it. When code and this document disagree, fix one of them in the same
change.

For a map of the code and a reading order, see [code-tour.md](code-tour.md).

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

| Boundary         | Contract                                                       | How it is enforced                                                                                      | Fake                               |
| ---------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| GUI ↔ host       | `crates/gui-contract`: command and event types                 | `tauri-specta` generates `src/bindings.ts`; CI fails if it is stale. React code imports only `src/api/` | `src/api` mock in vitest           |
| host ↔ transport | `crates/transport`: `trait Transport`, `trait Duplex`          | Rust trait; host-core depends on the trait only                                                         | `FakeTransport` (in-memory device) |
| host ↔ phone     | `crates/wire`: protocol v5                                     | One crate for both ends; golden byte tests; the version must match exactly                              | `FakeDaemon` (in `crates/sim`)     |
| daemon internals | `trait Fs`, `trait EventSink`; `sched` is a pure state machine | Rust traits                                                                                             | `SimFs`, a recording sink          |

### Workspace layout

| Path                   | Role                                                                                                                      |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `crates/wire`          | Host ↔ phone protocol types, framing, handshake                                                                           |
| `crates/gui-contract`  | GUI ↔ host command and event types (specta)                                                                               |
| `crates/transport`     | `Transport` trait and `FakeTransport`                                                                                     |
| `crates/transport-adb` | adb implementation: smart sockets, shell v2, sync SEND/RECV2, USB and wireless                                            |
| `crates/host-core`     | Tree index, session client, focus debounce, delete/copy orchestration, history                                            |
| `crates/daemon`        | Phone binary (lib + bin): `listener`, `session`, `outbox`, `fs_rustix` (`fswalk`), `actions`; the scan is `crates/engine` |
| `crates/engine`        | `sched`, `aggregate`, `pool`, `trait Fs`, `trait EventSink`, `StdFs`                                                      |
| `crates/sim`           | Dev-only: `SimFs`, discrete-event simulator, jwalk-like baseline, `FakeDaemon`                                            |
| `crates/bench`         | Dev-only: on-device benchmark (focus vs plain mode), the seeded device fixture, the emulator end-to-end test              |
| `src-tauri`            | Thin shell: maps generated commands to host-core and emits events                                                         |
| `src/`                 | React features that import only `src/bindings.ts`, through `src/api/`                                                     |

The daemon is built for `aarch64-linux-android` (phones) and
`x86_64-linux-android` (emulator, for end-to-end tests), at API 30 (Android 11)
and above. One phone at a time, but every contract carries a session id.

## 2. Wire protocol v5 (host ↔ phone)

### Framing

A 4-byte little-endian length followed by a postcard body. At most 16 MiB per
frame. Large directory listings are split into parts of at most 4,096 entries.

### Handshake

The first frame on every connection is a standalone `Hello`. Its first fields
are **frozen forever**, so any future daemon can read the version with
`postcard::take_from_bytes`.

```rust
pub const PROTOCOL_VERSION: u16 = 5;
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
                      one_filesystem: bool, tiers: [TierCfg; 3], ramp_ms: u16,
                      mode: ScanMode }
pub enum ScanMode { Focus, Plain }   // Plain: the benchmark baseline (section 6)

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
pub struct Totals { apparent: u64, disk: u64, files: u32, dirs: u32, errors: u32, flags: u8,
                     newest_mtime: i64 }
```

- `Totals.newest_mtime` is the maximum `Stat.mtime` (Unix seconds) over every entry counted below the directory, subdirectories' own entries included (a directory's mtime moves when something directly inside it is added or removed). It merges up with `max`, entries whose stat failed contribute nothing, and 0 means no mtime is known. It is a high-water mark: a prune or delete never lowers it, because the survivors would have to be revisited to find the next-newest.
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

| Module      | Responsibility                                                                                        | Contract                                                                                                                                                                           |
| ----------- | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `listener`  | Bind the abstract socket and accept connections                                                       | An authenticated new connection replaces the old one                                                                                                                               |
| `session`   | Handshake, reader thread, writer thread with two queues                                               | `trait EventSink { fn bulk(&self, Vec<ScanEvent>); fn control(&self, ServerFrame); fn congested(&self) -> bool; }`                                                                 |
| `fswalk`    | Read a directory and stat its entries                                                                 | `trait Fs { fn read_names(..); fn stat_all(..); fn list_full(..); }`. The real version uses rustix: `openat(O_DIRECTORY\|O_NOFOLLOW)`, `getdents64`, `statat(AT_SYMLINK_NOFOLLOW)` |
| `sched`     | Directory table, tiers, allocation, choosing work. Pure: no threads, time passed in                   | inputs → `Vec<ScanEvent>`                                                                                                                                                          |
| `aggregate` | Running totals, open-child counts, the completion cascade                                             | Lives in `ScanState` under the same lock as `sched`                                                                                                                                |
| `pool`      | Worker threads                                                                                        | `loop { lock; pick; unlock; do task; lock; apply; unlock }`                                                                                                                        |
| `actions`   | Delete (symlink-safe guard: canonicalise the parent, never follow the final component), copy manifest | —                                                                                                                                                                                  |

### Tasks

```rust
enum Task { Names(DirId) /* getdents + d_type only */,
            StatDir(DirId) /* deferred stats */,
            Full(DirId) /* getdents + stat together */ }
```

Workers are reassigned only between tasks; no task is interrupted part-way. A
directory entry whose type comes back as `DT_UNKNOWN` is stat'ed immediately
(by the `Fs` implementation, inside the `Names` read), and never again.

The engine (`crates/engine`) implements `sched`, `aggregate` and `pool`; the
daemon supplies the rustix `Fs` and the session's `EventSink`. `ScanState` is
the pure state machine: `pick`, `apply`, `set_focus`, `clear_focus`, `prune`,
`cancel` and `tick` take the time as an argument and queue wire events at the
moment the state changes. `pool::Scan` runs it on W worker threads plus a ticker
(skeleton budget, `Progress`), and is the handle the session uses for `Focus`,
`ClearFocus`, `Cancel` and delete prunes.

### The daemon process (`crates/daemon`)

```text
SOCKETSWEEP_TOKEN=<64 hex> socketsweep-daemon --socket <abstract-name> [--root <path>] [--threads <n>]
```

`--root` (default `/sdcard`) confines every request; `--threads` sets the
default workers per scan reported in `Welcome`. Logs go to stderr. Exit codes:
0 after `Shutdown`, 1 if the socket cannot be bound, 2 for bad arguments, 3 for
a missing or malformed token, 4 on a host that is not Linux or Android.

The crate is a library plus that binary. Everything but `fs_rustix` and
`listener` is portable and tested on every host:

- `session` sees the connection as a reader, a writer and a shutdown hook
  (`Conn`), and the phone as `Services`: an `Arc<dyn Fs>` and an
  `Arc<dyn Actions>`. `crates/sim`'s `FakeDaemon` runs it over an in-memory
  pipe with `SimFs` and `SimActions`.
- `outbox` is the writer: control frames first, then bulk events, each encoded
  as it is queued and batched per scan into `Scan` frames of at most 64 KiB,
  waiting at most 50 ms to fill a batch; `Heartbeat` after 2 s idle; the host
  is given up after 30 s of silence, which shuts the connection.
- `actions::Actions` resolves request paths and performs delete and copy.
  `LocalActions` is the real one (`std::fs` plus the guard).

Request handling:

- Ids must be non-zero and not in use (`BadRequest`, `DuplicateId`). Several
  scans, deletes and copies may run at once; `Busy` is not used.
- A hint (`Focus`, `ClearFocus`, `Cancel`) naming no running request is
  answered `Failed { BadRequest }` under the hint's own id, which the host
  ignores.
- `StartScan` resolves the root (all links followed, at or under `--root`),
  then opens it; either failure is `Failed` before `Started`.
- `Delete` resolves through the guard, prunes the hinted directory only if the
  scan's path for it equals the resolved path (otherwise the hint is ignored),
  then deletes, sending `DeleteProgress` at most every 500 ms. `Deleted.totals`
  covers everything removed, the named entry itself included. Being a control
  frame, `Deleted` may overtake the scan's `Pruned`.
- `PrepareCopy` needs `COPY_MANIFEST`; parts carry at most 4,096 items, the
  named entry first (`rel` empty), each directory before its contents.
- A lost connection cancels everything it started; an authenticated new
  connection shuts the previous one.

### Directory table

```rust
struct DirRec {
    parent: DirId, name: Box<[u8]>, depth: u8, first_child: DirId, n_children: u32,
    state: St,            // Pending | InFlight | NamesOnly | Listed | Done | Pruned
    stat: StatSt,         // Pending | InFlight | Done (deferred stats)
    pending_desc: u32,    // runnable tasks in this subtree, including self
    active_desc: u16,     // workers currently inside this subtree
    open_children: u32,   // child directories not yet Done
    totals: Totals,
}
```

- **Children** get consecutive ids when their parent is listed, so they are
  `first_child .. first_child + n_children`; no sibling links are needed.
- **Counter upkeep:** every state change updates the counters along the chain of
  parents (at most 64 steps).
- **When a directory is Done:** it is listed, its stats are done,
  `open_children == 0`, and (for G2) its parent's stats are done. A child of a
  directory listed by `Names` can be measured before its parent's deferred
  stats arrive; it then waits, complete, until they are applied, and applying
  them finishes every such child before the parent itself.
- **Completion cascade (a loop, not recursion):** finishing directory D emits
  `SubtreeDone(D)`, adds D's totals to its parent, and decrements the parent's
  `open_children`.
- **Failures and limits:** a directory that fails to open emits `DirFailed` and
  completes with `errors: 1` and `UNREADABLE`. A subdirectory beyond
  `max_depth` is not walked: it is listed with `id: None` (so it never gets a
  `DirId`), counted as an entry, and its parent gets `DEPTH_LIMIT`. With
  `one_filesystem`, a directory whose own device (from the open handle; a
  `Names` listing has no stats) differs from the root's is listed as empty and
  completes with `OTHER_FS`; its entries were read once and are discarded.
- **`None` stats:** a `None` stat in a `Listing` always promises a `Stats` slot.
  For a `Full` listing, entries whose `lstat` failed are sent straight after
  the listing as `Stats` runs of `None`. If a `StatDir` cannot reopen its
  directory, every owed entry is `None` and the totals get `UNREADABLE`; no
  `DirFailed` is sent, since the listing succeeded.
- **Hardlinks:** a non-directory with `nlink > 1` adds its size once per
  `(dev, ino)` in the scan (the engine keeps the set; the wire `Stat` has no
  inode). Later links still count in `files`. A prune does not forget inodes.

### Skeleton phase (breadth-first without barriers)

- **Ordering:** the Back tier (or, with no focus, everything) hands out `Names`
  tasks shallowest-first, using per-depth FIFO buckets. No worker ever waits
  for a level to finish. With a focus, the Open and Near tiers use `Full`
  tasks even during the skeleton phase.
- **Phase end:** the skeleton phase ends when one of these holds: the frontier
  holds at least 4 × W directories **and** the depth reached is at least
  `skeleton_min_depth` (2), the 1 s budget runs out, or the depth reaches
  `skeleton_max_depth` (4). The _frontier_ is the directories known but not
  yet listed (waiting or being read); the _depth reached_ is the shallowest
  depth that still has such a directory, so every level above it is listed.
- **After the skeleton:** remaining work is depth-first (preferring the highest
  `DirId`), using `Full` tasks. Skeleton directories still owe a `StatDir`.
- **Big jobs:** a skeleton listing tells the size of its deferred stats. A
  `StatDir` of at least 2,048 entries is handed out before anything else in its
  tier, largest first, so one huge folder does not become the scan's tail.

### Proximity allocation (gradual hiring)

The three tiers are defined relative to the open folder F and its parent P. Each
tier's pending work is O(1), from the counters:

| Tier   | Contents                     | Pending work                           |
| ------ | ---------------------------- | -------------------------------------- |
| `Open` | subtree(F)                   | `pending_desc(F)`                      |
| `Near` | the subtrees of F's siblings | `pending_desc(P) − pending_desc(F)`    |
| `Back` | everything else              | `pending_desc(root) − pending_desc(P)` |

With no focus, or with the focus at the root, everything is `Open`. The
scheduler then behaves like a plain parallel walk, choosing anchors by the Back
rule (fewest workers, then most work).

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
   any work exists. A tier "can use" its running workers plus its waiting
   tasks. Seats left over after every tier reaches its cap go, by the same
   quotient, to tiers that still have work beyond their cap.

If the floors exceed W, they are given Back first, then Near, then Open.

| W           | Steady state (open / near / back) | First 400 ms after a click | Focus fully measured |
| ----------- | --------------------------------- | -------------------------- | -------------------- |
| 8 (default) | 3 / 1 / 4                         | 1 / 1 / 6                  | 0 / 1 / 7            |

The last column follows from the rule: once Open has no work, Near's quotient
`1 / 2` never beats Back's `4 / 8`, so Near keeps only its floor.

As the user moves closer to a folder X, X's share grows: about 0.3 → 1 → 1–2 →
3 workers.

**`pick(worker)`:**

1. Release the worker's seat. Wait if the queue is congested or the scan is
   cancelled.
2. If Back has work and is below its floor, choose Back. Otherwise, a tier
   with work below its floor; otherwise the tier with the largest
   `target − active`. Only tiers with waiting tasks count. Ties go first to the
   worker's previous tier, then to the nearer tier.
3. **Anchor.** Keep the previous anchor if it is still in that tier and has
   work. Otherwise:
   - Open: among F's children, the one with the fewest workers, then the least
     work.
   - Near: among the siblings, the one with the fewest workers.
   - Back: outside P, the subtree with the fewest workers, then the **most**
     work, so the biggest jobs start first and the scan's tail stays short.
   - A tier root whose subtree straddles tiers (F, P, or an ancestor of P) is
     never an anchor; its own task (a `StatDir`, which gates its children's
     completion) is taken directly, before any anchor.
4. **Descent:** shallowest-first for skeleton work in Back, otherwise
   depth-first: a directory's own task first, then the highest-id child with
   work.

**Focus change.** Only the focus is replaced. Each in-flight worker's tier is
recomputed from where it currently is. Workers move one at a time as they
finish tasks, until the counts match the targets. Each task moves from waiting
to in-progress exactly once, under the lock, so nothing is lost or done twice.

**Avoiding churn:**

- the host combines focus changes over 150 ms and sends only the last;
- the dwell ramp;
- workers keep their tier and anchor when they can;
- `seq` ordering discards stale focus messages (a `seq` not above the last
  `Focus` or `ClearFocus` seen; an unknown or pruned `DirId` uses up its `seq`
  and is ignored);
- focusing the folder already open keeps its dwell;
- moves happen only at task boundaries.

**Worker count:** `W = clamp(2 × cores, 4, 8)`, which `ScanOpts` can override.
The limit is the phone's storage service (MediaProvider), not the CPU.

### Delete during a scan

- The subtree is marked `Pruned` and its counts are removed.
- If the deleted node was finished, its totals plus its own entry are
  subtracted from each parent up to and including the first parent that is not
  yet finished. If it was not, only its own entry is subtracted from its
  parent. Flags are not cleared. `Delete.hint` names directories only, so a
  file deleted during a scan is not pruned; a later stat of it is an error.
- If it was not finished, the parent's `open_children` goes down by one, which
  may let the parent complete.
- Results from workers still inside the deleted subtree are discarded; a
  pruned child's slot in its parent's later `Stats` is `None` and is not
  counted. A focus inside the subtree is cleared.
- The daemon emits `Pruned` (`Scan::prune`, before touching the files), then
  deletes the files and replies `Deleted`.

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
  - A jwalk-like baseline engine (per-worker LIFO, stealing the oldest) to
    check that the engine's plain mode (section 6) is a fair baseline: on
    the standard tree it must finish within 1.10× of it.
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
  - an emulator end-to-end CI job (`.github/workflows/e2e.yml`, nightly and
    by hand, not on pull requests) boots an API 33 x86_64 emulator, builds
    the x86_64 daemon and runs `socketsweep-bench e2e`: it pushes the seeded
    fixture (section 6) to `/sdcard/ssfixture` as a tar archive, then, through
    host-core's `App` as the GUI would: connect → scan → `SkeletonReady` →
    focus the fixture until settled → the scan completes and the fixture's
    files, folders and bytes equal what `find`/`stat` report on the phone →
    delete a folder (gone from the phone and from the totals) → copy a folder
    and compare it byte for byte with the fixture → disconnect (daemon
    stopped and removed). The same job runs a three-run benchmark and keeps
    its JSON as an artifact.

## 6. Benchmark and acceptance criteria

- **Baseline:** the engine's plain mode, `ScanOpts { mode: ScanMode::Plain }`,
  is what a straightforward parallel walk does, run by the same daemon, pool,
  file system code and wire, so only the v4 scheduling differs:
  - no skeleton phase, so no `SkeletonReady`;
  - no deferred stats: every directory is one `Full` task (`getdents` and
    `lstat` together);
  - the focus does not steer the workers. `Focus` is still acknowledged with
    `FocusState`, settled when the folder's `SubtreeDone` is sent, so the
    host works unchanged;
  - workers spread over the root's subtrees (fewest workers, then most known
    work) and walk each depth first, as with no focus in focus mode.

  The v3 jwalk engine is gone; the simulator keeps a jwalk-like walk only to
  check that the plain mode is not a straw man (section 5).

- **Tool:** `socketsweep-bench` (`crates/bench`) runs both modes on one
  connection over adb and prints a median/p90 table plus the criteria below,
  and writes every run as JSON. It times events as host-core's client
  delivers them; directory opens are derived from the stream (one per
  listing, plus one per skeleton listing for its deferred stats).
- **Targets:**
  - the emulator with the seeded fixture (`socketsweep-bench fixture`): 50,000
    files, 3,000 directories and one 30,000-file directory;
  - a Pixel and a Samsung scanning their real `/sdcard`;
  - `SimFs` in CI.
- **Runs:** 10 warm runs per configuration, interleaved A/B (alternating
  which mode goes first), after one unmeasured warm-up per mode. A cold run
  needs root to drop the page cache and is done by hand. Report the median
  and the 90th percentile.
- **Scripts:** the folders are chosen from a complete reference scan, so
  every run opens the same ones:
  - (a) no navigation;
  - (b) open the largest top-level folder at 300 ms, then drill into its
    largest child at 2 s;
  - (c) 10 opens of random top-level folders within 1 s (seeded).

  A folder's answer time is from the click to its `SubtreeDone`, in both
  modes.

Acceptance criteria:

1. Directory opens and stats match the baseline, with at most 1% extra
   (handles reopened for deferred stats).
2. Script (a): total time ≤ 1.05× the baseline at the median, ≤ 1.10× at the
   90th percentile.
3. Script (c): total time ≤ 1.10× the baseline, and the Back floor always
   holds.
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

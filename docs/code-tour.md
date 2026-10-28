# Code tour

A reading guide to the code. [architecture.md](architecture.md) is the design
spec (why); this page is the map (where). Paths are relative to the repo root
unless a link says otherwise.

## 1. Map

| Path                                              | What it does                                                                | Start here                                                                                                                                                                                                          |
| ------------------------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`crates/wire`](../crates/wire)                   | Host-to-phone protocol: message types, framing, handshake, token.           | [`messages/`](../crates/wire/src/messages) (`client.rs`, `server.rs`, `options.rs`), [`framing.rs`](../crates/wire/src/framing.rs), [`handshake.rs`](../crates/wire/src/handshake.rs)                               |
| [`crates/gui-contract`](../crates/gui-contract)   | The GUI-to-host API: commands, events, payload types. Generates TS.         | [`commands.rs`](../crates/gui-contract/src/commands.rs), [`events.rs`](../crates/gui-contract/src/events.rs), [`bindings.rs`](../crates/gui-contract/src/bindings.rs)                                               |
| [`crates/transport`](../crates/transport)         | The `Transport` trait (a byte tunnel to the daemon) plus an in-memory fake. | [`lib.rs`](../crates/transport/src/lib.rs), [`fake/`](../crates/transport/src/fake)                                                                                                                                 |
| [`crates/transport-adb`](../crates/transport-adb) | The real transport: talks to the adb server (smart sockets, shell, sync).   | [`transport.rs`](../crates/transport-adb/src/transport.rs), [`wire/`](../crates/transport-adb/src/wire), [`devices.rs`](../crates/transport-adb/src/devices.rs)                                                     |
| [`crates/engine`](../crates/engine)               | The scan brain on the phone: scheduler, aggregation, worker pool, `Fs`.     | [`pool.rs`](../crates/engine/src/pool.rs), [`sched/`](../crates/engine/src/sched), [`aggregate/`](../crates/engine/src/aggregate), [`fs.rs`](../crates/engine/src/fs.rs), [`sink.rs`](../crates/engine/src/sink.rs) |
| [`crates/daemon`](../crates/daemon)               | The phone binary: listener, session, outbox, delete/metadata actions.       | [`listener.rs`](../crates/daemon/src/listener.rs), [`session/`](../crates/daemon/src/session), [`outbox/`](../crates/daemon/src/outbox), [`actions/`](../crates/daemon/src/actions)                                 |
| [`crates/host-core`](../crates/host-core)         | Everything the desktop does, with no Tauri: tree index, client, actions.    | [`app/`](../crates/host-core/src/app), [`client/`](../crates/host-core/src/client), [`tree/`](../crates/host-core/src/tree), [`focus.rs`](../crates/host-core/src/focus.rs)                                         |
| [`crates/sim`](../crates/sim)                     | Dev only: simulated filesystem, simulator, checkers, `FakeDaemon`.          | [`simfs.rs`](../crates/sim/src/simfs.rs), [`fake_daemon.rs`](../crates/sim/src/fake_daemon.rs), [`des.rs`](../crates/sim/src/des.rs), [`checker.rs`](../crates/sim/src/checker.rs)                                  |
| [`crates/bench`](../crates/bench)                 | Benchmark, fixture and end-to-end runner against a real device.             | [`main.rs`](../crates/bench/src/main.rs) (usage), [`bench.rs`](../crates/bench/src/bench.rs), [`e2e.rs`](../crates/bench/src/e2e.rs), [`fixture.rs`](../crates/bench/src/fixture.rs)                                |
| [`src-tauri`](../src-tauri)                       | Thin Tauri shell: one Tauri command per contract command, event forwarding. | [`src/lib.rs`](../src-tauri/src/lib.rs) (`shell_commands!`, `forward`), [`tauri.conf.json`](../src-tauri/tauri.conf.json)                                                                                           |
| [`src/`](../src)                                  | The React GUI.                                                              | [`api/index.ts`](../src/api/index.ts), [`hooks/`](../src/hooks), [`components/`](../src/components), [`lib/`](../src/lib)                                                                                           |
| [`scripts/`](../scripts)                          | Node helpers: fetch adb, cross-compile the daemon.                          | [`setup-bins.mjs`](../scripts/setup-bins.mjs), [`build-daemon.mjs`](../scripts/build-daemon.mjs)                                                                                                                    |
| [`.github/workflows/`](../.github/workflows)      | CI: build and checks, nightly emulator end-to-end.                          | [`build.yml`](../.github/workflows/build.yml), [`e2e.yml`](../.github/workflows/e2e.yml)                                                                                                                            |

Dependencies point one way: `wire` and `gui-contract` are the base. The phone
side (`engine`, `daemon`) and the desktop side (`host-core`) never see each
other; they meet only through `wire`. `src/bindings.ts` is generated; do not
edit it.

## 2. Reading order

1. **Contracts first.** Everything else is an implementation of these.
   - [`wire/messages`](../crates/wire/src/messages): the whole phone protocol
     in a few small files. Read `ClientMsg`, `ServerMsg` and `ScanEvent`.
   - [`gui-contract/commands.rs`](../crates/gui-contract/src/commands.rs): the
     single list of GUI commands. Then `events.rs` and `nodes.rs` for the types.
   - [`transport/lib.rs`](../crates/transport/src/lib.rs): the byte tunnel
     both sides use; `transport-adb` can wait.
2. **Phone side.** Start with the pure parts.
   - [`engine/fs.rs`](../crates/engine/src/fs.rs) (`Fs`), then
     [`engine/sched/`](../crates/engine/src/sched): a pure state machine.
     Read `mod.rs`, `task.rs`, `tiers.rs`, `pick.rs`, `focus.rs`.
   - [`engine/aggregate/`](../crates/engine/src/aggregate) and
     [`pool.rs`](../crates/engine/src/pool.rs): what workers emit and how
     threads drive the scheduler.
   - [`daemon/session/`](../crates/daemon/src/session) and
     [`daemon/outbox/`](../crates/daemon/src/outbox): wire in, events out.
3. **Host side.**
   - [`host-core/client/`](../crates/host-core/src/client): the connection and
     reader thread.
   - [`host-core/tree/`](../crates/host-core/src/tree): the in-memory index
     built from events (`ingest/`, `sizes.rs`, `queries.rs`).
   - [`host-core/app/`](../crates/host-core/src/app): connect, scan, commands;
     plus [`focus.rs`](../crates/host-core/src/focus.rs) and `copy/`.
   - [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs): short; just wiring.
4. **GUI.** [`src/api`](../src/api) (the only import of bindings), then
   [`src/hooks`](../src/hooks) (`useScanSession`, `useSessionLifecycle`,
   `useNavigation`, `useScanProgress`), then
   [`src/components`](../src/components) and `screens/`.
5. **Tests, sim and bench last.** They make sense once you know what they
   check: [`crates/sim/tests`](../crates/sim/tests),
   [`crates/daemon/tests/session.rs`](../crates/daemon/tests/session.rs),
   [`crates/bench`](../crates/bench).

## 3. Walkthrough: clicking a folder

What happens when you click a folder while a scan is still running, from the
click to the screen updating. Focus is the "scan what I am looking at first"
signal.

**On the desktop**

1. **GUI.** The navigation hooks ([`useNavigation.ts`](../src/hooks/useNavigation.ts))
   load the new view, then call `useFocus` in
   [`useFocusAndTreemap.ts`](../src/hooks/useFocusAndTreemap.ts), which calls
   `commands.focus(node, limit, metric)`.
2. **API.** `commands` is re-exported from the generated bindings by
   [`src/api/index.ts`](../src/api/index.ts); it invokes the Tauri command
   `focus`.
3. **Shell.** The `focus` function generated by `shell_commands!` in
   [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs) runs
   `Commands::focus` on the blocking pool.
4. **Host.** `impl Commands for App` in
   [`host-core/src/app/commands.rs`](../crates/host-core/src/app/commands.rs)
   (`fn focus`) resolves the `NodeRef` with `tree.focus_dir`
   ([`tree/identity.rs`](../crates/host-core/src/tree/identity.rs)) to a daemon
   `DirId`, records the view size and metric in `scan.meta`, then calls
   `scan.focus.request(FocusTarget)`. The root maps to `FocusTarget::Clear`.
5. **Debounce.** [`focus.rs`](../crates/host-core/src/focus.rs): the pure
   debouncer's `request` sends the first change at once and holds later ones
   for a window; a driver thread (`drive`) flushes the held one. It hands
   `(FocusTarget, seq)` to the sender.
6. **Client.** `focus` in
   [`client/requests.rs`](../crates/host-core/src/client/requests.rs) writes
   `ClientMsg::Focus { scan, dir, seq }` (or `ClearFocus`), defined in
   [`wire/messages/client.rs`](../crates/wire/src/messages/client.rs). Bytes
   go through the `Transport` tunnel (adb in real use).

**On the phone**

7. **Session.** [`daemon/session/dispatch.rs`](../crates/daemon/src/session/dispatch.rs)
   (`Shared::dispatch`) matches `ClientMsg::Focus`, finds the running scan by
   id and calls `focus(seq, dir)` on it.
8. **Engine entry.** [`engine/pool.rs`](../crates/engine/src/pool.rs)
   (`Scan::focus`) takes the scheduler lock and calls `set_focus` in
   [`sched/focus.rs`](../crates/engine/src/sched/focus.rs). A `seq` lower than
   one already seen is ignored.
9. **Tiers and seats.** Folders are sorted into tiers (open, near, back) by
   their relation to the focus; see `tier_of`, `targets` and `choose_tier` in
   [`sched/tiers.rs`](../crates/engine/src/sched/tiers.rs). Each tier gets a
   share of the worker seats.
10. **Pick.** An idle worker (`Scan::worker` in `pool.rs`) asks the scheduler
    to `pick` ([`sched/pick.rs`](../crates/engine/src/sched/pick.rs)), which
    chooses a tier and then a task inside it (`find_task`, `start_task`).
11. **Read.** The worker runs the task against the `Fs` trait
    ([`engine/fs.rs`](../crates/engine/src/fs.rs)): the real implementation is
    [`daemon/fs_rustix.rs`](../crates/daemon/src/fs_rustix.rs), tests use
    `SimFs`. It lists the folder and stats the entries (`stat_all`).
12. **Events.** The results go through [`engine/aggregate/`](../crates/engine/src/aggregate)
    (`listing.rs`, `stats.rs`, `complete.rs`), which emit `ScanEvent::Listing`,
    `Stats`, `SubtreeDone`, and `FocusState { seq, dir, settled }` once the
    focused folder is settled. They go into an `EventSink`
    ([`engine/sink.rs`](../crates/engine/src/sink.rs)).
13. **Outbox.** The daemon's `ScanSink`
    ([`daemon/outbox/mod.rs`](../crates/daemon/src/outbox/mod.rs)) queues them;
    [`outbox/writer.rs`](../crates/daemon/src/outbox/writer.rs) batches and
    writes them to the socket. `congested()` makes workers back off when the
    link is slow.

**Back on the desktop**

14. **Reader.** `reader_loop` in
    [`client/threads.rs`](../crates/host-core/src/client/threads.rs) decodes
    frames and routes the scan's `ServerMsg::Scan(events)` to the scan's channel.
15. **Ingest.** `ingest_loop` in
    [`app/scan/ingest.rs`](../crates/host-core/src/app/scan/ingest.rs) receives
    them and calls `apply`, which feeds each event to the tree's `ingest`
    ([`tree/ingest/mod.rs`](../crates/host-core/src/tree/ingest/mod.rs); see
    `listing.rs` and `completion.rs` beside it). Sizes roll up the ancestors in
    [`tree/sizes.rs`](../crates/host-core/src/tree/sizes.rs).
16. **Progress event.** On a timer (`progress_interval`) `ingest_loop` builds
    a `ScanProgress` and emits `HostEvent::ScanProgress`; `forward` in
    `src-tauri/src/lib.rs` sends it to the window as the `scan-progress` event.
17. **GUI update.** [`useHostEvents.ts`](../src/hooks/useHostEvents.ts)
    subscribes to the event; [`useScanProgress.ts`](../src/hooks/useScanProgress.ts)
    handles it, refreshes the live view and treemap
    ([`useLiveRefresh.ts`](../src/hooks/useLiveRefresh.ts)), and React
    re-renders the file list and treemap.

The same shape (hook, `commands.x`, `Commands::x`, a client call, a wire
message, a daemon handler) holds for delete and for starting a scan.

## 4. Where to change things

- **Add a GUI command.** Add it to the `commands!` list in
  [`gui-contract/commands.rs`](../crates/gui-contract/src/commands.rs) and any
  new types beside it. Implement it in
  [`host-core/app/commands.rs`](../crates/host-core/src/app/commands.rs). Add
  the line to `shell_commands!` in
  [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs) (a test checks the lists
  match). Regenerate bindings (below), then use it from
  [`src/api`](../src/api) and a hook.
- **Add a GUI event.** [`gui-contract/events.rs`](../crates/gui-contract/src/events.rs)
  and `event_payloads.rs`, then `HostEvent` in host-core and `forward` in
  `src-tauri/src/lib.rs`.
- **Regenerate TS bindings.**
  `cargo run -p socketsweep-gui-contract --bin export-bindings` rewrites
  `src/bindings.ts`; CI fails if it is stale.
- **Add a protocol message.** Add the variant in
  [`wire/messages`](../crates/wire/src/messages); add or update its golden
  byte test in [`wire/src/tests.rs`](../crates/wire/src/tests.rs); handle it in
  [`daemon/session/dispatch.rs`](../crates/daemon/src/session/dispatch.rs);
  send it from [`host-core/client/requests.rs`](../crates/host-core/src/client/requests.rs).
  Bump the protocol version if old peers would misread it, and handle it in
  [`sim/fake_daemon.rs`](../crates/sim/src/fake_daemon.rs).
- **Change scheduling behaviour.** Defaults (tier weights, ramp times, skeleton
  limits) are in [`wire/messages/options.rs`](../crates/wire/src/messages/options.rs)
  (`ScanOpts`, `TierCfg`); the logic is in
  [`engine/sched/tiers.rs`](../crates/engine/src/sched/tiers.rs) and
  [`pick.rs`](../crates/engine/src/sched/pick.rs). Check with the sim tests
  and the bench.
- **Change what a folder listing contains.**
  [`engine/aggregate/`](../crates/engine/src/aggregate), then `tree/ingest` on
  the host.
- **Change delete safety.** [`daemon/actions/`](../crates/daemon/src/actions)
  (the `guard/` folder) and its tests. The daemon validates; the host is not
  trusted.
- **Change how the phone is reached.** [`transport-adb`](../crates/transport-adb)
  (device discovery, tunnel) and [`host-core/daemon/`](../crates/host-core/src/daemon)
  (push and start the daemon).
- **Change a screen.** [`src/components/screens`](../src/components/screens);
  state lives in [`src/hooks`](../src/hooks), pure helpers in [`src/lib`](../src/lib).

## 5. How to run things

First time:

```bash
npm install            # or npm ci
npm run setup          # fetch pinned adb into src-tauri/bin/
npm run build:daemon   # cross-compile the phone daemon (needs ANDROID_NDK_HOME)
```

Run and build:

```bash
npm run tauri dev      # desktop app with Vite hot reload; phone plugged in
npm run dev            # GUI only, in a browser (no host behind it)
npm run build          # type-check and bundle the GUI
```

Checks:

```bash
npm run lint && npm run typecheck && npm run format:check
npm test               # vitest
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p socketsweep-sim      # one crate, e.g. the simulator tests
```

Against a real phone or emulator (build the daemon first; the options are
listed at the top of [`bench/main.rs`](../crates/bench/src/main.rs)):

```bash
cargo run --release -p socketsweep-bench -- run --serial <serial> --runs 10   # focus vs plain benchmark
cargo run --release -p socketsweep-bench -- fixture --seed 1                  # push the seeded test tree
cargo run --release -p socketsweep-bench -- e2e --seed 1                      # connect, scan, focus, delete, copy
```

Add `--serial` when more than one device is attached. The nightly emulator
version of the last two is [`e2e.yml`](../.github/workflows/e2e.yml).

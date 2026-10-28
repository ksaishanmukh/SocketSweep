<div align="center">
  <img src="assets/socket_sweep_logo.png" alt="SocketSweep Logo" width="200" />
  <h1>SocketSweep</h1>
  <p><strong>See what is using your Android storage, fast, over USB. No app to install on the phone.</strong></p>

  <img src="https://img.shields.io/badge/License-GPL_3.0-blue.svg" alt="License: GPL 3.0" />
  <img src="https://img.shields.io/badge/Tauri-v2-FFC131.svg?logo=tauri&logoColor=white" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/React-19-61DAFB.svg?logo=react&logoColor=black" alt="React" />
  <img src="https://img.shields.io/badge/Rust-stable-000000.svg?logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Android-11%2B-3DDC84.svg?logo=android&logoColor=white" alt="Android 11+" />
</div>

---

## Status

> [!IMPORTANT]
> **The v4 rebuild is in progress on this branch (`rebuild/v4`). Nothing here is released yet.**
>
> The current working app is the **v3 code on `main`**. This branch is being
> rebuilt around the architecture in [docs/architecture.md](docs/architecture.md),
> so parts of this README describe the design being built, not something you can
> run today. Performance figures in the spec are **targets**, not measurements.
>
> If you want a working build today, use the [upstream releases](https://github.com/VishnuSrivatsava/SocketSweep/releases).

## Why

Browsing a phone's storage over MTP is slow: folder sizes take minutes to
calculate, and there is no fast way to find the largest files. SocketSweep skips
MTP. It runs a small native program on the phone, reached through `adb`, and
shows the result as an interactive treemap on your desktop.

<div align="center">
  <img src="assets/ui.png" alt="Connection dashboard (v3)" width="45%" />
  <img src="assets/tree.png" alt="Treemap (v3)" width="45%" />
  <br />
  <em>v3 screenshots. The v4 interface will differ.</em>
</div>

## How it works

1. **A native daemon is pushed over adb.** On connect, the desktop pushes a small
   Rust binary to `/data/local/tmp` on the phone and starts it. Nothing is
   installed as an app, and no root is needed.
2. **An adb tunnel carries the traffic.** The daemon listens on an abstract unix
   socket, and the desktop reaches it through `adb forward`. There is no network
   port on the phone.
3. **The scan is skeleton-first.** The daemon first lists the top few levels of
   the tree, so the treemap has something to draw almost at once. Sizes (file
   stats) are filled in afterwards, and folder totals climb as they arrive.
4. **Workers move toward where you are looking.** A pool of worker threads walks
   the storage in parallel. The daemon splits them into three groups, by
   distance from the folder you have open:
   - **open folder**: the folder you are browsing;
   - **neighbours**: its siblings;
   - **background**: everything else, so the whole scan keeps moving.

   When you open a folder, the desktop tells the daemon. Workers shift towards
   it gradually as they finish their current task: a folder you only pass through
   gets little help, and one you stay in gets more. Nothing is interrupted
   part-way, and no worker sits idle while there is work left.
5. **Delete and copy.** Delete runs on the phone, inside the scanned root, and the
   scan updates its totals as it goes. Copy-to-desktop gets a file list from the
   daemon and pulls the bytes with adb sync, over a few parallel connections.

Supported phones: Android 11 (API 30) or newer. The daemon is built for 64-bit
ARM (phones) and x86_64 (the emulator, used for end-to-end tests).

## Architecture

Four layers. Each talks only to its neighbour, through a contract crate, and each
contract has a fake implementation so both sides can be tested on their own.
The full specification, including the wire protocol, the scheduler and the
acceptance criteria, is in **[docs/architecture.md](docs/architecture.md)**.
To find your way around the code, start with the **[code tour](docs/code-tour.md)**.

```
React GUI ──► src-tauri (thin shell) ──► host-core ──► transport-adb ──► phone daemon
```

| Boundary | Contract | Fake |
|---|---|---|
| GUI and host | `crates/gui-contract`: command and event types, with generated TypeScript bindings | `src/api` mock in vitest |
| Host and transport | `crates/transport`: the `Transport` and `Duplex` traits | `FakeTransport` |
| Host and phone | `crates/wire`: protocol v4, checked by golden byte tests | `FakeDaemon` (in `crates/sim`) |
| Daemon internals | The `Fs` and `EventSink` traits; the scheduler is a pure state machine | `SimFs`, a recording sink |

### Workspace layout (target)

| Path | Role |
|---|---|
| `crates/wire` | Host and phone protocol types, framing, handshake |
| `crates/gui-contract` | GUI and host command and event types |
| `crates/transport` | `Transport` trait and `FakeTransport` |
| `crates/transport-adb` | adb implementation: smart sockets, shell v2, sync, USB and wireless |
| `crates/host-core` | Tree index, session client, focus debounce, delete and copy orchestration |
| `crates/daemon` | Phone binary: listener, session, scheduler, aggregation, worker pool, filesystem walk, actions |
| `crates/sim` | Dev only: simulated filesystem, simulator, baseline walker, `FakeDaemon` |
| `src-tauri` | Thin shell that maps generated commands to `host-core` |
| `src/` | React features, importing only the generated bindings through `src/api/` |
| `crates/bench` | Benchmark and end-to-end runner against a phone or emulator |

The benchmark baseline is the engine's plain mode (no skeleton, no deferred
stats, no focus), so both sides of the comparison share one code path.

## Security

The daemon runs on your phone and can delete files, so how it is reached and what
it will act on both matter.

- **Abstract socket, no network port.** The daemon binds an abstract unix socket
  and the desktop reaches it through `adb forward`. An installed app cannot
  connect to a socket owned by the `shell` SELinux domain, which loopback TCP
  would not prevent.
- **Token handshake.** The desktop generates 32 random bytes per session and
  gives them to the daemon through the `SOCKETSWEEP_TOKEN` environment variable,
  not on the command line. Every connection begins with a `Hello` carrying the
  token and protocol version. The daemon compares the token in constant time and
  answers a wrong one with a bare refusal.
- **Delete guard.** The daemon validates every delete itself. The target must lie
  inside the scan root. Symlinks are unlinked and never followed: the parent
  directory is resolved and the final path component is removed as it sits.
- **Reuses your adb server.** The host talks to an already-running adb server
  instead of restarting it.

None of the v4 security design has been verified on hardware yet.

## Using the app

1. Enable **USB debugging** on the phone: Settings, About phone, tap Build number
   seven times, then Settings, Developer options, USB debugging.
2. Connect the phone with a USB cable and open SocketSweep.
3. Click **Connect**, then **Scan**. Click blocks in the treemap to drill down.
   Delete from the app, or copy a folder to your desktop.

Troubleshooting:

- **0 files or missing folders on Android 11+:** scoped storage restricts access.
  SocketSweep tries `adb shell appops set com.android.shell MANAGE_EXTERNAL_STORAGE allow`
  itself. Some phone makers need an extra toggle, for example Xiaomi's "USB
  debugging (Security settings)".
- **Samsung, USB debugging greyed out:** turn off Settings, Security, Auto Blocker.

## Building from source

Prerequisites:

- Node.js 22.22+ or 24.15+
- Rust (stable) with Cargo
- Android NDK, only to build the phone daemon (targets `aarch64-linux-android`
  and `x86_64-linux-android`). Point `ANDROID_NDK_HOME` at it.

You do not need adb on your `PATH`; the setup script fetches a pinned copy.

```bash
npm install
npm run setup           # downloads pinned platform-tools, verifies its SHA-256, puts adb in src-tauri/bin/
npm run build:daemon    # cross-compiles the phone daemon (needs the NDK)
npm run tauri dev       # run the desktop app with a phone plugged in
```

`npm run build:helper` compiles the on-phone app-info helper (`android-helper/`,
a few Java files that report installed apps' names and icons) to
`src-tauri/bin/socketsweep-helper.dex`. It needs a JDK 17+ and an Android SDK
with a platform (API 30+) and build-tools, found through `ANDROID_HOME` or
`ANDROID_SDK_ROOT`. Instead of building either the daemon or the helper, you
can download them from the `daemon` and `helper` artifacts of a CI run into
`src-tauri/bin`.

`npm run build:daemon` writes gzip-compressed `src-tauri/bin/daemon-arm64.gz` and
`src-tauri/bin/daemon-x86_64.gz`; both are bundled with the app (compressed so
the Linux AppImage tooling does not try to resolve an Android ELF) and
decompressed on the host before being pushed to the phone.

Checks:

```bash
npm run lint && npm run typecheck && npm test
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Benchmark and end-to-end test on a phone or emulator (`crates/bench`; build the
daemon first, it is picked from `src-tauri/bin` by the device's ABI):

```bash
# The engine's focus mode against its plain mode (the baseline), scripts a/b/c,
# median and p90 table, results in socketsweep-bench.json
cargo run --release -p socketsweep-bench -- run --serial <serial> --runs 10
# The seeded fixture (50,000 files, 3,000 folders) pushed to /sdcard/ssfixture
cargo run --release -p socketsweep-bench -- fixture --seed 1
# Connect, scan, focus, delete, copy and disconnect through host-core, on that fixture
cargo run --release -p socketsweep-bench -- e2e --seed 1
```

The `Emulator end-to-end` workflow runs the last two (and a three-run benchmark)
on an x86_64 emulator nightly; start it by hand from the Actions tab.

## Origins and credit

SocketSweep was created by **[VishnuSrivatsava](https://github.com/VishnuSrivatsava/SocketSweep)**.
This fork started from that project and is being rebuilt with the architecture
above. The idea of pushing a native binary over adb and talking to it through a
local socket was inspired by [scrcpy](https://github.com/Genymobile/scrcpy).

If SocketSweep is useful to you, please support the original author:

<div align="center">
  <a href="https://github.com/sponsors/VishnuSrivatsava"><img src="https://img.shields.io/badge/♥_Sponsor_the_original_author-EA4AAA?style=for-the-badge&logo=github-sponsors&logoColor=white" alt="Sponsor VishnuSrivatsava" /></a>
</div>

## License

GNU General Public License v3.0, the same as upstream. See [LICENSE](LICENSE).

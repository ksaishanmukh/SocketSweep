<div align="center">
  <img src="src-tauri/icons/128x128@2x.png" alt="SocketSweep icon" width="128" />
  <h1>SocketSweep</h1>
  <p><strong>See what is using your Android storage, fast, over USB. No app to install on the phone.</strong></p>

  <img src="https://img.shields.io/badge/License-GPL_3.0-blue.svg" alt="License: GPL 3.0" />
  <img src="https://img.shields.io/badge/Tauri-v2-FFC131.svg?logo=tauri&logoColor=white" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/React-19-61DAFB.svg?logo=react&logoColor=black" alt="React" />
  <img src="https://img.shields.io/badge/Rust-stable-000000.svg?logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Android-11%2B-3DDC84.svg?logo=android&logoColor=white" alt="Android 11+" />
</div>

---

## Download

Get the installer for Windows, macOS or Linux from the
[releases page](https://github.com/ksaishanmukh/SocketSweep/releases).
The builds are not code-signed, so Windows SmartScreen and macOS Gatekeeper
will ask you to confirm the first launch.

## Why

Browsing a phone's storage over MTP is slow: folder sizes take minutes to
calculate, and there is no fast way to find the largest files. SocketSweep skips
MTP. It runs a small native program on the phone, reached through `adb`, and
reads the whole of shared storage in well under a second once the phone's file
cache is warm (see [Benchmarks](#benchmarks)).

## What you get

- **Overview.** Space by category (video, photos, audio, documents, archives,
  other), the biggest files, space by app with the app's real name and icon,
  and what changed since the last scan. Leftover folders of uninstalled apps
  are flagged.
- **Explorer.** A sortable list of the current folder with a resizable treemap
  strip above it. Breadcrumbs with back and forward, search within the folder,
  and filters by category, files or folders, and when they last changed.
- **Multi-select actions.** Tick several files or folders, then delete them on
  the phone or copy them to your computer.
- **Live from the first moment.** The folder tree appears almost at once and
  sizes fill in as the scan runs, with the folder you are looking at scanned
  first.
- **Calm, readable theme** that follows your system's light or dark setting.

## How it works

1. **A native daemon is pushed over adb.** On connect, the desktop pushes a small
   Rust binary to `/data/local/tmp` on the phone and starts it. Nothing is
   installed as an app, and no root is needed.
2. **An adb tunnel carries the traffic.** The daemon listens on an abstract unix
   socket, and the desktop reaches it through adb. There is no network port on
   the phone.
3. **The scan is skeleton-first.** The daemon first lists the top levels of the
   tree, so there is something to show almost at once. Sizes (file stats) are
   filled in afterwards, and folder totals climb as they arrive.
4. **Workers move toward where you are looking.** A pool of worker threads walks
   the storage in parallel. When you open a folder, the desktop tells the
   daemon, and each worker that finishes its current task takes work from that
   folder next, except for a quarter of them (at least one), which keep the
   rest of the scan moving. Nothing is interrupted part-way, and no worker sits
   idle while there is work left: once the open folder is measured, every
   worker goes back to the rest.
5. **App names and icons come from a tiny helper.** A few Java classes, run with
   `app_process` as the adb shell user, list installed apps with their labels
   and icons, so `Android/data/com.whatsapp` shows as WhatsApp.
6. **Delete and copy.** Delete runs on the phone, inside the scanned root, and the
   scan updates its totals as it goes. Copy-to-desktop gets a file list from the
   daemon and pulls the bytes with adb sync, over a few parallel connections.

Supported phones: Android 11 (API 30) or newer. The daemon is built for 64-bit
ARM (phones) and x86_64 (the emulator, used for end-to-end tests).

## Benchmarks

Measured with `socketsweep-bench` (below) on a OnePlus CPH2649, Android 17
(API 37), scanning all of shared storage (`/sdcard`): **110,068 files in
14,041 folders, 83.2 GB**, with the default 8 workers.

| Scan | Whole tree sized | First folder listing |
|---|---|---|
| Cold: first scan after a reboot | 4.9 s | 105 ms |
| Warm: phone's file cache populated | **0.60 s** median, 0.67 s p90 | **45 ms** |

The cold run is a single measurement after a reboot (108,136 files at the
time); only a reboot empties the phone's cache without root. The warm rows are
the median and 90th percentile of 5 runs.

**Focus costs nothing measurable.** Steering workers towards the open folder is
compared against the engine's plain mode (no skeleton, no focus), which serves
as the baseline:

| Script | Focus vs plain, total time (median) | Directory opens |
|---|---|---|
| a: scan only | 1.018× | 1.002× |
| b: open Music at 300 ms, a subfolder at 2 s | 1.021× | 1.002× |
| c: jump between folders | 0.997× | 1.002× |

On a warm cache the whole tree is sized before the first scripted click, so
focus has nothing to speed up there. Its benefit shows on cold scans: in one
cold-ish run, Music settled 36 ms after it was opened with focus, against 87 ms
without it.

## Architecture

Four layers. Each talks only to its neighbour, through a contract crate, and each
contract has a fake implementation so both sides can be tested on their own.
The full specification, including the wire protocol, the scheduler and the
acceptance criteria, is in **[docs/architecture.md](docs/architecture.md)**.
To find your way around the code, start with the **[code tour](docs/code-tour.md)**.

```
React GUI ──► src-tauri (thin shell) ──► host-core ──► transport-adb ──► phone daemon
                                                                     └─► app helper
```

| Boundary | Contract | Fake |
|---|---|---|
| GUI and host | `src-tauri/src/commands.rs`: each command declared once, with the types in `crates/gui-contract`; TypeScript bindings generated by tauri-specta | `src/api` mock in vitest |
| Host and transport | `crates/transport`: the `Transport` and `Duplex` traits | `FakeTransport` |
| Host and phone | `crates/wire`: protocol v7; the app helper's line format | `FakeDaemon` (in `crates/sim`) |
| Daemon internals | The `Fs` and `EventSink` traits; the scheduler is a pure state machine | `SimFs`, a recording sink |

### Workspace layout

| Path | Role |
|---|---|
| `crates/wire` | Host and phone protocol types, framing, handshake |
| `crates/gui-contract` | GUI and host command and event types |
| `crates/transport` | `Transport` trait and `FakeTransport` |
| `crates/transport-adb` | adb implementation: smart sockets, shell v2, sync, USB and wireless |
| `crates/host-core` | Tree index, session client, focus debounce, overview, filters, delete and copy orchestration |
| `crates/engine` | The scan scheduler and aggregation, pure and shared by the daemon and the tests |
| `crates/daemon` | Phone binary: listener, session, worker pool, filesystem walk, actions |
| `android-helper` | Java helper reporting installed apps' names and icons |
| `crates/sim` | Dev only: simulated filesystem, G1–G3 checker, `FakeDaemon` |
| `crates/bench` | Benchmark and end-to-end runner against a phone or emulator |
| `src-tauri` | Thin shell that maps generated commands to `host-core` |
| `src/` | React features, importing only the generated bindings through `src/api/` |

## Security

The daemon runs on your phone and can delete files, so how it is reached and what
it will act on both matter.

- **Abstract socket, no network port.** The daemon binds an abstract unix socket
  and the desktop reaches it through adb. An installed app cannot connect to a
  socket owned by the `shell` SELinux domain, which loopback TCP would not
  prevent.
- **Token handshake.** The desktop generates 32 random bytes per session and
  gives them to the daemon through the `SOCKETSWEEP_TOKEN` environment variable,
  not on the command line. Every connection begins with a `Hello` carrying the
  token and protocol version. The daemon compares the token in constant time and
  answers a wrong token or version with a bare refusal.
- **Delete guard.** The daemon validates every delete itself. The target must lie
  inside the scan root. Symlinks are unlinked and never followed: the parent
  directory is resolved and the final path component is removed as it sits.
- **Reuses your adb server.** The host talks to an already-running adb server
  instead of restarting it.
- **The helper only reads.** It lists apps and renders icons for the current
  user; it changes nothing on the phone.

## Using the app

1. Enable **USB debugging** on the phone: Settings, About phone, tap Build number
   seven times, then Settings, Developer options, USB debugging.
2. Connect the phone with a USB cable, open SocketSweep, and tap **Allow** on the
   phone when it asks.
3. Click **Scan this phone**. Start from the Overview, or open the Explorer to
   browse folders, sort, filter and search. Tick items to delete them or copy
   them to your computer.

Troubleshooting:

- **0 files or missing folders on Android 11+:** scoped storage restricts access.
  SocketSweep tries `adb shell appops set com.android.shell MANAGE_EXTERNAL_STORAGE allow`
  itself. Some phone makers need an extra toggle, for example Xiaomi's "USB
  debugging (Security settings)".
- **Samsung, USB debugging greyed out:** turn off Settings, Security, Auto Blocker.
- **Apps show as package names:** the app helper could not run (for example on a
  work profile). Sizes are unaffected.

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
npm run build:helper    # compiles the app helper (needs a JDK 17+ and the Android SDK)
npm run tauri dev       # run the desktop app with a phone plugged in
```

`npm run build:helper` writes `src-tauri/bin/socketsweep-helper.dex`. It needs
an Android SDK with a platform (API 30+) and build-tools, found through
`ANDROID_HOME` or `ANDROID_SDK_ROOT`. Instead of building either the daemon or
the helper, you can download them from the `daemon` and `helper` artifacts of a
CI run into `src-tauri/bin`.

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
# median and p90 table, results in socketsweep-bench.json. Read-only.
cargo run --release -p socketsweep-bench -- run --serial <serial> --runs 5
# The seeded fixture (50,000 files, 3,000 folders) pushed to /sdcard/ssfixture
cargo run --release -p socketsweep-bench -- fixture --seed 1
# Connect, scan, focus, delete, copy and disconnect through host-core, on that fixture
cargo run --release -p socketsweep-bench -- e2e --seed 1
```

### CI

CI runs only when you push a tag, so pushes and pull requests (documentation
included) run nothing. The prefix picks the job:

| Tag | Runs |
|---|---|
| `check-<anything>` | The fast gate: formatting, clippy, Rust and GUI tests, lint (about 2 min) |
| `build-<anything>` | The gate, the phone daemon and app helper, and Windows, macOS and Linux bundles, uploaded as artifacts |
| `e2e-<anything>` | The emulator end-to-end test and a three-run benchmark on an x86_64 emulator |
| `v<version>` | A full build, with the version taken from the tag, attached to a draft release |

```bash
git tag check-my-change && git push origin check-my-change
git push origin :refs/tags/check-my-change   # delete it again afterwards
```

Both workflows can also be started by hand from the Actions tab.

## Origins and credit

SocketSweep was created by **[VishnuSrivatsava](https://github.com/VishnuSrivatsava/SocketSweep)**.
This fork started from that project and has been rebuilt with the architecture
above. The idea of pushing a native binary over adb and talking to it through a
local socket was inspired by [scrcpy](https://github.com/Genymobile/scrcpy).

If SocketSweep is useful to you, please support the original author:

<div align="center">
  <a href="https://github.com/sponsors/VishnuSrivatsava"><img src="https://img.shields.io/badge/♥_Sponsor_the_original_author-EA4AAA?style=for-the-badge&logo=github-sponsors&logoColor=white" alt="Sponsor VishnuSrivatsava" /></a>
</div>

## License

GNU General Public License v3.0, the same as upstream. See [LICENSE](LICENSE).

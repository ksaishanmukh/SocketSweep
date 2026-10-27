//! An in-memory phone, for testing everything above the [`Transport`] trait.
//!
//! Enabled by the `fake` feature (and always in this crate's own tests).
//!
//! [`FakeTransport`] holds any number of [`FakeDevice`]s, each with:
//!
//! - **scripted shell responses**: handlers tried newest first, so a test can
//!   override a default; a command no handler answers exits 127 like `sh`
//!   does for an unknown command. Every command is logged.
//! - **a virtual file store** that [`Transport::push`] writes and
//!   [`Transport::pull`] reads, keyed by exact path bytes.
//! - **tunnel handlers**: [`Transport::open_tunnel`] creates an in-memory
//!   [`pipe`], returns one end and hands the other to the handler registered
//!   for that socket name, on a thread of its own. That is where a fake daemon
//!   plugs in. With no handler, the tunnel reads end of stream at once, as an
//!   adb forward to a socket nothing listens on does.
//!
//! [`FakeTransport::fail_next`] injects one failure into the next call of a
//! given kind.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::{Device, DeviceInfo, DeviceState, Duplex, Output, Result, Transport, TransportError};

/// Lock a mutex, recovering the data if a thread panicked while holding it.
/// A panicking test handler should fail that test, not every later call.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ── In-memory duplex pipe ───────────────────────────────────────────────────

/// Bytes buffered in each direction of a [`pipe`] before writes block.
pub const DEFAULT_PIPE_CAPACITY: usize = 1024 * 1024;

/// One direction of a pipe.
struct Channel {
    state: Mutex<ChannelState>,
    changed: Condvar,
}

struct ChannelState {
    buf: VecDeque<u8>,
    capacity: usize,
    /// The writing end shut down or went away: reads drain, then see EOF.
    writer_closed: bool,
    /// The reading end shut down or went away: writes fail.
    reader_closed: bool,
}

impl Channel {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Channel {
            state: Mutex::new(ChannelState {
                buf: VecDeque::new(),
                capacity: capacity.max(1),
                writer_closed: false,
                reader_closed: false,
            }),
            changed: Condvar::new(),
        })
    }

    /// Wait for a change, until `deadline` if there is one. Returns the guard
    /// and whether the deadline passed.
    fn wait<'a>(
        &self,
        guard: MutexGuard<'a, ChannelState>,
        deadline: Option<Instant>,
    ) -> (MutexGuard<'a, ChannelState>, bool) {
        match deadline {
            None => (
                self.changed.wait(guard).unwrap_or_else(|e| e.into_inner()),
                false,
            ),
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return (guard, true);
                }
                let (guard, _) = self
                    .changed
                    .wait_timeout(guard, deadline - now)
                    .unwrap_or_else(|e| e.into_inner());
                (guard, false)
            }
        }
    }
}

/// What every handle of one end shares.
struct EndShared {
    rx: Arc<Channel>,
    tx: Arc<Channel>,
    read_timeout: Mutex<Option<Duration>>,
    write_timeout: Mutex<Option<Duration>>,
}

impl EndShared {
    fn close(&self) {
        {
            let mut tx = lock(&self.tx.state);
            tx.writer_closed = true;
        }
        self.tx.changed.notify_all();
        {
            let mut rx = lock(&self.rx.state);
            rx.reader_closed = true;
        }
        self.rx.changed.notify_all();
    }
}

impl Drop for EndShared {
    fn drop(&mut self) {
        self.close();
    }
}

/// One end of an in-memory [`pipe`]. Behaves like a connected socket: reads
/// block until data arrives, return end of stream once the other end has
/// closed and everything it sent has been read, and time out with
/// [`io::ErrorKind::TimedOut`]; writes block while the other direction's
/// buffer is full and fail with [`io::ErrorKind::BrokenPipe`] once the other
/// end has gone.
pub struct PipeEnd(Arc<EndShared>);

/// A connected pair of in-memory [`Duplex`] streams with
/// [`DEFAULT_PIPE_CAPACITY`] buffered each way.
pub fn pipe() -> (PipeEnd, PipeEnd) {
    pipe_with_capacity(DEFAULT_PIPE_CAPACITY)
}

/// As [`pipe`], buffering at most `capacity` bytes each way, so tests can
/// exercise back-pressure.
pub fn pipe_with_capacity(capacity: usize) -> (PipeEnd, PipeEnd) {
    let a_to_b = Channel::new(capacity);
    let b_to_a = Channel::new(capacity);
    let end = |rx: &Arc<Channel>, tx: &Arc<Channel>| {
        PipeEnd(Arc::new(EndShared {
            rx: Arc::clone(rx),
            tx: Arc::clone(tx),
            read_timeout: Mutex::new(None),
            write_timeout: Mutex::new(None),
        }))
    };
    (end(&b_to_a, &a_to_b), end(&a_to_b, &b_to_a))
}

fn deadline(timeout: &Mutex<Option<Duration>>) -> Option<Instant> {
    lock(timeout).map(|t| Instant::now() + t)
}

impl Read for PipeEnd {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let deadline = deadline(&self.0.read_timeout);
        let channel = &self.0.rx;
        let mut state = lock(&channel.state);
        loop {
            if !state.buf.is_empty() {
                let n = out.len().min(state.buf.len());
                for (slot, byte) in out.iter_mut().zip(state.buf.drain(..n)) {
                    *slot = byte;
                }
                drop(state);
                channel.changed.notify_all();
                return Ok(n);
            }
            if state.writer_closed || state.reader_closed {
                return Ok(0);
            }
            let (next, timed_out) = channel.wait(state, deadline);
            if timed_out {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pipe read timed out",
                ));
            }
            state = next;
        }
    }
}

impl Write for PipeEnd {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let deadline = deadline(&self.0.write_timeout);
        let channel = &self.0.tx;
        let mut state = lock(&channel.state);
        loop {
            if state.reader_closed || state.writer_closed {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "the other end of the pipe is closed",
                ));
            }
            let space = state.capacity - state.buf.len();
            if space > 0 {
                let n = space.min(data.len());
                state.buf.extend(&data[..n]);
                drop(state);
                channel.changed.notify_all();
                return Ok(n);
            }
            let (next, timed_out) = channel.wait(state, deadline);
            if timed_out {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pipe write timed out",
                ));
            }
            state = next;
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Duplex for PipeEnd {
    fn shutdown(&self) -> io::Result<()> {
        self.0.close();
        Ok(())
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *lock(&self.0.read_timeout) = timeout;
        Ok(())
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *lock(&self.0.write_timeout) = timeout;
        Ok(())
    }

    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(PipeEnd(Arc::clone(&self.0))))
    }
}

// ── Fake transport ──────────────────────────────────────────────────────────

/// A device to add to a [`FakeTransport`].
#[derive(Debug, Clone)]
pub struct FakeDevice {
    pub serial: String,
    pub model: String,
    pub state: DeviceState,
    pub hint: Option<String>,
    pub info: DeviceInfo,
}

impl FakeDevice {
    /// A ready, authorised arm64 phone on Android 14.
    pub fn ready(serial: &str) -> Self {
        FakeDevice {
            serial: serial.into(),
            model: "Fake Phone".into(),
            state: DeviceState::Ready,
            hint: None,
            info: DeviceInfo {
                sdk: 34,
                abi: "arm64-v8a".into(),
                model: "Fake Phone".into(),
            },
        }
    }
}

/// A file in a fake device's store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeFile {
    pub data: Vec<u8>,
    pub mode: u32,
}

/// The kinds of call [`FakeTransport::fail_next`] can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FakeOp {
    Devices,
    DeviceInfo,
    Shell,
    Push,
    Pull,
    OpenTunnel,
}

type ShellHandler = Arc<dyn Fn(&str) -> Option<Output> + Send + Sync>;
type TunnelHandler = Arc<dyn Fn(PipeEnd) + Send + Sync>;

struct DeviceSlot {
    device: FakeDevice,
    shell: Vec<ShellHandler>,
    shell_log: Vec<String>,
    files: BTreeMap<Vec<u8>, FakeFile>,
    tunnels: HashMap<String, TunnelHandler>,
    tunnels_opened: HashMap<String, usize>,
}

#[derive(Default)]
struct FakeState {
    devices: Vec<DeviceSlot>,
    faults: Vec<(FakeOp, TransportError)>,
}

impl FakeState {
    fn slot(&mut self, serial: &str) -> Option<&mut DeviceSlot> {
        self.devices.iter_mut().find(|d| d.device.serial == serial)
    }

    /// The device, if it is attached and usable, as the real transport
    /// requires for every per-device call.
    fn usable(&mut self, serial: &str) -> Result<&mut DeviceSlot> {
        let slot = self.slot(serial).ok_or_else(|| TransportError::NotFound {
            serial: serial.into(),
        })?;
        if slot.device.state != DeviceState::Ready {
            return Err(TransportError::Unusable {
                serial: serial.into(),
                state: slot.device.state.clone(),
                hint: slot.device.hint.clone(),
            });
        }
        Ok(slot)
    }

    fn take_fault(&mut self, op: FakeOp) -> Result<()> {
        match self.faults.iter().position(|(o, _)| *o == op) {
            Some(i) => Err(self.faults.remove(i).1),
            None => Ok(()),
        }
    }
}

/// An in-memory [`Transport`]. See the [module documentation](self).
///
/// Every method that configures a device panics if the serial was never
/// added: that is a mistake in the test, not a condition under test.
#[derive(Default)]
pub struct FakeTransport {
    state: Mutex<FakeState>,
}

impl FakeTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`FakeTransport::add_device`].
    pub fn with_device(self, device: FakeDevice) -> Self {
        self.add_device(device);
        self
    }

    /// Attach a device, replacing any with the same serial.
    pub fn add_device(&self, device: FakeDevice) {
        let mut state = lock(&self.state);
        state.devices.retain(|d| d.device.serial != device.serial);
        state.devices.push(DeviceSlot {
            device,
            shell: Vec::new(),
            shell_log: Vec::new(),
            files: BTreeMap::new(),
            tunnels: HashMap::new(),
            tunnels_opened: HashMap::new(),
        });
    }

    /// Unplug a device, forgetting everything about it.
    pub fn remove_device(&self, serial: &str) {
        lock(&self.state)
            .devices
            .retain(|d| d.device.serial != serial);
    }

    /// Change a device's state, e.g. to simulate the user revoking
    /// authorisation.
    pub fn set_state(&self, serial: &str, new: DeviceState) {
        self.with_slot(serial, |slot| slot.device.state = new);
    }

    /// Answer shell commands with `handler`, which returns `None` for
    /// commands it does not handle. Handlers are tried newest first. A
    /// handler runs without any lock held, so it may call back into the
    /// transport.
    pub fn on_shell(
        &self,
        serial: &str,
        handler: impl Fn(&str) -> Option<Output> + Send + Sync + 'static,
    ) {
        self.with_slot(serial, |slot| slot.shell.push(Arc::new(handler)));
    }

    /// Answer every command that starts with `prefix` with `output`.
    pub fn respond(&self, serial: &str, prefix: &str, output: Output) {
        let prefix = prefix.to_owned();
        self.on_shell(serial, move |command| {
            command.starts_with(&prefix).then(|| output.clone())
        });
    }

    /// Every shell command run on the device, in order.
    pub fn shell_log(&self, serial: &str) -> Vec<String> {
        self.with_slot(serial, |slot| slot.shell_log.clone())
    }

    /// Put a file in the device's store, as if it were on the phone.
    pub fn put_file(&self, serial: &str, remote: &[u8], data: Vec<u8>, mode: u32) {
        self.with_slot(serial, |slot| {
            slot.files.insert(remote.to_vec(), FakeFile { data, mode });
        });
    }

    /// A file in the device's store, if there is one at `remote`.
    pub fn file(&self, serial: &str, remote: &[u8]) -> Option<FakeFile> {
        self.with_slot(serial, |slot| slot.files.get(remote).cloned())
    }

    /// Serve tunnels to the abstract socket `name` with `handler`, which is
    /// given the daemon's end of each new tunnel on a thread of its own.
    pub fn on_tunnel(
        &self,
        serial: &str,
        name: &str,
        handler: impl Fn(PipeEnd) + Send + Sync + 'static,
    ) {
        self.with_slot(serial, |slot| {
            slot.tunnels.insert(name.into(), Arc::new(handler));
        });
    }

    /// How many tunnels to `name` have been opened.
    pub fn tunnels_opened(&self, serial: &str, name: &str) -> usize {
        self.with_slot(serial, |slot| {
            slot.tunnels_opened.get(name).copied().unwrap_or(0)
        })
    }

    /// Make the next call of kind `op`, on any device, fail with `error`.
    /// Several can be queued; each is used once.
    pub fn fail_next(&self, op: FakeOp, error: TransportError) {
        lock(&self.state).faults.push((op, error));
    }

    fn with_slot<T>(&self, serial: &str, f: impl FnOnce(&mut DeviceSlot) -> T) -> T {
        let mut state = lock(&self.state);
        let slot = state
            .slot(serial)
            .unwrap_or_else(|| panic!("no fake device {serial} was added"));
        f(slot)
    }
}

/// Chunk size [`Transport::pull`] reports progress at, like adb's sync `DATA`.
const PULL_CHUNK: usize = 64 * 1024;

impl Transport for FakeTransport {
    fn devices(&self) -> Result<Vec<Device>> {
        let mut state = lock(&self.state);
        state.take_fault(FakeOp::Devices)?;
        Ok(state
            .devices
            .iter()
            .map(|slot| Device {
                serial: slot.device.serial.clone(),
                model: slot.device.model.clone(),
                state: slot.device.state.clone(),
                hint: slot.device.hint.clone(),
            })
            .collect())
    }

    fn device_info(&self, serial: &str) -> Result<DeviceInfo> {
        let mut state = lock(&self.state);
        state.take_fault(FakeOp::DeviceInfo)?;
        Ok(state.usable(serial)?.device.info.clone())
    }

    fn shell(&self, serial: &str, command: &str) -> Result<Output> {
        let handlers = {
            let mut state = lock(&self.state);
            state.take_fault(FakeOp::Shell)?;
            let slot = state.usable(serial)?;
            slot.shell_log.push(command.into());
            slot.shell.clone()
        };
        for handler in handlers.iter().rev() {
            if let Some(output) = handler(command) {
                return Ok(output);
            }
        }
        let program = command.split_whitespace().next().unwrap_or("");
        Ok(Output {
            stdout: Vec::new(),
            stderr: format!("/system/bin/sh: {program}: inaccessible or not found\n").into_bytes(),
            exit: Some(127),
        })
    }

    fn push(&self, serial: &str, local: &Path, remote: &str, mode: u32) -> Result<()> {
        lock(&self.state).take_fault(FakeOp::Push)?;
        let data = std::fs::read(local).map_err(|source| TransportError::Local {
            path: local.to_path_buf(),
            source,
        })?;
        let mut state = lock(&self.state);
        let slot = state.usable(serial)?;
        slot.files
            .insert(remote.as_bytes().to_vec(), FakeFile { data, mode });
        Ok(())
    }

    fn pull(
        &self,
        serial: &str,
        remote: &[u8],
        local: &Path,
        progress: &mut dyn FnMut(u64),
    ) -> Result<u64> {
        let data = {
            let mut state = lock(&self.state);
            state.take_fault(FakeOp::Pull)?;
            let slot = state.usable(serial)?;
            match slot.files.get(remote) {
                Some(file) => file.data.clone(),
                None => {
                    return Err(TransportError::Refused(format!(
                        "remote object '{}' does not exist",
                        String::from_utf8_lossy(remote)
                    )))
                }
            }
        };

        let local_err = |source| TransportError::Local {
            path: local.to_path_buf(),
            source,
        };
        let mut file = std::fs::File::create(local).map_err(local_err)?;
        let mut done = 0u64;
        for chunk in data.chunks(PULL_CHUNK) {
            file.write_all(chunk).map_err(local_err)?;
            done += chunk.len() as u64;
            progress(done);
        }
        file.flush().map_err(local_err)?;
        Ok(done)
    }

    fn open_tunnel(&self, serial: &str, name: &str) -> Result<Box<dyn Duplex>> {
        let handler = {
            let mut state = lock(&self.state);
            state.take_fault(FakeOp::OpenTunnel)?;
            let slot = state.usable(serial)?;
            *slot.tunnels_opened.entry(name.into()).or_insert(0) += 1;
            slot.tunnels.get(name).cloned()
        };

        let (near, far) = pipe();
        match handler {
            Some(handler) => {
                std::thread::Builder::new()
                    .name(format!("fake-tunnel-{name}"))
                    .spawn(move || handler(far))
                    .map_err(TransportError::Io)?;
            }
            // Nothing listening: the far end closes at once.
            None => drop(far),
        }
        Ok(Box::new(near))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    /// A fresh directory for one test's local files.
    fn scratch(test: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "socketsweep-transport-{}-{test}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn phone() -> FakeTransport {
        FakeTransport::new().with_device(FakeDevice::ready("P1"))
    }

    #[test]
    fn it_is_usable_as_a_shared_trait_object() {
        let t: Arc<dyn Transport> = Arc::new(phone());
        let t2 = Arc::clone(&t);
        std::thread::spawn(move || t2.devices().unwrap())
            .join()
            .unwrap();
        assert_eq!(t.devices().unwrap().len(), 1);
    }

    #[test]
    fn devices_are_listed_whatever_their_state() {
        let t = phone().with_device(FakeDevice {
            state: DeviceState::Unauthorized,
            ..FakeDevice::ready("P2")
        });
        let devices = t.devices().unwrap();
        assert_eq!(devices.len(), 2);
        assert!(devices[0].usable());
        assert_eq!(devices[1].state, DeviceState::Unauthorized);
        assert!(!devices[1].usable());

        t.remove_device("P1");
        assert_eq!(t.devices().unwrap().len(), 1);
    }

    #[test]
    fn calls_to_missing_or_unusable_devices_fail() {
        let t = phone();
        assert!(matches!(
            t.shell("nope", "true"),
            Err(TransportError::NotFound { .. })
        ));

        t.set_state("P1", DeviceState::NoPermission);
        match t.device_info("P1") {
            Err(TransportError::Unusable { state, .. }) => {
                assert_eq!(state, DeviceState::NoPermission)
            }
            other => panic!("expected Unusable, got {other:?}"),
        }
        t.set_state("P1", DeviceState::Ready);
        assert_eq!(t.device_info("P1").unwrap().abi, "arm64-v8a");
    }

    #[test]
    fn shell_commands_are_scripted_logged_and_newest_first() {
        let t = phone();
        let ok = |s: &str| Output {
            stdout: s.as_bytes().to_vec(),
            stderr: Vec::new(),
            exit: Some(0),
        };
        t.respond("P1", "getprop", ok("34\n"));
        assert_eq!(
            t.shell("P1", "getprop ro.build.version.sdk")
                .unwrap()
                .stdout_text(),
            "34\n"
        );

        // A newer handler overrides an older one.
        t.respond("P1", "getprop", ok("35\n"));
        assert_eq!(t.shell("P1", "getprop x").unwrap().stdout_text(), "35\n");

        // Unscripted commands fail the way sh does.
        let out = t.shell("P1", "frobnicate --now").unwrap();
        assert_eq!(out.exit, Some(127));
        assert!(!out.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("frobnicate"));

        assert_eq!(
            t.shell_log("P1"),
            [
                "getprop ro.build.version.sdk",
                "getprop x",
                "frobnicate --now"
            ]
        );
    }

    #[test]
    fn a_shell_handler_may_call_back_into_the_transport() {
        let t = Arc::new(phone());
        let weak = Arc::downgrade(&t);
        t.on_shell("P1", move |command| {
            let t = weak.upgrade()?;
            let path = command.strip_prefix("rm ")?;
            let existed = t.file("P1", path.as_bytes()).is_some();
            t.put_file("P1", path.as_bytes(), Vec::new(), 0);
            Some(Output {
                exit: Some(if existed { 0 } else { 1 }),
                ..Output::default()
            })
        });
        t.put_file("P1", b"/data/local/tmp/x", vec![1], 0o644);
        assert!(t.shell("P1", "rm /data/local/tmp/x").unwrap().success());
    }

    #[test]
    fn pushed_files_land_in_the_store_and_pull_back() {
        let dir = scratch("push-pull");
        let local = dir.join("daemon");
        let data: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
        std::fs::write(&local, &data).unwrap();

        let t = phone();
        t.push("P1", &local, "/data/local/tmp/socketsweep", 0o755)
            .unwrap();
        let stored = t.file("P1", b"/data/local/tmp/socketsweep").unwrap();
        assert_eq!(stored.mode, 0o755);
        assert_eq!(stored.data, data);

        let back = dir.join("back");
        let mut seen = Vec::new();
        let n = t
            .pull("P1", b"/data/local/tmp/socketsweep", &back, &mut |done| {
                seen.push(done)
            })
            .unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(std::fs::read(&back).unwrap(), data);
        // Cumulative, in sync-sized chunks, ending at the full size.
        assert_eq!(seen, [65_536, 131_072, 196_608, 200_000]);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn non_utf8_remote_paths_are_exact() {
        let dir = scratch("bytes");
        let t = phone();
        let name = b"/sdcard/\xFF\xFEpic.jpg";
        t.put_file("P1", name, b"jpeg".to_vec(), 0o660);
        t.pull("P1", name, &dir.join("pic"), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(dir.join("pic")).unwrap(), b"jpeg");
        assert!(matches!(
            t.pull("P1", b"/sdcard/\xFFpic.jpg", &dir.join("x"), &mut |_| {}),
            Err(TransportError::Refused(_))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pushing_a_missing_local_file_is_a_local_error() {
        let t = phone();
        let err = t
            .push("P1", Path::new("does/not/exist"), "/x", 0o755)
            .unwrap_err();
        assert!(matches!(err, TransportError::Local { .. }), "{err:?}");
    }

    #[test]
    fn injected_faults_fire_once() {
        let t = phone();
        t.fail_next(FakeOp::Devices, TransportError::Server("down".into()));
        assert!(matches!(t.devices(), Err(TransportError::Server(_))));
        assert!(t.devices().is_ok());

        t.fail_next(FakeOp::OpenTunnel, TransportError::Refused("no".into()));
        assert!(t.open_tunnel("P1", "x").is_err());
        assert!(t.open_tunnel("P1", "x").is_ok());
    }

    #[test]
    fn tunnels_reach_the_registered_handler() {
        let t = phone();
        let (closed_tx, closed_rx) = mpsc::channel();
        t.on_tunnel("P1", "socketsweep", move |mut daemon| {
            // An echo server that reports when the host hangs up.
            let mut buf = [0u8; 64];
            loop {
                match daemon.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => daemon.write_all(&buf[..n]).unwrap(),
                    Err(e) => panic!("{e}"),
                }
            }
            closed_tx.send(()).unwrap();
        });

        let mut tunnel = t.open_tunnel("P1", "socketsweep").unwrap();
        tunnel.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        tunnel.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        assert_eq!(t.tunnels_opened("P1", "socketsweep"), 1);

        drop(tunnel);
        closed_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the handler sees end of stream once the host end is dropped");
    }

    #[test]
    fn a_tunnel_with_no_listener_reads_end_of_stream() {
        let t = phone();
        let mut tunnel = t.open_tunnel("P1", "nobody").unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(tunnel.read(&mut buf).unwrap(), 0);
        assert_eq!(
            tunnel.write(b"x").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn pipe_reads_time_out() {
        let (a, _b) = pipe();
        a.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        let mut a = a;
        let started = Instant::now();
        let err = a.read(&mut [0u8; 1]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    #[test]
    fn pipe_writes_block_at_capacity_until_read() {
        let (mut a, mut b) = pipe_with_capacity(4);
        assert_eq!(
            a.write(b"abcdef").unwrap(),
            4,
            "a partial write up to capacity"
        );

        a.set_write_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        assert_eq!(a.write(b"x").unwrap_err().kind(), io::ErrorKind::TimedOut);
        a.set_write_timeout(None).unwrap();

        let writer = std::thread::spawn(move || {
            a.write_all(b"efgh").unwrap();
            a
        });
        let mut got = [0u8; 8];
        b.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"abcdefgh");
        writer.join().unwrap();
    }

    #[test]
    fn data_sent_before_close_is_still_delivered() {
        let (mut a, mut b) = pipe();
        a.write_all(b"last words").unwrap();
        drop(a);
        let mut s = String::new();
        b.read_to_string(&mut s).unwrap();
        assert_eq!(s, "last words");
    }

    #[test]
    fn shutdown_through_a_clone_wakes_a_blocked_reader() {
        let (a, mut b) = pipe();
        let mut reader = a.try_clone().unwrap();
        let blocked = std::thread::spawn(move || reader.read(&mut [0u8; 1]).unwrap());
        std::thread::sleep(Duration::from_millis(20));
        a.shutdown().unwrap();
        assert_eq!(blocked.join().unwrap(), 0);
        // The peer sees end of stream too, and cannot write any more.
        assert_eq!(b.read(&mut [0u8; 1]).unwrap(), 0);
        assert_eq!(b.write(b"x").unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn a_pipe_end_stays_open_while_any_clone_lives() {
        let (a, mut b) = pipe();
        let mut clone = a.try_clone().unwrap();
        drop(a);
        clone.write_all(b"x").unwrap();
        let mut buf = [0u8; 1];
        b.read_exact(&mut buf).unwrap();
        drop(clone);
        assert_eq!(b.read(&mut buf).unwrap(), 0);
    }
}

//! Daemon lifecycle and the connection to it.
//!
//! Push the binary, start it, open the tunnel, confirm it answers, and tear all
//! of that down again.
//!
//! The tunnel forwards to `localabstract:<name>` rather than a port on the
//! device, so the daemon is not exposed to other apps, and the name is fresh per
//! session so a stale daemon cannot be mistaken for the current one. The host
//! end is a port the adb server picks, so nothing else on the desktop can be
//! holding it already. Teardown also runs from `RunEvent::Exit`, so closing the
//! window does not leave a forward open and a daemon running on the phone.
//!
//! # Authentication
//!
//! Each session generates a random token, hands it to the daemon through the
//! environment it is launched with, and presents it in the `Hello` that opens
//! every connection. Anything else on the desktop that finds the forwarded port
//! is refused, which matters because the daemon's commands include recursive
//! delete.

use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::Rng;
use socketsweep_protocol::{
    read_msg, token_to_hex, write_msg, Frame, Request, Token, PROTOCOL_VERSION, TOKEN_ENV,
};

use crate::adb::{Adb, AdbError, Result as AdbResult};
use crate::lock;

/// The root scanned when the frontend does not name one.
pub const DEFAULT_ROOT: &str = "/sdcard";

const DEVICE_BIN: &str = "/data/local/tmp/socketsweep-daemon";

/// The daemon's stderr. Read back into the error when it never answers, since a
/// daemon that exits at startup says why there and nowhere else.
const DEVICE_LOG: &str = "/data/local/tmp/socketsweep.log";

/// How much of [`DEVICE_LOG`] an error quotes.
const LOG_EXCERPT_BYTES: usize = 2048;

/// Android 11. The daemon is built for, and tested against, nothing older.
const MIN_SDK: u32 = 30;

/// The only daemon build that ships.
const REQUIRED_ABI: &str = "arm64-v8a";

/// Kill any running daemon.
///
/// The bracket stops the pattern matching the shell that runs it. Unbracketed,
/// `pkill -f socketsweep-daemon` finds its own `sh -c` command line, and since
/// pkill signals matches as it walks `/proc` it can kill its own shell before
/// reaching the daemon — leaving alive the very process this exists to stop.
///
/// The protection is per-invocation: a compound command whose other arguments
/// contain the literal name is vulnerable again, which is why the call sites
/// keep this as a command of its own.
const KILL_DAEMON: &str = "pkill -f '[s]ocketsweep-daemon'";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A scan streams frames continuously, so this bounds the gap between frames
/// rather than the whole scan.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// Per-attempt wait while the freshly started daemon binds its socket. Until it
/// does, adb accepts the connection and then closes it, so a short wait is
/// enough to tell "not yet" from "answering".
const STARTUP_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(500);
/// How long the daemon gets to start answering before the connect fails.
const STARTUP_DEADLINE: Duration = Duration::from_secs(10);
const STARTUP_RETRY_GAP: Duration = Duration::from_millis(50);
/// Teardown should not hold up closing the app on a daemon that is not
/// answering.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Returned by [`Session::scan`] when [`ScanCancel::cancel`] stopped it.
pub const SCAN_CANCELLED: &str = "the scan was cancelled";

/// Returned by [`Session::scan`] when the stream ends without `ScanDone`.
pub const SCAN_CUT_OFF: &str = "the daemon closed the connection before the scan finished";

pub struct Session {
    pub serial: String,
    pub socket_name: String,
    /// The root the session was started for, as the user chose it.
    pub root: String,
    /// Host end of the tunnel, chosen by the adb server.
    port: u16,
    token: Token,
}

/// Lets another thread stop a running [`Session::scan`].
///
/// Cancelling shuts the scan's socket down, which unblocks a read waiting on
/// the daemon, and sets a flag the scan checks between frames, which stops it
/// acting on frames already buffered.
#[derive(Default)]
pub struct ScanCancel {
    cancelled: AtomicBool,
    stream: Mutex<Option<TcpStream>>,
}

impl ScanCancel {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(stream) = lock(&self.stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Hold a handle on the scan's socket so [`ScanCancel::cancel`] can close
    /// it. A cancel that arrived first closes it at once.
    fn attach(&self, stream: &TcpStream) -> Result<(), String> {
        let handle = stream.try_clone().map_err(|e| e.to_string())?;
        *lock(&self.stream) = Some(handle);
        if self.is_cancelled() {
            self.cancel();
        }
        Ok(())
    }
}

/// Random enough that two sessions cannot collide. Not the security boundary:
/// that is the token.
fn random_socket_name() -> String {
    let mut rng = rand::rng();
    let suffix: String = (0..16)
        .map(|_| {
            const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
            CHARS[rng.random_range(0..CHARS.len())] as char
        })
        .collect();
    format!("socketsweep-{suffix}")
}

/// A fresh session token. `rand::rng()` is a CSPRNG seeded from the operating
/// system.
fn random_token() -> Token {
    let mut token = [0u8; socketsweep_protocol::TOKEN_LEN];
    rand::rng().fill(&mut token);
    token
}

/// Accept `root` only if it is one of the storage roots a phone exposes:
/// `/sdcard`, `/storage/emulated/<user>`, or an SD card at
/// `/storage/XXXX-XXXX`.
///
/// The root is placed on the daemon's command line, and the daemon confines
/// deletes to it, so an arbitrary string is refused rather than interpreted.
pub fn check_root(root: &str) -> Result<(), String> {
    fn is_user_storage(rest: &str) -> bool {
        !rest.is_empty() && rest.len() <= 9 && rest.bytes().all(|b| b.is_ascii_digit())
    }
    fn is_sd_card(rest: &str) -> bool {
        let b = rest.as_bytes();
        b.len() == 9
            && b[4] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || c.is_ascii_hexdigit())
    }

    let ok = root == DEFAULT_ROOT
        || root
            .strip_prefix("/storage/emulated/")
            .is_some_and(is_user_storage)
        || root.strip_prefix("/storage/").is_some_and(is_sd_card);

    if ok {
        Ok(())
    } else {
        Err(format!(
            "Cannot scan {root:?}. Choose /sdcard, /storage/emulated/<user>, or an SD card at /storage/XXXX-XXXX."
        ))
    }
}

/// Quote `s` as one word for `sh`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The opening of [`DEVICE_LOG`], for an error message.
fn daemon_log(adb: &Adb, serial: &str) -> Option<String> {
    let log = adb.shell(serial, &format!("cat {DEVICE_LOG}")).ok()?;
    let log = log.trim();
    if log.is_empty() {
        return None;
    }
    let mut end = log.len().min(LOG_EXCERPT_BYTES);
    while !log.is_char_boundary(end) {
        end -= 1;
    }
    let ellipsis = if end < log.len() { "…" } else { "" };
    Some(format!("{}{ellipsis}", &log[..end]))
}

/// Undo whatever [`Session::start`] got as far as setting up.
fn tear_down(adb: &Adb, serial: &str, port: Option<u16>) {
    if let Some(port) = port {
        adb.forward_remove(serial, port);
    }
    adb.shell_ignoring_errors(serial, KILL_DAEMON);
    adb.shell_ignoring_errors(serial, &format!("rm -f {DEVICE_BIN}"));
}

impl Session {
    /// Check the phone can run the daemon, push it, start it, open the tunnel
    /// and confirm it answers.
    ///
    /// `root` must already have passed [`check_root`]. On failure after the
    /// push, everything set up so far is torn down again.
    pub fn start(adb: &Adb, serial: &str, daemon_binary: &Path, root: &str) -> AdbResult<Session> {
        let info = adb.device_info(serial)?;
        if info.sdk < MIN_SDK {
            return Err(AdbError::Device(format!(
                "SocketSweep needs Android 11 or newer (this phone runs API {}).",
                info.sdk
            )));
        }
        if info.abi != REQUIRED_ABI {
            return Err(AdbError::Device(format!(
                "SocketSweep supports 64-bit ARM phones only (this phone is {}).",
                info.abi
            )));
        }

        // Any daemon left over from a crashed run holds the old socket and the
        // old binary path.
        adb.shell_ignoring_errors(serial, KILL_DAEMON);

        // Scoped Storage otherwise hides most of /sdcard from the shell user.
        adb.shell_ignoring_errors(
            serial,
            "appops set com.android.shell MANAGE_EXTERNAL_STORAGE allow",
        );

        adb.push(serial, daemon_binary, DEVICE_BIN)?;

        let mut port = None;
        let result = Self::launch(adb, serial, root, &mut port);
        if result.is_err() {
            tear_down(adb, serial, port);
        }
        result
    }

    /// Everything after the push. `port` records the forward as soon as it
    /// exists, so a failure after that point can remove it.
    fn launch(adb: &Adb, serial: &str, root: &str, port: &mut Option<u16>) -> AdbResult<Session> {
        adb.shell(serial, &format!("chmod 755 {DEVICE_BIN}"))?;

        let socket_name = random_socket_name();
        let token = random_token();

        // nohup + background, otherwise the daemon dies with the shell. Its
        // stdin is detached so it does not hold the shell connection open.
        adb.shell_ignoring_errors(
            serial,
            &format!(
                "{TOKEN_ENV}={} nohup {DEVICE_BIN} --socket {socket_name} --root {} \
                 </dev/null >/dev/null 2>{DEVICE_LOG} &",
                token_to_hex(&token),
                shell_quote(root),
            ),
        );

        let forwarded = adb.forward_any(serial, &format!("localabstract:{socket_name}"))?;
        *port = Some(forwarded);

        let session = Session {
            serial: serial.to_string(),
            socket_name,
            root: root.to_string(),
            port: forwarded,
            token,
        };

        // The daemon needs a moment to bind. Retry rather than sleeping a fixed
        // pessimistic amount.
        let deadline = Instant::now() + STARTUP_DEADLINE;
        let last = loop {
            match session.ping(STARTUP_ATTEMPT_TIMEOUT) {
                Ok(()) => return Ok(session),
                Err(e) if Instant::now() >= deadline => break e,
                Err(_) => std::thread::sleep(STARTUP_RETRY_GAP),
            }
        };

        let mut message = format!(
            "The daemon was pushed and started but never answered on the tunnel. Last error: {last}"
        );
        if let Some(log) = daemon_log(adb, serial) {
            message.push_str("\n\nDaemon log:\n");
            message.push_str(&log);
        }
        Err(AdbError::Device(message))
    }

    /// Open a connection and complete the handshake, so whatever is sent next
    /// is served.
    fn open(&self, read_timeout: Duration) -> Result<(BufReader<TcpStream>, TcpStream), String> {
        let addr: SocketAddr = ([127, 0, 0, 1], self.port).into();
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("cannot reach the daemon tunnel: {e}"))?;
        stream
            .set_read_timeout(Some(read_timeout))
            .map_err(|e| format!("cannot set read timeout: {e}"))?;

        let mut reader =
            BufReader::with_capacity(256 * 1024, stream.try_clone().map_err(|e| e.to_string())?);
        send(
            &stream,
            &Request::Hello {
                token: self.token,
                protocol: PROTOCOL_VERSION,
            },
        )?;
        match read_msg::<_, Frame>(&mut reader) {
            Ok(Some(Frame::Pong)) => Ok((reader, stream)),
            Ok(Some(Frame::Error { message })) => {
                Err(format!("the daemon refused the connection: {message}"))
            }
            Ok(Some(other)) => Err(format!("unexpected reply to the handshake: {other:?}")),
            Ok(None) => Err("the daemon closed the connection during the handshake".into()),
            Err(e) => Err(format!("handshake with the daemon failed: {e}")),
        }
    }

    fn ping(&self, timeout: Duration) -> Result<(), String> {
        let (mut reader, stream) = self.open(timeout)?;
        send(&stream, &Request::Ping)?;
        match read_msg::<_, Frame>(&mut reader).map_err(|e| e.to_string())? {
            Some(Frame::Pong) => Ok(()),
            Some(other) => Err(format!("expected Pong, got {other:?}")),
            None => Err("daemon closed the connection without answering".into()),
        }
    }

    /// Run a scan, handing every frame to `on_frame` as it arrives.
    ///
    /// Nothing is buffered: the callback folds each frame into the arena so the
    /// UI can draw a partial tree while the walk is still going. See
    /// [`read_scan`] for what counts as success.
    pub fn scan<F>(&self, root: &[u8], cancel: &ScanCancel, on_frame: F) -> Result<(), String>
    where
        F: FnMut(Frame),
    {
        let (mut reader, stream) = self.open(READ_TIMEOUT)?;
        cancel.attach(&stream)?;
        send(
            &stream,
            &Request::Scan {
                root: root.to_vec(),
            },
        )
        .map_err(|e| {
            if cancel.is_cancelled() {
                SCAN_CANCELLED.into()
            } else {
                e
            }
        })?;
        read_scan(&mut reader, cancel, on_frame)
    }

    /// Ask the daemon to delete a path. The daemon re-validates it against the
    /// session root; this side does not police it.
    pub fn delete(&self, path: &[u8]) -> Result<u64, String> {
        let (mut reader, stream) = self.open(READ_TIMEOUT)?;
        send(
            &stream,
            &Request::Delete {
                path: path.to_vec(),
            },
        )?;
        match read_msg::<_, Frame>(&mut reader).map_err(|e| e.to_string())? {
            Some(Frame::Deleted { items }) => Ok(items),
            Some(Frame::Error { message }) => Err(message),
            Some(other) => Err(format!("unexpected reply to delete: {other:?}")),
            None => Err("daemon closed the connection without answering".into()),
        }
    }

    /// Best-effort teardown. Every step is allowed to fail: the device may
    /// already be unplugged, which is one of the ways we get here.
    pub fn stop(&self, adb: &Adb) {
        if let Ok((_, stream)) = self.open(SHUTDOWN_TIMEOUT) {
            let _ = send(&stream, &Request::Shutdown);
            let _ = stream.shutdown(Shutdown::Both);
        }
        tear_down(adb, &self.serial, Some(self.port));
    }
}

fn send(stream: &TcpStream, request: &Request) -> Result<(), String> {
    let mut writer = BufWriter::new(stream);
    write_msg(&mut writer, request).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

/// Read a scan reply to its end, handing each frame other than `Error` to
/// `on_frame`.
///
/// Succeeds only when the stream reaches `ScanDone` with no `Error` before it.
/// An `Error` is held until `ScanDone` arrives and then returned, so a failed
/// walk never reads as a finished one; that `ScanDone` is not passed on. A
/// stream that ends without `ScanDone` is [`SCAN_CUT_OFF`], and a cancelled one
/// is [`SCAN_CANCELLED`], with no further frames passed on.
pub fn read_scan<R, F>(reader: &mut R, cancel: &ScanCancel, mut on_frame: F) -> Result<(), String>
where
    R: Read,
    F: FnMut(Frame),
{
    let mut error: Option<String> = None;
    loop {
        let frame = read_msg::<_, Frame>(reader);
        if cancel.is_cancelled() {
            return Err(SCAN_CANCELLED.into());
        }
        match frame.map_err(|e| e.to_string())? {
            None => return Err(error.unwrap_or_else(|| SCAN_CUT_OFF.into())),
            Some(Frame::Error { message }) => {
                error.get_or_insert(message);
            }
            Some(frame @ Frame::ScanDone(_)) => {
                return match error {
                    Some(message) => Err(message),
                    None => {
                        on_frame(frame);
                        Ok(())
                    }
                };
            }
            Some(frame) => on_frame(frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supported_roots_are_accepted() {
        for root in [
            "/sdcard",
            "/storage/emulated/0",
            "/storage/emulated/10",
            "/storage/1A2B-3C4D",
            "/storage/abcd-ef01",
        ] {
            assert_eq!(check_root(root), Ok(()), "{root}");
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for root in [
            "",
            "/",
            "/sdcard/",
            "/sdcard/DCIM",
            "/sdcard; rm -rf /",
            "/sdcard'",
            "/storage/emulated",
            "/storage/emulated/",
            "/storage/emulated/0/",
            "/storage/emulated/0/Android",
            "/storage/emulated/x",
            "/storage/emulated/0 1",
            "/storage/1A2B3C4D",
            "/storage/1A2B-3C4G",
            "/storage/1A2B-3C4D/",
            "/storage/self/primary",
            "/data",
            "sdcard",
            "/storage/emulated/../../data",
        ] {
            assert!(check_root(root).is_err(), "{root:?} should be refused");
        }
    }

    #[test]
    fn shell_quoting_survives_single_quotes_and_spaces() {
        assert_eq!(shell_quote("/sdcard"), "'/sdcard'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn tokens_are_not_reused() {
        assert_ne!(random_token(), random_token());
    }
}

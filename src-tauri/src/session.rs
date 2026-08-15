//! Daemon lifecycle and the connection to it.
//!
//! Owns the sequence that used to live inline in `init_daemon`: push the binary,
//! start it, open the tunnel, confirm it answers. Differences from the version
//! it replaces:
//!
//!   - the tunnel forwards to `localabstract:<random-name>` rather than
//!     `tcp:5050` on the device, so the daemon is not exposed to other apps
//!   - the socket name is fresh per session, so a stale daemon from a previous
//!     run cannot be mistaken for this one
//!   - the shutdown path also runs from `RunEvent::Exit`, so closing the window
//!     no longer leaks a forward and a running daemon on the phone

use std::io::{BufReader, BufWriter};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use rand::Rng;
use socketsweep_protocol::{read_msg, write_msg, Frame, Request};

use crate::adb::{Adb, Result as AdbResult};

/// Host-side port for the tunnel. The device side is an abstract socket, so this
/// number only has to be free on the desktop.
pub const HOST_PORT: u16 = 5050;

const DEVICE_BIN: &str = "/data/local/tmp/socketsweep-daemon";

/// Kill any running daemon.
///
/// The bracket around the first letter stops the pattern from matching the very
/// shell that is running it. Without it `pkill -f socketsweep-daemon` finds its
/// own `sh -c` command line, and since pkill signals matches as it walks
/// `/proc`, it can kill its own shell before ever reaching the daemon — leaving
/// the stale process alive, which is the one thing this command exists to
/// prevent. Verified on a device: the plain form exits 143 and never completes.
const KILL_DAEMON: &str = "pkill -f '[s]ocketsweep-daemon'";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A scan streams frames continuously, so this bounds the gap between frames
/// rather than the whole scan. The old code allowed 120s for one giant response.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Session {
    pub serial: String,
    pub socket_name: String,
    pub root: Vec<u8>,
}

/// Random enough that two sessions cannot collide; not a security boundary,
/// which is SELinux's job here.
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

impl Session {
    /// Push the daemon, start it, open the tunnel and confirm it answers.
    pub fn start(
        adb: &mut Adb,
        serial: &str,
        daemon_binary: &Path,
        root: &[u8],
    ) -> AdbResult<Session> {
        // Any daemon left over from a crashed run holds the old socket and the
        // old binary path.
        adb.shell_ignoring_errors(serial, KILL_DAEMON);

        // Scoped Storage otherwise hides most of /sdcard from the shell user.
        adb.shell_ignoring_errors(
            serial,
            "appops set com.android.shell MANAGE_EXTERNAL_STORAGE allow",
        );

        adb.push(serial, daemon_binary, DEVICE_BIN)?;
        adb.shell(serial, &format!("chmod 755 {DEVICE_BIN}"))?;

        let socket_name = random_socket_name();
        let root_str = String::from_utf8_lossy(root).into_owned();

        // nohup + background, otherwise the daemon dies with the shell.
        adb.shell_ignoring_errors(
            serial,
            &format!(
                "nohup {DEVICE_BIN} --socket {socket_name} --root {root_str} >/dev/null 2>&1 &"
            ),
        );

        adb.forward_abstract(serial, HOST_PORT, &socket_name)?;

        let session = Session {
            serial: serial.to_string(),
            socket_name,
            root: root.to_vec(),
        };

        // The daemon needs a moment to bind. Retry rather than sleeping a fixed
        // pessimistic amount.
        let mut last = String::new();
        for _ in 0..40 {
            match session.ping() {
                Ok(()) => return Ok(session),
                Err(e) => {
                    last = e;
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }

        Err(crate::adb::AdbError::Device(format!(
            "The daemon was pushed and started but never answered on the tunnel. Last error: {last}"
        )))
    }

    fn connect(&self) -> Result<TcpStream, String> {
        let addr: SocketAddr = ([127, 0, 0, 1], HOST_PORT).into();
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("cannot reach the daemon tunnel: {e}"))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| format!("cannot set read timeout: {e}"))?;
        Ok(stream)
    }

    pub fn ping(&self) -> Result<(), String> {
        let stream = self.connect()?;
        let mut writer = BufWriter::new(stream.try_clone().map_err(|e| e.to_string())?);
        write_msg(&mut writer, &Request::Ping).map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;

        let mut reader = BufReader::new(stream);
        match read_msg::<_, Frame>(&mut reader).map_err(|e| e.to_string())? {
            Some(Frame::Pong) => Ok(()),
            Some(other) => Err(format!("expected Pong, got {other:?}")),
            None => Err("daemon closed the connection without answering".into()),
        }
    }

    /// Run a scan, handing every frame to `on_frame` as it arrives.
    ///
    /// Nothing is buffered: the callback folds each frame into the arena so the
    /// UI can draw a partial tree while the walk is still going.
    pub fn scan<F>(&self, root: &[u8], mut on_frame: F) -> Result<(), String>
    where
        F: FnMut(Frame),
    {
        let stream = self.connect()?;
        let mut writer = BufWriter::new(stream.try_clone().map_err(|e| e.to_string())?);
        write_msg(
            &mut writer,
            &Request::Scan {
                root: root.to_vec(),
            },
        )
        .map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;

        let mut reader = BufReader::with_capacity(256 * 1024, stream);
        loop {
            match read_msg::<_, Frame>(&mut reader).map_err(|e| e.to_string())? {
                None => return Ok(()),
                Some(frame) => {
                    let done = matches!(frame, Frame::ScanDone(_));
                    on_frame(frame);
                    if done {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Ask the daemon to delete a path. The daemon re-validates it against the
    /// session root; this side does not police it.
    pub fn delete(&self, path: &[u8]) -> Result<u64, String> {
        let stream = self.connect()?;
        let mut writer = BufWriter::new(stream.try_clone().map_err(|e| e.to_string())?);
        write_msg(
            &mut writer,
            &Request::Delete {
                path: path.to_vec(),
            },
        )
        .map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut writer).map_err(|e| e.to_string())?;

        let mut reader = BufReader::new(stream);
        match read_msg::<_, Frame>(&mut reader).map_err(|e| e.to_string())? {
            Some(Frame::Deleted { items }) => Ok(items),
            Some(Frame::Error { message }) => Err(message),
            Some(other) => Err(format!("unexpected reply to delete: {other:?}")),
            None => Err("daemon closed the connection without answering".into()),
        }
    }

    /// Best-effort teardown. Every step is allowed to fail: the device may
    /// already be unplugged, which is one of the ways we get here.
    pub fn stop(&self, adb: &mut Adb) {
        if let Ok(stream) = self.connect() {
            let mut writer = BufWriter::new(&stream);
            let _ = write_msg(&mut writer, &Request::Shutdown);
            let _ = std::io::Write::flush(&mut writer);
            let _ = stream.shutdown(Shutdown::Both);
        }
        adb.forward_remove(&self.serial, HOST_PORT);
        adb.shell_ignoring_errors(&self.serial, KILL_DAEMON);
        adb.shell_ignoring_errors(&self.serial, &format!("rm -f {DEVICE_BIN}"));
    }
}

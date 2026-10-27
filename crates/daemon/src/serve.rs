//! Socket listener and request dispatch. Linux/Android only.

use std::io::{BufReader, BufWriter, Read, Write};
// Abstract-namespace sockets are a Linux kernel feature, but std files the
// extension trait under a per-OS module and Android is not `target_os = "linux"`.
#[cfg(target_os = "android")]
use std::os::android::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use socketsweep_protocol::{
    read_msg, token_from_hex, tokens_match, write_msg, Frame, ProtocolError, Request, ScanStats,
    Token, KEEPALIVE_INTERVAL, PROTOCOL_VERSION, TOKEN_ENV, TOKEN_LEN,
};
use socketsweep_scanner::{scan, ScanConfig, ScanError};

use crate::{guard, keepalive};

/// How long a connection may sit without sending a request. The daemon serves
/// one connection at a time, so a client that connects and goes quiet would
/// otherwise lock every other client out indefinitely.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a single write may block on a host that is not reading. Generous,
/// because a scan streams for minutes and the host may briefly stall while it
/// folds frames into its tree; a write that does time out ends the scan.
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

struct Args {
    socket: String,
    root: PathBuf,
    threads: usize,
}

fn usage() -> ! {
    eprintln!("usage: {TOKEN_ENV}=<hex> socketsweep-daemon --socket <abstract-name> [--root <path>] [--threads <n>]");
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut socket = None;
    let mut root = PathBuf::from("/sdcard");
    let mut threads = 0usize;

    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--socket" => socket = Some(argv.next().unwrap_or_else(|| usage())),
            "--root" => root = PathBuf::from(argv.next().unwrap_or_else(|| usage())),
            "--threads" => {
                threads = argv
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            _ => usage(),
        }
    }

    Args {
        socket: socket.unwrap_or_else(|| usage()),
        root,
        threads,
    }
}

/// The session token comes from the environment, never argv: a process's
/// arguments are readable by anything that can list processes, its
/// environment only by the same user.
fn read_token() -> Result<Token, String> {
    let raw = std::env::var_os(TOKEN_ENV).ok_or_else(|| {
        format!("{TOKEN_ENV} is not set; the host passes the session token through it")
    })?;
    raw.to_str()
        .and_then(token_from_hex)
        .ok_or_else(|| format!("{TOKEN_ENV} must be exactly {} hex digits", TOKEN_LEN * 2))
}

pub fn main() -> ExitCode {
    let args = parse_args();
    let token = match read_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[daemon] refusing to start: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The host supplies a fresh socket name per session, so a stale daemon
    // cannot be mistaken for the current one.
    let addr = match SocketAddr::from_abstract_name(args.socket.as_bytes()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[daemon] invalid socket name {:?}: {e}", args.socket);
            return ExitCode::FAILURE;
        }
    };

    let listener = match UnixListener::bind_addr(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[daemon] cannot bind abstract socket {:?}: {e}",
                args.socket
            );
            return ExitCode::FAILURE;
        }
    };

    eprintln!(
        "[daemon] listening on abstract:{} (root {}, protocol {PROTOCOL_VERSION}, pid {})",
        args.socket,
        args.root.display(),
        std::process::id()
    );

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                if handle_client(stream, &args, &token) == Control::Shutdown {
                    break;
                }
            }
            Err(e) => eprintln!("[daemon] accept failed: {e}"),
        }
    }

    eprintln!("[daemon] shutdown complete");
    ExitCode::SUCCESS
}

#[derive(PartialEq, Eq)]
enum Control {
    /// Keep serving this connection.
    Continue,
    /// Drop this connection and go back to accepting.
    Close,
    /// Stop the daemon.
    Shutdown,
}

fn handle_client(stream: UnixStream, args: &Args, token: &Token) -> Control {
    // Socket options belong to the socket, not the descriptor, so these also
    // cover the clones below and the one a scan writes through.
    if let Err(e) = stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(WRITE_TIMEOUT)))
    {
        eprintln!("[daemon] cannot set socket timeouts: {e}");
        return Control::Continue;
    }

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[daemon] cannot clone stream: {e}");
            return Control::Continue;
        }
    });
    let mut writer = BufWriter::new(match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[daemon] cannot clone stream: {e}");
            return Control::Continue;
        }
    });

    let accepted = handshake(&mut reader, &mut writer, token);
    if writer.flush().is_err() || !accepted {
        return Control::Continue;
    }

    loop {
        let request: Request = match read_msg(&mut reader) {
            Ok(Some(r)) => r,
            Ok(None) => return Control::Continue, // peer hung up between requests
            Err(e) => {
                report_read_error(&e);
                return Control::Continue;
            }
        };

        let control = dispatch(request, args, &stream, &mut writer);
        if writer.flush().is_err() {
            return Control::Continue;
        }
        match control {
            Control::Continue => {}
            Control::Close => return Control::Continue,
            Control::Shutdown => return Control::Shutdown,
        }
    }
}

/// Read and check the [`Request::Hello`] every connection must open with,
/// answering `Pong` or `Error`. Returns whether to go on serving.
fn handshake<R: Read, W: Write>(reader: &mut R, out: &mut W, token: &Token) -> bool {
    let refusal = match read_msg::<_, Request>(reader) {
        Ok(Some(Request::Hello {
            token: offered,
            protocol,
        })) => {
            // The token is checked first so that only a host holding it learns
            // anything about this daemon, its version included.
            if !tokens_match(&offered, token) {
                "authentication failed".to_string()
            } else if protocol != PROTOCOL_VERSION {
                format!(
                    "protocol mismatch: the host speaks version {protocol}, \
                     this daemon speaks version {PROTOCOL_VERSION}"
                )
            } else {
                let _ = write_msg(out, &Frame::Pong);
                return true;
            }
        }
        Ok(Some(_)) => "the first request on a connection must be Hello".to_string(),
        Ok(None) => return false,
        // A body that does not decode is most likely a host built against a
        // different protocol, which deserves an answer rather than silence.
        Err(e @ ProtocolError::Decode(_)) => format!("unreadable handshake: {e}"),
        Err(e) => {
            report_read_error(&e);
            return false;
        }
    };

    eprintln!("[daemon] refused connection: {refusal}");
    let _ = write_msg(out, &Frame::Error { message: refusal });
    false
}

fn report_read_error(e: &ProtocolError) {
    match e {
        ProtocolError::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            eprintln!(
                "[daemon] client sent nothing for {}s, closing",
                READ_TIMEOUT.as_secs()
            );
        }
        _ => eprintln!("[daemon] malformed request: {e}"),
    }
}

fn dispatch<W: Write>(request: Request, args: &Args, stream: &UnixStream, out: &mut W) -> Control {
    match request {
        Request::Ping => {
            let _ = write_msg(out, &Frame::Pong);
            Control::Continue
        }

        Request::Shutdown => {
            let _ = write_msg(out, &Frame::Pong);
            Control::Shutdown
        }

        Request::Hello { .. } => {
            let _ = write_msg(
                out,
                &Frame::Error {
                    message: "Hello is only valid as the first request on a connection".into(),
                },
            );
            Control::Close
        }

        Request::Scan { root } => {
            let requested = bytes_to_path(&root);
            match guard::resolve_at_or_under_root(&args.root, &requested) {
                Ok(resolved) => run_scan(&resolved, args.threads, stream, out),
                Err(e) => {
                    eprintln!("[daemon] rejected scan: {e}");
                    scan_failed(out, e.to_string());
                    Control::Continue
                }
            }
        }

        Request::Delete { path } => {
            let requested = bytes_to_path(&path);
            match guard::resolve_under_root(&args.root, &requested) {
                Ok(resolved) => run_delete(resolved, out),
                Err(e) => {
                    eprintln!("[daemon] rejected delete: {e}");
                    let _ = write_msg(
                        out,
                        &Frame::Error {
                            message: e.to_string(),
                        },
                    );
                    Control::Continue
                }
            }
        }
    }
}

/// Remove `target` and report how it went, sending [`Frame::Keepalive`] every
/// [`KEEPALIVE_INTERVAL`] until it is done.
///
/// Counting and removing a large folder through FUSE can take minutes, and the
/// host bounds how long it waits in silence, not how long it waits in all.
fn run_delete<W: Write>(target: PathBuf, out: &mut W) -> Control {
    let worker_target = target.clone();
    let done = keepalive::run_with_keepalives(out, KEEPALIVE_INTERVAL, move || {
        guard::remove(&worker_target)
    });
    let result = done.result.and_then(|removed| removed);

    if let Err(e) = &result {
        eprintln!("[daemon] delete of {} failed: {e}", target.display());
    }
    if !done.connected {
        return Control::Close;
    }
    let frame = match result {
        Ok(items) => Frame::Deleted { items },
        Err(e) => Frame::Error {
            message: format!("failed to delete {}: {e}", target.display()),
        },
    };
    let _ = write_msg(out, &frame);
    Control::Continue
}

/// End a scan reply that produced no result. Every scan reply ends in
/// `ScanDone`, failures included, so the host never waits on a reply that is
/// not coming.
fn scan_failed<W: Write>(out: &mut W, message: String) {
    let _ = write_msg(out, &Frame::Error { message });
    let _ = write_msg(out, &Frame::ScanDone(ScanStats::default()));
}

fn run_scan<W: Write>(root: &Path, threads: usize, stream: &UnixStream, out: &mut W) -> Control {
    let cfg = ScanConfig {
        threads,
        ..ScanConfig::new(root)
    };

    // `root` is canonicalised, so it is typically /storage/emulated/0 rather
    // than the /sdcard that was requested. Every Dir frame is keyed on it, so
    // the host is told before any of them arrive.
    let _ = write_msg(
        out,
        &Frame::ScanStarted {
            root: path_bytes(root),
        },
    );

    // The scanner writes Dir frames straight into the socket as it walks, so the
    // host can start drawing before the walk finishes. It needs an owned `'static`
    // sink (jwalk's callback bound), hence a cloned descriptor rather than a
    // borrow of `out`. Flush first so the two writers cannot interleave.
    if out.flush().is_err() {
        return Control::Close; // the host is gone; there is no one to tell
    }
    let sink = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            scan_failed(out, format!("cannot clone socket: {e}"));
            return Control::Continue;
        }
    };

    match scan(&cfg, sink) {
        // A frame failed to send, most likely because the host has gone. Part of
        // one may already be on the wire, so anything written after it would be
        // misread; the connection is dropped instead.
        Err(e @ ScanError::Protocol(_)) => {
            eprintln!("[daemon] scan of {} abandoned: {e}", root.display());
            return Control::Close;
        }
        Ok(stats) => {
            eprintln!(
                "[daemon] scan of {} finished: {} files, {} dirs, {} bytes, {} errors, {} ms",
                root.display(),
                stats.files,
                stats.dirs,
                stats.total_size,
                stats.errors,
                stats.elapsed_ms,
            );
            let _ = write_msg(out, &Frame::ScanDone(stats));
        }
        Err(e) => {
            eprintln!("[daemon] scan failed: {e}");
            scan_failed(out, e.to_string());
        }
    }
    Control::Continue
}

#[cfg(unix)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

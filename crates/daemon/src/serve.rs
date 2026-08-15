//! Socket listener and request dispatch. Linux/Android only.

use std::io::{BufReader, BufWriter, Write};
// Abstract-namespace sockets are a Linux kernel feature, but std files the
// extension trait under a per-OS module and Android is not `target_os = "linux"`.
#[cfg(target_os = "android")]
use std::os::android::net::SocketAddrExt;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use socketsweep_protocol::{write_msg, Frame, Request, ScanStats};
use socketsweep_scanner::{scan, ScanConfig};

use crate::guard;

struct Args {
    socket: String,
    root: PathBuf,
    threads: usize,
}

fn usage() -> ! {
    eprintln!("usage: socketsweep-daemon --socket <abstract-name> [--root <path>] [--threads <n>]");
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

pub fn main() -> ExitCode {
    let args = parse_args();

    // The host generates a fresh socket name per session, so a stale daemon from
    // a previous run cannot be mistaken for this one.
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
        "[daemon] listening on abstract:{} (root {}, pid {})",
        args.socket,
        args.root.display(),
        std::process::id()
    );

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                if handle_client(stream, &args) == Control::Shutdown {
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
    Continue,
    Shutdown,
}

fn handle_client(stream: UnixStream, args: &Args) -> Control {
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

    loop {
        let request: Request = match socketsweep_protocol::read_msg(&mut reader) {
            Ok(Some(r)) => r,
            Ok(None) => return Control::Continue, // peer hung up between requests
            Err(e) => {
                eprintln!("[daemon] malformed request: {e}");
                return Control::Continue;
            }
        };

        let control = dispatch(request, args, &stream, &mut writer);
        if writer.flush().is_err() {
            return Control::Continue;
        }
        if control == Control::Shutdown {
            return Control::Shutdown;
        }
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

        Request::Scan { root } => {
            let requested = bytes_to_path(&root);
            match guard::resolve_at_or_under_root(&args.root, &requested) {
                Ok(resolved) => run_scan(&resolved, args.threads, stream, out),
                Err(e) => {
                    let _ = write_msg(
                        out,
                        &Frame::Error {
                            message: e.to_string(),
                        },
                    );
                }
            }
            Control::Continue
        }

        Request::Delete { path } => {
            let requested = bytes_to_path(&path);
            match guard::resolve_under_root(&args.root, &requested) {
                Ok(resolved) => {
                    let frame = match remove(&resolved) {
                        Ok(items) => Frame::Deleted { items },
                        Err(e) => Frame::Error {
                            message: format!("failed to delete {}: {e}", resolved.display()),
                        },
                    };
                    let _ = write_msg(out, &frame);
                }
                Err(e) => {
                    eprintln!("[daemon] rejected delete: {e}");
                    let _ = write_msg(
                        out,
                        &Frame::Error {
                            message: e.to_string(),
                        },
                    );
                }
            }
            Control::Continue
        }
    }
}

fn run_scan<W: Write>(root: &Path, threads: usize, stream: &UnixStream, out: &mut W) {
    let cfg = ScanConfig {
        root: root.to_path_buf(),
        threads,
        max_depth: 64,
    };

    // The scanner writes Dir frames straight into the socket as it walks, so the
    // host can start drawing before the walk finishes. It needs an owned `'static`
    // sink (jwalk's callback bound), hence a cloned descriptor rather than a
    // borrow of `out`. Flush first so the two writers cannot interleave.
    if out.flush().is_err() {
        return;
    }
    let sink = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            let _ = write_msg(
                out,
                &Frame::Error {
                    message: format!("cannot clone socket: {e}"),
                },
            );
            return;
        }
    };

    match scan(&cfg, sink) {
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
            let _ = write_msg(
                out,
                &Frame::Error {
                    message: e.to_string(),
                },
            );
            let _ = write_msg(out, &Frame::ScanDone(ScanStats::default()));
        }
    }
}

/// Remove a validated path, returning how many entries went with it.
fn remove(path: &Path) -> std::io::Result<u64> {
    // Judge the path itself, not what it points at: `resolve_under_root` has
    // already confirmed the destination is inside the root, so a symlink here
    // should be unlinked rather than followed into a recursive delete.
    let meta = std::fs::symlink_metadata(path)?;

    if meta.is_symlink() || !meta.is_dir() {
        std::fs::remove_file(path)?;
        return Ok(1);
    }

    let count = count_entries(path);
    std::fs::remove_dir_all(path)?;
    Ok(count)
}

fn count_entries(path: &Path) -> u64 {
    let mut n = 1;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(t) if t.is_dir() => n += count_entries(&entry.path()),
                Ok(_) => n += 1,
                Err(_) => {}
            }
        }
    }
    n
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

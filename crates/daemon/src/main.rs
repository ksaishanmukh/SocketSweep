//! `socketsweep-daemon`: the phone binary. See the library docs.
//!
//! ```text
//! SOCKETSWEEP_TOKEN=<64 hex digits> socketsweep-daemon --socket <abstract-name> [--root <path>] [--threads <n>]
//! ```
//!
//! - `--socket`: the abstract socket name to listen on (fresh per session).
//! - `--root`: the storage root every request is confined to (default
//!   `/sdcard`).
//! - `--threads`: workers per scan unless the host's `ScanOpts` says
//!   otherwise (default `clamp(2 × cores, 4, 8)`), reported in `Welcome`.
//!
//! The token comes from the environment, never argv, where any app could read
//! it from `/proc`. Logs go to stderr.
//!
//! Exit codes: 0 after `Shutdown`; 1 if the socket cannot be bound; 2 for bad
//! arguments; 3 for a missing or malformed token; 4 on a host that is not
//! Linux or Android.

use std::process::ExitCode;

/// Bad arguments.
pub const EXIT_USAGE: u8 = 2;
/// `SOCKETSWEEP_TOKEN` missing or malformed.
pub const EXIT_TOKEN: u8 = 3;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    socket: String,
    root: String,
    threads: Option<u8>,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut socket = None;
    let mut root = "/sdcard".to_string();
    let mut threads = None;
    let mut argv = argv.into_iter();
    while let Some(arg) = argv.next() {
        let mut value = |flag: &str| argv.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--socket" => socket = Some(value("--socket")?),
            "--root" => root = value("--root")?,
            "--threads" => {
                let v = value("--threads")?;
                let n: u8 = v
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("--threads must be 1-255, not {v:?}"))?;
                threads = Some(n);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let socket = socket.ok_or("--socket is required")?;
    if socket.is_empty() {
        return Err("--socket must not be empty".into());
    }
    Ok(Args {
        socket,
        root,
        threads,
    })
}

fn usage(why: &str) -> ExitCode {
    eprintln!("[daemon] {why}");
    eprintln!(
        "usage: {}=<hex> socketsweep-daemon --socket <abstract-name> [--root <path>] [--threads <n>]",
        socketsweep_wire::TOKEN_ENV
    );
    ExitCode::from(EXIT_USAGE)
}

fn read_token() -> Result<socketsweep_wire::Token, String> {
    use socketsweep_wire::{token_from_hex, TOKEN_ENV, TOKEN_LEN};
    let raw = std::env::var_os(TOKEN_ENV).ok_or_else(|| {
        format!("{TOKEN_ENV} is not set; the host passes the session token through it")
    })?;
    raw.to_str()
        .and_then(token_from_hex)
        .ok_or_else(|| format!("{TOKEN_ENV} must be exactly {} hex digits", TOKEN_LEN * 2))
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(why) => return usage(&why),
    };
    let token = match read_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[daemon] refusing to start: {e}");
            return ExitCode::from(EXIT_TOKEN);
        }
    };
    run(args, token)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn run(args: Args, token: socketsweep_wire::Token) -> ExitCode {
    use std::sync::Arc;

    use socketsweep_daemon::actions::LocalActions;
    use socketsweep_daemon::fs_rustix::RustixFs;
    use socketsweep_daemon::session::{Services, SessionConfig};
    use socketsweep_daemon::{listener, log};

    let mut cfg = SessionConfig::new(token);
    if let Some(n) = args.threads {
        cfg.workers = n.min(cfg.limits.max_workers);
    }
    let services = Services {
        fs: Arc::new(RustixFs),
        actions: Arc::new(LocalActions::new(&args.root)),
    };
    let sock = match listener::bind(&args.socket) {
        Ok(l) => l,
        Err(e) => {
            log!("cannot bind abstract socket {:?}: {e}", args.socket);
            return ExitCode::FAILURE;
        }
    };
    log!(
        "listening on abstract:{} (root {}, protocol {}, {} workers, pid {})",
        args.socket,
        args.root,
        socketsweep_wire::PROTOCOL_VERSION,
        cfg.workers,
        std::process::id()
    );
    listener::serve(sock, cfg, services);
    log!("shutdown");
    ExitCode::SUCCESS
}

// Abstract sockets are a Linux-family feature. Other hosts still build the
// binary so `cargo clippy` and the argument tests run during development.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn run(_args: Args, _token: socketsweep_wire::Token) -> ExitCode {
    eprintln!("[daemon] socketsweep-daemon runs on Android/Linux only");
    ExitCode::from(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_host_launch_command_parses() {
        assert_eq!(
            parse(&[
                "--socket",
                "socketsweep-abc",
                "--root",
                "/storage/emulated/0"
            ]),
            Ok(Args {
                socket: "socketsweep-abc".into(),
                root: "/storage/emulated/0".into(),
                threads: None,
            })
        );
        assert_eq!(
            parse(&["--socket", "s", "--threads", "6"]).unwrap(),
            Args {
                socket: "s".into(),
                root: "/sdcard".into(),
                threads: Some(6),
            }
        );
    }

    #[test]
    fn bad_arguments_are_refused() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--root", "/sdcard"]).is_err());
        assert!(parse(&["--socket"]).is_err());
        assert!(parse(&["--socket", "s", "--threads", "0"]).is_err());
        assert!(parse(&["--socket", "s", "--threads", "x"]).is_err());
        assert!(parse(&["--socket", "s", "--token", "abc"]).is_err());
    }
}

//! Pushing the fixture to a phone and measuring what is there.

use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use socketsweep_sim::SimFs;
use socketsweep_transport::Transport;

use super::{build, write_tar, DEVICE_TAR, FIXTURE_DIR};

/// Build the fixture for `seed`, write it to `tar`, push it to `serial` and
/// extract it under `parent` (replacing any earlier fixture there). Returns
/// the tree and the archive size.
pub fn push(
    transport: &dyn Transport,
    serial: &str,
    seed: u64,
    parent: &str,
    tar: &Path,
) -> Result<(SimFs, u64), String> {
    let root = format!("{parent}/{FIXTURE_DIR}");
    let fs = build(seed, root.as_bytes());
    let file = std::fs::File::create(tar).map_err(|e| format!("{}: {e}", tar.display()))?;
    let mut w = io::BufWriter::new(file);
    let size = write_tar(&fs, seed, &mut w)
        .and_then(|n| w.flush().map(|()| n))
        .map_err(|e| format!("writing {}: {e}", tar.display()))?;
    drop(w);
    transport
        .push(serial, tar, DEVICE_TAR, 0o644)
        .map_err(|e| format!("pushing the fixture: {e}"))?;
    run_detached(
        transport,
        serial,
        "extracting the fixture",
        &format!("rm -rf '{root}' && cd '{parent}' && tar -xf {DEVICE_TAR}"),
    )?;
    transport
        .shell(serial, &format!("rm -f {DEVICE_TAR}"))
        .map_err(|e| format!("removing the pushed archive: {e}"))?;
    Ok((fs, size))
}

/// Files a detached job leaves behind: its exit status, stdout and stderr.
const JOB_DONE: &str = "/data/local/tmp/ssbench.done";
const JOB_OUT: &str = "/data/local/tmp/ssbench.out";
const JOB_LOG: &str = "/data/local/tmp/ssbench.log";

/// How long a detached job may take. Tens of thousands of files through
/// `/sdcard`'s FUSE layer take minutes on an emulator.
const JOB_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// Run `script` on the phone and return its stdout.
///
/// A shell call that stays silent longer than the transport's read timeout is
/// treated as a lost device, and `tar` or `find -exec … +` over a large tree
/// can be silent for minutes. So the script runs detached, writing its output
/// and exit status to files, and is polled for. `script` must not contain
/// double quotes.
fn run_detached(
    transport: &dyn Transport,
    serial: &str,
    what: &str,
    script: &str,
) -> Result<String, String> {
    let launch = format!(
        "rm -f {JOB_DONE} {JOB_DONE}.tmp {JOB_OUT} {JOB_LOG}; \
         nohup sh -c \"{script}; echo \\$? > {JOB_DONE}.tmp; mv {JOB_DONE}.tmp {JOB_DONE}\" \
         </dev/null >{JOB_OUT} 2>{JOB_LOG} &"
    );
    let out = transport
        .shell(serial, &launch)
        .map_err(|e| format!("{what}: starting: {e}"))?;
    if !out.success() {
        return Err(format!(
            "{what}: could not start ({:?}): {}",
            out.exit,
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let deadline = Instant::now() + JOB_DEADLINE;
    let status = loop {
        std::thread::sleep(Duration::from_secs(1));
        let status = transport
            .shell(serial, &format!("cat {JOB_DONE} 2>/dev/null"))
            .map_err(|e| format!("{what}: waiting: {e}"))?
            .stdout_text();
        let status = status.trim().to_string();
        if !status.is_empty() {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{what}: took longer than {} minutes",
                JOB_DEADLINE.as_secs() / 60
            ));
        }
    };

    let read = |file: &str| {
        transport
            .shell(serial, &format!("cat {file} 2>/dev/null"))
            .map(|o| o.stdout_text())
            .map_err(|e| format!("{what}: reading its output: {e}"))
    };
    let stdout = read(JOB_OUT)?;
    let stderr = read(JOB_LOG)?;
    transport
        .shell(serial, &format!("rm -f {JOB_DONE} {JOB_OUT} {JOB_LOG}"))
        .ok();
    if status != "0" {
        return Err(format!("{what}: failed (exit {status}): {}", stderr.trim()));
    }
    Ok(stdout)
}

/// What is under `path` on the phone, by `find` and `stat`: (files, dirs,
/// apparent bytes of everything below, file bytes). Slow on big trees; for
/// checks.
pub fn measure_on_device(
    transport: &dyn Transport,
    serial: &str,
    path: &str,
) -> Result<(u32, u32, u64, u64), String> {
    let out = run_detached(
        transport,
        serial,
        &format!("measuring {path}"),
        // xargs, not `-exec … +`: toybox's find does not keep its batches
        // under the argument limit on a tree this large.
        &format!("find '{path}' -mindepth 1 -print0 | xargs -0 stat -c '%F|%s'"),
    )?;
    let (mut files, mut dirs, mut bytes, mut file_bytes) = (0u32, 0u32, 0u64, 0u64);
    for line in out.lines() {
        let Some((kind, size)) = line.rsplit_once('|') else {
            continue;
        };
        let size: u64 = size
            .trim()
            .parse()
            .map_err(|_| format!("odd stat line {line:?}"))?;
        bytes += size;
        if kind == "directory" {
            dirs += 1;
        } else {
            files += 1;
            file_bytes += size;
        }
    }
    Ok((files, dirs, bytes, file_bytes))
}

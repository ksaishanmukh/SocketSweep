//! One scan of the benchmark: the clicks sent on time, the events recorded.

use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use socketsweep_host_core::Client;
use socketsweep_wire::{Outcome, ScanOpts};

use super::report::ms;
use super::{mode_name, ClickRecord, RunRecord, Script};
use crate::record::Recorder;

/// Per click: (seq, sent at).
type SentClicks = Vec<Option<(u32, Duration)>>;

/// Run one scan of `root` in `mode` under `script`, on `client`.
pub fn run_once(
    client: &Client,
    root: &[u8],
    opts: &ScanOpts,
    script: &Script,
    timeout: Duration,
) -> Result<(RunRecord, Recorder), String> {
    let (req, rx) = client
        .start_scan(root.to_vec(), opts.clone())
        .map_err(|e| format!("StartScan: {e}"))?;
    let t0 = Instant::now();
    let mut rec = Recorder::new(t0);
    let mut sent: SentClicks = vec![None; script.clicks.len()];
    let mut seq = 0u32;
    let deadline = t0 + timeout;
    while !rec.is_done() {
        let now = Instant::now();
        if now >= deadline {
            let _ = client.cancel(req);
            return Err(format!("the scan took longer than {timeout:?}"));
        }
        // Wake for the next click that is due and not yet sent.
        let next = script
            .clicks
            .iter()
            .zip(&sent)
            .filter(|(_, s)| s.is_none())
            .map(|(c, _)| t0 + c.at)
            .min();
        let wait = next
            .map_or(deadline, |n| n.min(deadline))
            .saturating_duration_since(now)
            .max(Duration::from_millis(1));
        match rx.recv_timeout(wait) {
            Ok(msg) => rec.on_msg(req, &msg, Instant::now())?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(client
                    .closed_reason()
                    .unwrap_or_else(|| "the scan's channel closed".into()))
            }
        }
        send_due_clicks(client, req, script, t0, &rec, &mut sent, &mut seq)?;
    }
    let end = rec.end.expect("done");
    if end.outcome != Outcome::Complete {
        return Err(format!("the scan ended {:?}", end.outcome));
    }
    let clicks = click_records(script, &sent, &rec);
    let record = run_record(opts, script, clicks, &rec);
    Ok((record, rec))
}

/// Focus every click that is due, whose folder is known, and not yet sent.
fn send_due_clicks(
    client: &Client,
    req: u32,
    script: &Script,
    t0: Instant,
    rec: &Recorder,
    sent: &mut SentClicks,
    seq: &mut u32,
) -> Result<(), String> {
    let now = Instant::now();
    for (c, s) in script.clicks.iter().zip(sent.iter_mut()) {
        if s.is_some() || now < t0 + c.at || rec.is_done() {
            continue;
        }
        if let Some(dir) = rec.dir(&c.path) {
            *seq += 1;
            client
                .focus(req, dir, *seq)
                .map_err(|e| format!("Focus: {e}"))?;
            *s = Some((*seq, rec.elapsed(Instant::now())));
        }
    }
    Ok(())
}

fn click_records(script: &Script, sent: &SentClicks, rec: &Recorder) -> Vec<ClickRecord> {
    script
        .clicks
        .iter()
        .zip(sent)
        .map(|(c, s)| {
            let dir = rec.dir(&c.path);
            let done = dir.and_then(|d| rec.done_at(d));
            ClickRecord {
                path: String::from_utf8_lossy(&c.path).into_owned(),
                at_ms: ms(c.at),
                sent_ms: s.map(|s| ms(s.1)),
                wait_ms: done.map(|d| ms(d.saturating_sub(c.at))),
                settled_ms: s
                    .and_then(|(seq, _)| rec.settled.get(&seq))
                    .map(|t| ms(t.saturating_sub(c.at))),
            }
        })
        .collect()
}

fn run_record(
    opts: &ScanOpts,
    script: &Script,
    clicks: Vec<ClickRecord>,
    rec: &Recorder,
) -> RunRecord {
    let end = rec.end.expect("done");
    RunRecord {
        script: script.kind.name(),
        mode: mode_name(opts.mode),
        run: 0,
        warmup: false,
        total_ms: ms(end.at),
        daemon_ms: end.elapsed_ms,
        first_listing_ms: rec.first_listing.map(ms),
        skeleton_ms: rec.skeleton.map(|s| ms(s.at)),
        skeleton_levels: rec.skeleton.map(|s| s.levels),
        skeleton_listed: rec.skeleton.map(|s| s.listed),
        clicks,
        opens: rec.listed + rec.deferred,
        listed: rec.listed,
        deferred: rec.deferred,
        stats: rec.entries,
        bytes: rec.bytes,
        frames: rec.frames,
        events: rec.events,
        files: end.totals.files,
        dirs: end.totals.dirs,
        apparent: end.totals.apparent,
        disk: end.totals.disk,
        errors: end.totals.errors,
    }
}

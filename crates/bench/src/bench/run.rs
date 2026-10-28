//! The whole benchmark: reference scan, warm-ups, then interleaved runs.

use socketsweep_host_core::Client;
use socketsweep_wire::{ScanMode, ScanOpts};

use super::report::ms;
use super::{
    mode_name, run_once, Config, Reference, Report, RunRecord, Script, ScriptKind, ScriptSetup,
};

const MODES: [ScanMode; 2] = [ScanMode::Focus, ScanMode::Plain];

fn opts_for(cfg: &Config, mode: ScanMode) -> ScanOpts {
    ScanOpts {
        workers: cfg.workers,
        mode,
        ..ScanOpts::default()
    }
}

/// Run the whole benchmark on `client`. `log` gets one line per scan.
pub fn run(
    client: &Client,
    device: &str,
    cfg: &Config,
    log: &mut dyn FnMut(&str),
) -> Result<Report, String> {
    let none = Script {
        kind: ScriptKind::A,
        clicks: Vec::new(),
    };
    // The reference scan: complete, so every folder's size is known.
    let (first, reference_scan) = run_once(
        client,
        &cfg.root,
        &opts_for(cfg, ScanMode::Focus),
        &none,
        cfg.timeout,
    )?;
    let reference = Reference::from_scan(&reference_scan);
    let root = String::from_utf8_lossy(reference_scan.root_path().unwrap_or(&cfg.root)).into();
    let mut records = vec![RunRecord {
        warmup: true,
        ..first
    }];
    log(&format!(
        "reference scan: {} files, {} dirs, {:.0} ms",
        records[0].files, records[0].dirs, records[0].total_ms
    ));
    let scripts: Vec<Script> = cfg
        .scripts
        .iter()
        .map(|&k| reference.script(k, cfg.seed))
        .collect();
    warm_up(client, cfg, &none, &mut records, log)?;
    measured_runs(client, cfg, &scripts, &mut records, log)?;
    Ok(build_report(client, device, cfg, root, &scripts, records))
}

fn warm_up(
    client: &Client,
    cfg: &Config,
    none: &Script,
    records: &mut Vec<RunRecord>,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    for w in 0..cfg.warmup {
        for &mode in &MODES {
            // The reference scan was the first focus warm-up.
            if w == 0 && mode == ScanMode::Focus {
                continue;
            }
            let (r, _) = run_once(client, &cfg.root, &opts_for(cfg, mode), none, cfg.timeout)?;
            log(&format!(
                "warm-up {}: {:.0} ms",
                mode_name(mode),
                r.total_ms
            ));
            records.push(RunRecord { warmup: true, ..r });
        }
    }
    Ok(())
}

fn measured_runs(
    client: &Client,
    cfg: &Config,
    scripts: &[Script],
    records: &mut Vec<RunRecord>,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    for i in 0..cfg.runs {
        for script in scripts {
            // Interleaved A/B, alternating which goes first.
            let order = if i % 2 == 0 {
                MODES
            } else {
                [MODES[1], MODES[0]]
            };
            for mode in order {
                let (r, _) =
                    run_once(client, &cfg.root, &opts_for(cfg, mode), script, cfg.timeout)?;
                log(&format!(
                    "run {}/{} script {} {}: {:.0} ms",
                    i + 1,
                    cfg.runs,
                    script.kind.name(),
                    mode_name(mode),
                    r.total_ms
                ));
                records.push(RunRecord { run: i + 1, ..r });
            }
        }
    }
    Ok(())
}

fn build_report(
    client: &Client,
    device: &str,
    cfg: &Config,
    root: String,
    scripts: &[Script],
    records: Vec<RunRecord>,
) -> Report {
    let summary = crate::summary::summarise(&records);
    let criteria = crate::summary::criteria(&records);
    Report {
        root,
        device: device.to_string(),
        daemon_build: client.welcome().daemon_build.clone(),
        workers: cfg.workers.unwrap_or(client.welcome().workers),
        runs: cfg.runs,
        seed: cfg.seed,
        scripts: scripts
            .iter()
            .map(|s| ScriptSetup {
                script: s.kind.name(),
                clicks: s
                    .clicks
                    .iter()
                    .map(|c| (ms(c.at), String::from_utf8_lossy(&c.path).into_owned()))
                    .collect(),
            })
            .collect(),
        records,
        summary,
        criteria,
    }
}

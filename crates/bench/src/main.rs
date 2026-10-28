//! `socketsweep-bench`: the command line for the benchmark, the fixture and the end-to-end test.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use socketsweep_bench::bench::{self, ScriptKind};
use socketsweep_bench::{device, e2e, fixture, summary};
use socketsweep_transport::Transport;

const USAGE: &str = "usage:
  socketsweep-bench [run] [options]    benchmark focus mode against plain mode
      --root <path>        scan root on the phone (default /sdcard)
      --runs <n>           measured runs per script and mode (default 10)
      --warmup <n>         unmeasured runs per mode first (default 1)
      --scripts <abc>      which scripts (default abc)
      --workers <n>        workers per scan (default: the daemon's)
      --seed <n>           seed of script (c) (default 1)
      --json <file>        where to write the results (default socketsweep-bench.json)
      --timeout <secs>     longest one scan may take (default 600)
  socketsweep-bench fixture [options]  build the seeded fixture and push it
      --seed <n>           (default 1)
      --parent <path>      extract it under this folder (default /sdcard)
      --out <file.tar>     where to write the archive (default: a temp file)
      --no-push            only write the archive
  socketsweep-bench e2e [options]      the end-to-end test through host-core's App
      --seed <n>           fixture seed (default 1)
      --parent <path>      storage root holding the fixture (default /sdcard)
      --no-push            the fixture is already on the phone (same seed)
      --work <dir>         scratch folder on this computer (default: a temp dir)
      --timeout <secs>     longest one step may take (default 600)
common options:
  --serial <serial>        the device (default: the only one attached)
  --daemon <path>          daemon build for the device's ABI (default: src-tauri/bin)
  --adb <path>             adb used to start a server if none runs
  --confine <root>         the daemon's --root, for run (default /sdcard)";

#[derive(Debug, Default)]
struct Args {
    cmd: String,
    serial: Option<String>,
    daemon: Option<PathBuf>,
    adb: Option<PathBuf>,
    confine: Option<String>,
    root: Option<String>,
    runs: Option<usize>,
    warmup: Option<usize>,
    scripts: Option<String>,
    workers: Option<u8>,
    seed: Option<u64>,
    json: Option<PathBuf>,
    timeout: Option<u64>,
    parent: Option<String>,
    out: Option<PathBuf>,
    work: Option<PathBuf>,
    no_push: bool,
}

fn num<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("{flag}: not a number: {v:?}"))
}

/// `Err("")` asks for the usage text.
fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut a = Args {
        cmd: "run".into(),
        ..Args::default()
    };
    let mut it = argv.into_iter().peekable();
    if it.peek().is_some_and(|first| !first.starts_with('-')) {
        a.cmd = it.next().unwrap_or_default();
    }
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--no-push" => {
                a.no_push = true;
                continue;
            }
            "--help" | "-h" => return Err(String::new()),
            _ => {}
        }
        let v = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--serial" => a.serial = Some(v),
            "--daemon" => a.daemon = Some(v.into()),
            "--adb" => a.adb = Some(v.into()),
            "--confine" => a.confine = Some(v),
            "--root" => a.root = Some(v),
            "--runs" => a.runs = Some(num(&flag, &v)?),
            "--warmup" => a.warmup = Some(num(&flag, &v)?),
            "--scripts" => a.scripts = Some(v),
            "--workers" => a.workers = Some(num(&flag, &v)?),
            "--seed" => a.seed = Some(num(&flag, &v)?),
            "--json" => a.json = Some(v.into()),
            "--timeout" => a.timeout = Some(num(&flag, &v)?),
            "--parent" => a.parent = Some(v),
            "--out" => a.out = Some(v.into()),
            "--work" => a.work = Some(v.into()),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(a)
}

fn log(line: &str) {
    eprintln!("[bench] {line}");
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("socketsweep-bench-{}-{name}", std::process::id()))
}

fn run_bench(a: &Args) -> Result<(), String> {
    let transport = device::transport(a.adb.clone())?;
    let serial = device::serial(transport.as_ref(), a.serial.clone())?;
    let info = transport.device_info(&serial).map_err(|e| e.to_string())?;
    let mut cfg = bench::Config::default();
    if let Some(r) = &a.root {
        cfg.root = r.as_bytes().to_vec();
    }
    cfg.runs = a.runs.unwrap_or(cfg.runs);
    cfg.warmup = a.warmup.unwrap_or(cfg.warmup);
    cfg.workers = a.workers;
    cfg.seed = a.seed.unwrap_or(cfg.seed);
    if let Some(t) = a.timeout {
        cfg.timeout = Duration::from_secs(t);
    }
    if let Some(s) = &a.scripts {
        cfg.scripts = s
            .chars()
            .map(|c| {
                ScriptKind::parse(&c.to_string()).ok_or_else(|| format!("unknown script {c:?}"))
            })
            .collect::<Result<_, _>>()?;
    }
    let daemon = device::start_daemon(
        transport.as_ref(),
        &serial,
        a.confine.as_deref(),
        a.daemon.clone(),
    )?;
    let name = format!("{} ({serial}, API {}, {})", info.model, info.sdk, info.abi);
    log(&format!(
        "daemon {} on {name}, {} workers by default",
        daemon.client.welcome().daemon_build,
        daemon.client.welcome().workers
    ));
    let report = bench::run(&daemon.client, &name, &cfg, &mut |l| log(l));
    daemon.stop(transport.as_ref());
    let report = report?;
    println!("{}", summary::table(&report.summary, &report.criteria));
    let json = a
        .json
        .clone()
        .unwrap_or_else(|| PathBuf::from("socketsweep-bench.json"));
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&json, text).map_err(|e| format!("{}: {e}", json.display()))?;
    log(&format!("wrote {}", json.display()));
    Ok(())
}

fn run_fixture(a: &Args) -> Result<(), String> {
    let seed = a.seed.unwrap_or(1);
    let parent = a.parent.clone().unwrap_or_else(|| "/sdcard".into());
    let out = a.out.clone().unwrap_or_else(|| scratch("ssfixture.tar"));
    if a.no_push {
        let root = format!("{parent}/{}", fixture::FIXTURE_DIR);
        let fs = fixture::build(seed, root.as_bytes());
        let file = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let mut w = std::io::BufWriter::new(file);
        let n = fixture::write_tar(&fs, seed, &mut w)
            .and_then(|n| std::io::Write::flush(&mut w).map(|()| n))
            .map_err(|e| e.to_string())?;
        log(&format!("wrote {} ({} MiB)", out.display(), n >> 20));
        return Ok(());
    }
    let transport = device::transport(a.adb.clone())?;
    let serial = device::serial(transport.as_ref(), a.serial.clone())?;
    let started = std::time::Instant::now();
    let pushed = fixture::push(transport.as_ref(), &serial, seed, &parent, &out);
    if a.out.is_none() {
        let _ = std::fs::remove_file(&out);
    }
    let (fs, size) = pushed?;
    let e = fixture::expected(&fs);
    log(&format!(
        "fixture seed {seed} at {parent}/{}: {} files, {} dirs, {} file bytes ({} MiB archive, {:.1} s)",
        fixture::FIXTURE_DIR,
        e.files,
        e.dirs,
        e.file_bytes,
        size >> 20,
        started.elapsed().as_secs_f64()
    ));
    Ok(())
}

fn run_e2e(a: &Args) -> Result<(), String> {
    let transport = device::transport(a.adb.clone())?;
    let serial = device::serial(transport.as_ref(), a.serial.clone())?;
    let work = a.work.clone().unwrap_or_else(|| scratch("e2e"));
    let cfg = e2e::Config {
        serial,
        seed: a.seed.unwrap_or(1),
        parent: a.parent.clone().unwrap_or_else(|| "/sdcard".into()),
        push: !a.no_push,
        daemon: a.daemon.clone(),
        work: work.clone(),
        timeout: Duration::from_secs(a.timeout.unwrap_or(600)),
    };
    let transport: Arc<dyn Transport> = transport;
    let r = e2e::run(transport, &cfg, &mut |l| log(l));
    if a.work.is_none() {
        let _ = std::fs::remove_dir_all(&work);
    }
    r?;
    log("e2e: PASS");
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1).collect()) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let r = match args.cmd.as_str() {
        "run" => run_bench(&args),
        "fixture" => run_fixture(&args),
        "e2e" => run_e2e(&args),
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[bench] error: {e}");
            ExitCode::FAILURE
        }
    }
}

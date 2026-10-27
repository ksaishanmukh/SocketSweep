//! Copy to this computer: map a [`CopyManifest`](socketsweep_wire::ServerMsg::CopyManifest)
//! onto local paths, then pull every file with [`Transport::pull`].
//!
//! # Names
//!
//! Android names are arbitrary bytes; Windows (and to a lesser degree macOS)
//! refuse many of them. Every component goes through [`host_safe_name`], the
//! same mapping on every platform so a copy looks the same everywhere:
//!
//! - invalid UTF-8 becomes U+FFFD;
//! - `< > : " / \ | ? *` and control characters become `_`;
//! - trailing dots and spaces, which Windows silently drops, become `_`;
//! - reserved device names (`CON`, `nul.txt`, `COM1`, …) get a leading `_`;
//! - an empty name becomes `_`.
//!
//! Two names that map to the same result, or differ only in case (Windows and
//! macOS compare names case-insensitively), are told apart with ` (2)`,
//! ` (3)`, … before the extension. Each changed name counts as renamed.
//!
//! # Pulling
//!
//! Directories are created first, then files are pulled by [`COPY_WORKERS`]
//! threads, as the spec's 2–3 parallel sync connections. Cancelling stops
//! workers taking new files; a file part-way through finishes (the transport
//! cannot be interrupted), and files already copied stay. A failed pull's
//! partial file is removed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use socketsweep_transport::{Transport, TransportError};
use socketsweep_wire::{CopyItem, Kind};

use crate::lock;
use crate::tree::basename;

/// Files pulled at once.
pub const COPY_WORKERS: usize = 3;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// `name` as a valid file name on any desktop, and whether it changed.
pub fn host_safe_name(name: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(name);
    let mut out: String = text
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 || c == '\u{7f}' => '_',
            c => c,
        })
        .collect();
    let kept = out.trim_end_matches(['.', ' ']).len();
    let trailing = out.len() - kept;
    out.truncate(kept);
    out.extend(std::iter::repeat_n('_', trailing));
    if out.is_empty() {
        out.push('_');
    }
    let stem = out.split('.').next().unwrap_or("").trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        out.insert(0, '_');
    }
    let changed = out.as_bytes() != name;
    (out, changed)
}

/// `name` with ` (n)` before its extension.
fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

/// The names already used in one local directory, compared without case.
#[derive(Default)]
struct UsedNames(HashSet<String>);

impl UsedNames {
    /// `name`, or the first free numbered variant of it, now claimed.
    fn claim(&mut self, name: &str) -> (String, bool) {
        if self.0.insert(name.to_lowercase()) {
            return (name.to_string(), false);
        }
        (2..)
            .map(|n| numbered(name, n))
            .find(|candidate| self.0.insert(candidate.to_lowercase()))
            .map(|c| (c, true))
            .unwrap_or_else(|| (name.to_string(), true))
    }
}

/// One thing to create or pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedItem {
    /// The phone path.
    pub remote: Vec<u8>,
    pub local: PathBuf,
    pub kind: Kind,
    pub size: u64,
    pub mtime: i64,
    /// Relative to what was asked for, for display.
    pub rel: String,
}

/// A manifest mapped onto this computer.
#[derive(Debug, Clone, Default)]
pub struct CopyPlan {
    /// Where the copy lands: the copied folder or file itself.
    pub top: PathBuf,
    /// Directories, parents before children.
    pub dirs: Vec<PlannedItem>,
    pub files: Vec<PlannedItem>,
    /// Symbolic links and special files, not copied.
    pub skipped: u32,
    /// Items stored under a changed name.
    pub renamed: u32,
    pub bytes: u64,
}

/// Map the manifest of `root` onto `dest_dir`. `exists` says whether a local
/// path is taken, so the copy never overwrites what is already there.
pub fn plan(
    root: &[u8],
    items: &[CopyItem],
    dest_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> CopyPlan {
    let mut plan = CopyPlan::default();

    let (top_name, mut renamed_top) = host_safe_name(basename(root));
    let mut top_name_final = top_name.clone();
    let mut n = 2;
    while exists(&dest_dir.join(&top_name_final)) {
        top_name_final = numbered(&top_name, n);
        renamed_top = true;
        n += 1;
    }
    plan.top = dest_dir.join(&top_name_final);

    // Remote relative directory -> its local path; and per local directory,
    // the names used in it.
    let mut dirs: HashMap<Vec<u8>, PathBuf> = HashMap::new();
    dirs.insert(Vec::new(), plan.top.clone());
    let mut used: HashMap<PathBuf, UsedNames> = HashMap::new();

    for item in items {
        let remote = join_remote(root, &item.rel);
        let rel_display = String::from_utf8_lossy(&item.rel).into_owned();
        let (local, renamed) = if item.rel.is_empty() {
            (plan.top.clone(), renamed_top)
        } else {
            local_path(&item.rel, &mut dirs, &mut used)
        };
        if renamed {
            plan.renamed += 1;
        }
        let planned = PlannedItem {
            remote,
            local: local.clone(),
            kind: item.kind,
            size: item.size,
            mtime: item.mtime,
            rel: rel_display,
        };
        match item.kind {
            Kind::Dir => {
                dirs.insert(item.rel.clone(), local);
                plan.dirs.push(planned);
            }
            Kind::File => {
                plan.bytes += item.size;
                plan.files.push(planned);
            }
            Kind::Symlink | Kind::Other => plan.skipped += 1,
        }
    }
    plan.dirs.sort_by_key(|d| d.local.components().count());
    plan
}

fn join_remote(root: &[u8], rel: &[u8]) -> Vec<u8> {
    let mut out = root.to_vec();
    if !rel.is_empty() {
        if out.last() != Some(&b'/') {
            out.push(b'/');
        }
        out.extend_from_slice(rel);
    }
    out
}

/// The local path for remote `rel`, mapping each new component.
fn local_path(
    rel: &[u8],
    dirs: &mut HashMap<Vec<u8>, PathBuf>,
    used: &mut HashMap<PathBuf, UsedNames>,
) -> (PathBuf, bool) {
    let (parent_rel, name) = match rel.iter().rposition(|b| *b == b'/') {
        Some(slash) => (&rel[..slash], &rel[slash + 1..]),
        None => (&rel[..0], rel),
    };
    let parent = match dirs.get(parent_rel) {
        Some(p) => p.clone(),
        None => {
            // A directory the manifest did not list before its contents.
            let (p, _) = local_path(parent_rel, dirs, used);
            dirs.insert(parent_rel.to_vec(), p.clone());
            p
        }
    };
    let (safe, changed) = host_safe_name(name);
    let (unique, numbered) = used.entry(parent.clone()).or_default().claim(&safe);
    (parent.join(unique), changed || numbered)
}

/// Progress of a running copy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyTick {
    pub files_done: u32,
    pub bytes_done: u64,
    pub current: Option<String>,
}

/// Why a copy stopped.
#[derive(Debug)]
pub enum CopyError {
    Cancelled,
    /// A local file or folder could not be written.
    Local {
        path: PathBuf,
        source: std::io::Error,
    },
    Transport(TransportError),
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyError::Cancelled => write!(f, "The copy was cancelled."),
            CopyError::Local { path, source } => {
                write!(f, "Cannot write {}: {source}", path.display())
            }
            CopyError::Transport(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CopyError {}

/// Create the plan's directories and pull its files from `serial`.
/// `progress` is called from the worker threads.
pub fn execute(
    transport: &dyn Transport,
    serial: &str,
    plan: &CopyPlan,
    cancel: &AtomicBool,
    progress: &(dyn Fn(CopyTick) + Sync),
) -> Result<CopyTick, CopyError> {
    let local_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| CopyError::Local { path, source }
    };
    for dir in &plan.dirs {
        std::fs::create_dir_all(&dir.local).map_err(local_err(&dir.local))?;
    }

    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let failure: Mutex<Option<CopyError>> = Mutex::new(None);
    let tick = Mutex::new(CopyTick::default());

    std::thread::scope(|scope| {
        for _ in 0..COPY_WORKERS.min(plan.files.len()) {
            scope.spawn(|| loop {
                if stop.load(Ordering::SeqCst) || cancel.load(Ordering::SeqCst) {
                    return;
                }
                let Some(file) = plan.files.get(next.fetch_add(1, Ordering::SeqCst)) else {
                    return;
                };
                if let Some(parent) = file.local.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        stop.store(true, Ordering::SeqCst);
                        lock(&failure).get_or_insert(local_err(parent)(e));
                        return;
                    }
                }
                {
                    let mut t = lock(&tick);
                    t.current = Some(file.rel.clone());
                    progress(t.clone());
                }
                let mut last = 0u64;
                let pulled = transport.pull(serial, &file.remote, &file.local, &mut |done| {
                    let delta = done.saturating_sub(last);
                    last = done;
                    let mut t = lock(&tick);
                    t.bytes_done += delta;
                    progress(t.clone());
                });
                match pulled {
                    Ok(_) => {
                        set_mtime(&file.local, file.mtime);
                        let mut t = lock(&tick);
                        t.files_done += 1;
                        progress(t.clone());
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&file.local);
                        stop.store(true, Ordering::SeqCst);
                        lock(&failure).get_or_insert(CopyError::Transport(e));
                        return;
                    }
                }
            });
        }
    });

    if let Some(e) = lock(&failure).take() {
        return Err(e);
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(CopyError::Cancelled);
    }
    let mut done = lock(&tick).clone();
    done.current = None;
    Ok(done)
}

/// Best effort: keep the phone's modification time.
fn set_mtime(path: &Path, mtime: i64) {
    let Ok(secs) = u64::try_from(mtime) else {
        return;
    };
    let when = UNIX_EPOCH + Duration::from_secs(secs);
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(when.min(SystemTime::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use socketsweep_transport::fake::{FakeDevice, FakeOp, FakeTransport};

    fn safe(name: &[u8]) -> String {
        host_safe_name(name).0
    }

    #[test]
    fn ordinary_names_are_kept() {
        assert_eq!(
            host_safe_name(b"IMG_0001.jpg"),
            ("IMG_0001.jpg".into(), false)
        );
        assert_eq!(
            host_safe_name("Café ☕.txt".as_bytes()),
            ("Café ☕.txt".into(), false)
        );
        assert_eq!(safe(b".nomedia"), ".nomedia");
    }

    #[test]
    fn invalid_utf8_is_replaced() {
        let (name, changed) = host_safe_name(b"pic\xFF.jpg");
        assert_eq!(name, "pic\u{FFFD}.jpg");
        assert!(changed);
    }

    #[test]
    fn windows_reserved_characters_and_controls_are_replaced() {
        assert_eq!(safe(b"a<b>c:d\"e|f?g*h\\i"), "a_b_c_d_e_f_g_h_i");
        assert_eq!(safe(b"tab\there"), "tab_here");
        assert_eq!(safe(b"what?.mp3"), "what_.mp3");
    }

    #[test]
    fn trailing_dots_and_spaces_are_kept_visible() {
        assert_eq!(safe(b"name. "), "name__");
        assert_eq!(safe(b"."), "_");
        assert_eq!(safe(b".."), "__");
        assert_eq!(safe(b""), "_");
    }

    #[test]
    fn reserved_device_names_are_prefixed() {
        for name in ["CON", "con", "nul.txt", "Com1.log", "LPT9", "aux .x"] {
            let (out, changed) = host_safe_name(name.as_bytes());
            assert!(out.starts_with('_'), "{name} -> {out}");
            assert!(changed);
        }
        assert_eq!(safe(b"CONSOLE.txt"), "CONSOLE.txt");
        assert_eq!(safe(b"COM10"), "COM10");
    }

    #[test]
    fn colliding_names_are_numbered_case_insensitively() {
        let mut used = UsedNames::default();
        assert_eq!(used.claim("a.txt"), ("a.txt".into(), false));
        assert_eq!(used.claim("A.TXT"), ("A (2).TXT".into(), true));
        assert_eq!(used.claim("a.txt"), ("a (3).txt".into(), true));
        assert_eq!(numbered("noext", 2), "noext (2)");
        assert_eq!(numbered(".hidden", 2), ".hidden (2)");
    }

    fn item(rel: &[u8], kind: Kind, size: u64) -> CopyItem {
        CopyItem {
            rel: rel.to_vec(),
            kind,
            size,
            mode: 0o644,
            mtime: 1_700_000_000,
        }
    }

    #[test]
    fn a_manifest_is_planned_without_overwriting_or_collisions() {
        let items = [
            item(b"", Kind::Dir, 0),
            item(b"sub", Kind::Dir, 0),
            item(b"sub/a?.jpg", Kind::File, 3),
            item(b"sub/a_.jpg", Kind::File, 4),
            item(b"link", Kind::Symlink, 0),
            item(b"x\xFF", Kind::File, 5),
        ];
        let dest = Path::new("/dest");
        let taken = |p: &Path| p == Path::new("/dest/Camera");
        let plan = plan(b"/storage/emulated/0/DCIM/Camera", &items, dest, &taken);
        assert_eq!(plan.top, Path::new("/dest/Camera (2)"));
        assert_eq!(plan.dirs.len(), 2);
        assert_eq!(plan.skipped, 1);
        assert_eq!(plan.bytes, 12);
        let locals: Vec<_> = plan.files.iter().map(|f| f.local.clone()).collect();
        let top = Path::new("/dest/Camera (2)");
        assert_eq!(
            locals,
            [
                top.join("sub").join("a_.jpg"),
                top.join("sub").join("a_ (2).jpg"),
                top.join("x\u{FFFD}"),
            ]
        );
        // The top folder, a?.jpg, a_.jpg (numbered) and x\xFF.
        assert_eq!(plan.renamed, 4);
        assert_eq!(
            plan.files[0].remote,
            b"/storage/emulated/0/DCIM/Camera/sub/a?.jpg"
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socketsweep-copy-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn files_are_pulled_into_place_with_progress() {
        let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
        let root = b"/sdcard/Music";
        let mut items = vec![item(b"", Kind::Dir, 0), item(b"deep", Kind::Dir, 0)];
        for i in 0..7u8 {
            let rel = format!("deep/t{i}.mp3").into_bytes();
            t.put_file(
                "P1",
                &join_remote(root, &rel),
                vec![i; 100 + i as usize],
                0o644,
            );
            items.push(item(&rel, Kind::File, 100 + i as u64));
        }
        let dest = scratch("pull");
        let plan = plan(root, &items, &dest, &|p| p.exists());
        let ticks = Mutex::new(Vec::new());
        let done = execute(&t, "P1", &plan, &AtomicBool::new(false), &|t| {
            lock(&ticks).push(t)
        })
        .unwrap();
        assert_eq!(done.files_done, 7);
        assert_eq!(done.bytes_done, plan.bytes);
        assert_eq!(
            std::fs::read(dest.join("Music").join("deep").join("t3.mp3")).unwrap(),
            vec![3; 103]
        );
        assert!(lock(&ticks).iter().any(|t| t.current.is_some()));
        let _ = std::fs::remove_dir_all(dest);
    }

    #[test]
    fn a_failed_pull_stops_the_copy_and_leaves_no_partial_file() {
        let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
        t.fail_next(FakeOp::Pull, TransportError::Refused("gone".into()));
        let dest = scratch("fail");
        let items = [item(b"", Kind::File, 1)];
        let plan = plan(b"/sdcard/a.txt", &items, &dest, &|p| p.exists());
        let err = execute(&t, "P1", &plan, &AtomicBool::new(false), &|_| {}).unwrap_err();
        assert!(matches!(err, CopyError::Transport(_)));
        assert!(!dest.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(dest);
    }

    #[test]
    fn a_cancelled_copy_takes_no_new_files() {
        let t = FakeTransport::new().with_device(FakeDevice::ready("P1"));
        t.put_file("P1", b"/sdcard/d/a", vec![1], 0o644);
        let dest = scratch("cancel");
        let items = [item(b"", Kind::Dir, 0), item(b"a", Kind::File, 1)];
        let plan = plan(b"/sdcard/d", &items, &dest, &|p| p.exists());
        let err = execute(&t, "P1", &plan, &AtomicBool::new(true), &|_| {}).unwrap_err();
        assert!(matches!(err, CopyError::Cancelled));
        assert!(!dest.join("d").join("a").exists());
        let _ = std::fs::remove_dir_all(dest);
    }
}

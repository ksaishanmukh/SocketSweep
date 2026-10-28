//! Host-safe file names, and telling colliding ones apart.

use std::collections::HashSet;

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
pub(super) fn numbered(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

/// The names already used in one local directory, compared without case.
#[derive(Default)]
pub(super) struct UsedNames(HashSet<String>);

impl UsedNames {
    /// `name`, or the first free numbered variant of it, now claimed.
    pub(super) fn claim(&mut self, name: &str) -> (String, bool) {
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

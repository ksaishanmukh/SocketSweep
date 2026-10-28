//! The line format printed by the on-phone app helper (`android-helper/`).
//!
//! The helper is not part of the framed protocol: it is a one-shot Java
//! program the host runs through `adb shell`, and this module only parses what
//! it prints. The output is UTF-8, one JSON object per line:
//!
//! ```text
//! {"v":1}
//! {"pkg":"com.whatsapp","label":"WhatsApp","system":false,"icon":"<base64 png>"}
//! ```
//!
//! The first line carries the format version; every later line is one app,
//! with `icon` present only for packages whose icon was requested and could be
//! drawn. Unknown fields are ignored, so the helper can add fields without
//! breaking an older host; anything incompatible must bump
//! [`HELPER_FORMAT_VERSION`].
//!
//! `serde_json` and `base64` are ordinary dependencies of this crate rather
//! than a feature: the daemon links the crate too, but with LTO it drops code
//! it never calls, and a feature would have to be switched on by every host
//! crate separately.

use std::fmt;

use base64::Engine;
use serde::Deserialize;

/// The `v` the helper prints on its first line, and the only one accepted.
pub const HELPER_FORMAT_VERSION: u32 = 1;

/// One installed app as reported by the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    /// Android package name, e.g. `com.whatsapp`.
    pub pkg: String,
    /// The label the launcher shows, in the phone's language.
    pub label: String,
    /// Part of the system image, including updated system apps.
    pub system: bool,
    /// PNG bytes, decoded from the helper's base64. `None` when no icon was
    /// requested, it could not be drawn, or its base64 was malformed: a bad
    /// icon costs the icon, not the listing.
    pub icon_png: Option<Vec<u8>>,
}

/// Why helper output could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppsError {
    /// No lines at all: the helper printed nothing.
    Empty,
    /// The first line is not a `{"v":N}` object.
    BadHeader(String),
    /// The helper speaks a format this host does not.
    UnsupportedVersion(u32),
    /// A line after the header is not an app object. `line` is 1-based.
    BadLine { line: usize, reason: String },
}

impl fmt::Display for AppsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "app helper printed nothing"),
            Self::BadHeader(why) => write!(f, "app helper header is not a version line: {why}"),
            Self::UnsupportedVersion(v) => write!(
                f,
                "app helper format version {v} is not supported (expected {HELPER_FORMAT_VERSION})"
            ),
            Self::BadLine { line, reason } => {
                write!(f, "app helper line {line} is not an app: {reason}")
            }
        }
    }
}

impl std::error::Error for AppsError {}

#[derive(Deserialize)]
struct Header {
    v: u32,
}

#[derive(Deserialize)]
struct Line {
    pkg: String,
    label: String,
    #[serde(default)]
    system: bool,
    #[serde(default)]
    icon: Option<String>,
}

/// Parse the helper's whole stdout.
///
/// Blank lines are skipped and a trailing `\r` (adb on older phones converts
/// line endings) is tolerated. A malformed line fails the whole parse rather
/// than silently dropping an app.
pub fn parse_apps(stdout: &str) -> Result<Vec<AppInfo>, AppsError> {
    let mut lines = stdout
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty());

    let (_, first) = lines.next().ok_or(AppsError::Empty)?;
    let header: Header =
        serde_json::from_str(first).map_err(|e| AppsError::BadHeader(e.to_string()))?;
    if header.v != HELPER_FORMAT_VERSION {
        return Err(AppsError::UnsupportedVersion(header.v));
    }

    lines
        .map(|(line, text)| {
            let parsed: Line = serde_json::from_str(text).map_err(|e| AppsError::BadLine {
                line,
                reason: e.to_string(),
            })?;
            Ok(AppInfo {
                pkg: parsed.pkg,
                label: parsed.label,
                system: parsed.system,
                icon_png: parsed.icon.and_then(|b64| decode_icon(&b64)),
            })
        })
        .collect()
}

fn decode_icon(b64: &str) -> Option<Vec<u8>> {
    let png = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    (!png.is_empty()).then_some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of a PNG signature; 7 bytes, so the base64 needs padding.
    const ICON_BYTES: &[u8] = b"\x89PNG\r\n\x1a";
    const ICON_B64: &str = "iVBORw0KGg==";

    #[test]
    fn parses_golden_output() {
        let out = format!(
            "{{\"v\":1}}\n\
             {{\"pkg\":\"com.whatsapp\",\"label\":\"WhatsApp\",\"system\":false,\"icon\":\"{}\"}}\n\
             {{\"pkg\":\"com.android.settings\",\"label\":\"Settings\",\"system\":true}}\n",
            ICON_B64
        );
        let apps = parse_apps(&out).unwrap();
        assert_eq!(
            apps,
            vec![
                AppInfo {
                    pkg: "com.whatsapp".into(),
                    label: "WhatsApp".into(),
                    system: false,
                    icon_png: Some(ICON_BYTES.to_vec()),
                },
                AppInfo {
                    pkg: "com.android.settings".into(),
                    label: "Settings".into(),
                    system: true,
                    icon_png: None,
                },
            ]
        );
    }

    #[test]
    fn keeps_non_ascii_and_escaped_labels() {
        let out = "{\"v\":1}\n\
            {\"pkg\":\"jp.line\",\"label\":\"ラインメッセンジャー\",\"system\":false}\n\
            {\"pkg\":\"x.emoji\",\"label\":\"Fun 🎉 \\\"app\\\"\\n2\",\"system\":false}\n";
        let apps = parse_apps(out).unwrap();
        assert_eq!(apps[0].label, "ラインメッセンジャー");
        assert_eq!(apps[1].label, "Fun 🎉 \"app\"\n2");
    }

    #[test]
    fn skips_blank_lines_and_tolerates_crlf_and_unknown_fields() {
        let out = "\r\n{\"v\":1}\r\n\r\n\
            {\"pkg\":\"a.b\",\"label\":\"AB\",\"system\":false,\"future\":[1,2]}\r\n\r\n";
        let apps = parse_apps(out).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].pkg, "a.b");
    }

    #[test]
    fn header_only_is_an_empty_list() {
        assert_eq!(parse_apps("{\"v\":1}\n").unwrap(), vec![]);
    }

    #[test]
    fn system_defaults_to_false_when_absent() {
        let apps = parse_apps("{\"v\":1}\n{\"pkg\":\"a\",\"label\":\"A\"}\n").unwrap();
        assert!(!apps[0].system);
    }

    #[test]
    fn bad_icon_base64_drops_only_the_icon() {
        let apps =
            parse_apps("{\"v\":1}\n{\"pkg\":\"a\",\"label\":\"A\",\"icon\":\"!!\"}\n").unwrap();
        assert_eq!(apps[0].pkg, "a");
        assert_eq!(apps[0].icon_png, None);
    }

    #[test]
    fn empty_output_is_an_error() {
        assert_eq!(parse_apps(""), Err(AppsError::Empty));
        assert_eq!(parse_apps("\n  \n"), Err(AppsError::Empty));
    }

    #[test]
    fn wrong_version_is_refused() {
        assert_eq!(
            parse_apps("{\"v\":2}\n{\"pkg\":\"a\",\"label\":\"A\"}\n"),
            Err(AppsError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn missing_version_line_is_a_bad_header() {
        let err = parse_apps("{\"pkg\":\"a\",\"label\":\"A\"}\n").unwrap_err();
        assert!(matches!(err, AppsError::BadHeader(_)), "{err:?}");
        let err = parse_apps("Error: app_process not found\n").unwrap_err();
        assert!(matches!(err, AppsError::BadHeader(_)), "{err:?}");
    }

    #[test]
    fn garbage_line_reports_its_position() {
        let out = "{\"v\":1}\n{\"pkg\":\"a\",\"label\":\"A\"}\n\nnot json\n";
        match parse_apps(out).unwrap_err() {
            AppsError::BadLine { line, .. } => assert_eq!(line, 4),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn app_missing_required_fields_is_a_bad_line() {
        let err = parse_apps("{\"v\":1}\n{\"pkg\":\"a\"}\n").unwrap_err();
        assert!(matches!(err, AppsError::BadLine { line: 2, .. }), "{err:?}");
    }
}

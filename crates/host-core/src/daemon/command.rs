//! The daemon's command line: the roots it may scan, quoting, and the launch
//! command with the token in its environment.

use socketsweep_wire::{token_to_hex, Token, TOKEN_ENV};

use super::{LaunchError, DEFAULT_ROOT, DEVICE_BIN, DEVICE_LOG};

/// Accept `root` only if it is a storage root a phone exposes: `/sdcard`,
/// `/storage/emulated/<user>`, or an SD card at `/storage/XXXX-XXXX`. It goes
/// on the daemon's command line, so anything else is refused rather than
/// interpreted.
pub fn check_root(root: &str) -> Result<(), LaunchError> {
    fn is_user_storage(rest: &str) -> bool {
        !rest.is_empty() && rest.len() <= 9 && rest.bytes().all(|b| b.is_ascii_digit())
    }
    fn is_sd_card(rest: &str) -> bool {
        let b = rest.as_bytes();
        b.len() == 9
            && b[4] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || c.is_ascii_hexdigit())
    }
    let ok = root == DEFAULT_ROOT
        || root
            .strip_prefix("/storage/emulated/")
            .is_some_and(is_user_storage)
        || root.strip_prefix("/storage/").is_some_and(is_sd_card);
    if ok {
        Ok(())
    } else {
        Err(LaunchError::BadRoot(format!(
            "Cannot scan {root:?}. Choose /sdcard, /storage/emulated/<user>, or an SD card at /storage/XXXX-XXXX."
        )))
    }
}

/// Quote `s` as one word for `sh`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The command that starts the daemon in the background. The token travels
/// in the environment, never argv, where other apps could read it.
pub fn launch_command(token: &Token, socket: &str, root: &str) -> String {
    format!(
        "{TOKEN_ENV}={} nohup {DEVICE_BIN} --socket {} --root {} </dev/null >/dev/null 2>{DEVICE_LOG} &",
        token_to_hex(token),
        shell_quote(socket),
        shell_quote(root),
    )
}

/// What [`launch_command`] carries, for fake daemons that play the phone's
/// shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub token_hex: String,
    pub socket: String,
    pub root: String,
}

/// Read back a [`launch_command`]; `None` for any other command.
pub fn parse_launch_command(command: &str) -> Option<LaunchCommand> {
    let rest = command.strip_prefix(TOKEN_ENV)?.strip_prefix('=')?;
    let (token_hex, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix("nohup ")?.strip_prefix(DEVICE_BIN)?;
    let rest = rest.strip_prefix(" --socket ")?;
    let (socket, rest) = take_quoted(rest)?;
    let rest = rest.strip_prefix(" --root ")?;
    let (root, _) = take_quoted(rest)?;
    Some(LaunchCommand {
        token_hex: token_hex.to_string(),
        socket,
        root,
    })
}

/// One word quoted by [`shell_quote`], and what follows it.
pub(super) fn take_quoted(s: &str) -> Option<(String, &str)> {
    let mut rest = s.strip_prefix('\'')?;
    let mut out = String::new();
    loop {
        let end = rest.find('\'')?;
        out.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix(r"\''") {
            Some(after) => {
                out.push('\'');
                rest = after;
            }
            None => return Some((out, rest)),
        }
    }
}

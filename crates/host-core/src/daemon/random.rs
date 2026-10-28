//! Per-session secrets: the socket name, the token and the session id.

use rand::Rng;
use socketsweep_wire::{SessionId, Token, TOKEN_LEN};

pub(super) fn random_socket_name() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    let suffix: String = (0..16)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect();
    format!("socketsweep-{suffix}")
}

/// A fresh token from the operating system's CSPRNG.
pub fn random_token() -> Token {
    let mut token = [0u8; TOKEN_LEN];
    rand::rng().fill(&mut token);
    token
}

pub fn random_session() -> SessionId {
    let mut s = [0u8; 16];
    rand::rng().fill(&mut s);
    s
}

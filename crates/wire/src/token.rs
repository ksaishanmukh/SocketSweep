//! The per-session shared secret.

/// Length in bytes of the per-session token.
pub const TOKEN_LEN: usize = 32;

/// The per-session shared secret presented in [`Hello`](crate::Hello).
pub type Token = [u8; TOKEN_LEN];

/// Environment variable through which the daemon receives its expected token,
/// as exactly `2 * TOKEN_LEN` hex digits (either case). See [`token_to_hex`].
pub const TOKEN_ENV: &str = "SOCKETSWEEP_TOKEN";

/// Encode a token as lowercase hex, the form [`TOKEN_ENV`] carries.
pub fn token_to_hex(token: &Token) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(TOKEN_LEN * 2);
    for b in token {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    s
}

/// Decode a token from exactly `2 * TOKEN_LEN` hex digits. Whitespace is not
/// trimmed: a malformed variable should stop the daemon starting, not be
/// quietly repaired.
pub fn token_from_hex(s: &str) -> Option<Token> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    if bytes.len() != TOKEN_LEN * 2 {
        return None;
    }
    let mut token = [0u8; TOKEN_LEN];
    for (i, out) in token.iter_mut().enumerate() {
        *out = (nibble(bytes[2 * i])? << 4) | nibble(bytes[2 * i + 1])?;
    }
    Some(token)
}

/// Compare two tokens in time that does not depend on where they first differ,
/// so a timing probe cannot learn how much of a guess was right.
pub fn tokens_match(a: &Token, b: &Token) -> bool {
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

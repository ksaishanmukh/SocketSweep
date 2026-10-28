//! Token hex round trips and comparison.

use crate::*;

#[test]
fn tokens_round_trip_through_hex() {
    let token: Token = std::array::from_fn(|i| (i * 37) as u8);
    let hex = token_to_hex(&token);
    assert_eq!(hex.len(), TOKEN_LEN * 2);
    assert_eq!(token_from_hex(&hex), Some(token));
    assert_eq!(token_from_hex(&hex.to_uppercase()), Some(token));
}

#[test]
fn malformed_hex_tokens_are_rejected() {
    let good = "ab".repeat(TOKEN_LEN);
    assert!(token_from_hex(&good).is_some());
    assert_eq!(token_from_hex(""), None);
    assert_eq!(token_from_hex(&good[2..]), None, "too short");
    assert_eq!(token_from_hex(&format!("{good}ab")), None, "too long");
    assert_eq!(token_from_hex(&format!(" {}", &good[1..])), None);
    assert_eq!(token_from_hex(&"zz".repeat(TOKEN_LEN)), None);
}

#[test]
fn token_comparison_is_exact() {
    let a: Token = [7; TOKEN_LEN];
    assert!(tokens_match(&a, &a.clone()));

    let mut last = a;
    last[TOKEN_LEN - 1] ^= 1;
    assert!(!tokens_match(&a, &last));

    let mut first = a;
    first[0] ^= 0x80;
    assert!(!tokens_match(&a, &first));
}

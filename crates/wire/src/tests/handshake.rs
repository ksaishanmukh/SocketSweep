//! The hello and its checks.

use super::support::*;

use crate::*;

const TOKEN: Token = [7; TOKEN_LEN];

#[test]
fn a_good_hello_is_accepted_and_welcomed() {
    let good = Hello::new(TOKEN, [3; 16]);
    assert_eq!(check_hello(&enc(&good), &TOKEN), Some(good.clone()));
    let welcome = good.welcome("b".into());
    assert_eq!(welcome.session, [3; 16]);
    assert_eq!(welcome.daemon_build, "b");
}

#[test]
fn a_wrong_token_magic_or_version_is_refused() {
    let good = Hello::new(TOKEN, [3; 16]);

    let mut wrong_token = good.clone();
    wrong_token.token[TOKEN_LEN - 1] ^= 1;
    assert_eq!(check_hello(&enc(&wrong_token), &TOKEN), None);

    let mut bad_magic = good.clone();
    bad_magic.magic = *b"HTTP";
    assert_eq!(check_hello(&enc(&bad_magic), &TOKEN), None);

    // Any other version, older or newer, is the same bare refusal.
    for version in [PROTOCOL_VERSION - 1, PROTOCOL_VERSION + 1] {
        let mut other = good.clone();
        other.version = version;
        assert_eq!(check_hello(&enc(&other), &TOKEN), None);
    }
}

#[test]
fn garbage_and_trailing_bytes_are_refused() {
    assert_eq!(check_hello(&[], &TOKEN), None);
    assert_eq!(check_hello(b"GET / HTTP/1.1\r\n", &TOKEN), None);

    let mut body = enc(&Hello::new(TOKEN, [3; 16]));
    body.push(0);
    assert_eq!(check_hello(&body, &TOKEN), None);
}

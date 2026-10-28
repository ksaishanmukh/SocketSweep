//! The hello and its checks.

use super::support::*;
use serde::Serialize;

use crate::*;

const TOKEN: Token = [7; TOKEN_LEN];

#[test]
fn a_hello_prefix_decodes_from_any_hello() {
    let body = enc(&Hello::new(TOKEN, [3; 16], Caps::all()));
    let (prefix, rest) = postcard::take_from_bytes::<HelloPrefix>(&body).unwrap();
    assert_eq!(
        prefix,
        HelloPrefix {
            magic: MAGIC,
            version: PROTOCOL_VERSION
        }
    );
    assert!(!rest.is_empty());

    // A future Hello whose fields after the frozen ones are entirely different.
    #[derive(Serialize)]
    struct FutureHello {
        magic: [u8; 4],
        version: u16,
        token: Token,
        something_new: Vec<String>,
    }
    let future = enc(&FutureHello {
        magic: MAGIC,
        version: 900,
        token: TOKEN,
        something_new: vec!["x".into()],
    });
    assert_eq!(
        HelloPrefix::peek(&future),
        Some(HelloPrefix {
            magic: MAGIC,
            version: 900
        })
    );
    assert_eq!(
        check_hello(&future, &TOKEN),
        HelloCheck::WrongVersion { host_version: 900 }
    );
    // The version is only revealed once the token matched.
    assert_eq!(check_hello(&future, &[8; TOKEN_LEN]), HelloCheck::Refused);
}

#[test]
fn hellos_are_checked_token_then_version() {
    let good = Hello::new(TOKEN, [3; 16], Caps::DISK_USAGE);
    assert_eq!(
        check_hello(&enc(&good), &TOKEN),
        HelloCheck::Accepted(good.clone())
    );
    assert_eq!(check_hello(&enc(&good), &TOKEN).rejection(), None);

    let mut wrong_token = good.clone();
    wrong_token.token[TOKEN_LEN - 1] ^= 1;
    let check = check_hello(&enc(&wrong_token), &TOKEN);
    assert_eq!(check, HelloCheck::Refused);
    assert_eq!(check.rejection(), Some(HelloReply::Refused));

    // Wrong token and wrong version: the token is checked first.
    let mut both = wrong_token.clone();
    both.version = 3;
    assert_eq!(check_hello(&enc(&both), &TOKEN), HelloCheck::Refused);

    let mut old = good.clone();
    old.version = 3;
    let check = check_hello(&enc(&old), &TOKEN);
    assert_eq!(check, HelloCheck::WrongVersion { host_version: 3 });
    assert_eq!(
        check.rejection(),
        Some(HelloReply::WrongVersion {
            version: PROTOCOL_VERSION
        })
    );

    let mut bad_magic = good.clone();
    bad_magic.magic = *b"HTTP";
    assert_eq!(check_hello(&enc(&bad_magic), &TOKEN), HelloCheck::Refused);
}

#[test]
fn garbage_and_trailing_bytes_are_refused() {
    assert_eq!(check_hello(&[], &TOKEN), HelloCheck::Refused);
    assert_eq!(
        check_hello(b"GET / HTTP/1.1\r\n", &TOKEN),
        HelloCheck::Refused
    );

    let mut body = enc(&Hello::new(TOKEN, [3; 16], Caps::empty()));
    body.push(0);
    assert_eq!(check_hello(&body, &TOKEN), HelloCheck::Refused);
}

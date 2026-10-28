//! Variant indices and capability bits are pinned.

use serde::Serialize;

use super::support::*;
use crate::*;

/// Every sample encodes as its expected index, and the samples cover every
/// index from 0 up, so none is missing.
fn check_indices<T: Serialize>(samples: &[T], index: impl Fn(&T) -> u8) {
    let mut seen: Vec<u8> = samples
        .iter()
        .map(|s| {
            let bytes = enc(s);
            assert_eq!(bytes[0], index(s), "variant index moved");
            bytes[0]
        })
        .collect();
    seen.sort_unstable();
    seen.dedup();
    let expected: Vec<u8> = (0..seen.len() as u8).collect();
    assert_eq!(seen, expected, "a variant has no sample");
}

#[test]
fn variant_indices_are_stable() {
    check_indices(&client_msgs(), client_index);
    check_indices(&server_msgs(), server_index);
    check_indices(&scan_events(), scan_index);
    check_indices(&hello_replies(), reply_index);

    assert_eq!(enc(&Kind::File), [0]);
    assert_eq!(enc(&Kind::Dir), [1]);
    assert_eq!(enc(&Kind::Symlink), [2]);
    assert_eq!(enc(&Kind::Other), [3]);
    assert_eq!(enc(&FsOp::Open), [0]);
    assert_eq!(enc(&FsOp::ReadDir), [1]);
    assert_eq!(enc(&FsOp::Stat), [2]);
    assert_eq!(enc(&Outcome::Complete), [0]);
    assert_eq!(enc(&Outcome::Cancelled), [1]);
    assert_eq!(
        enc(&Outcome::Aborted {
            code: ErrCode::BadRequest
        }),
        [2, 0]
    );
    let codes = [
        ErrCode::BadRequest,
        ErrCode::DuplicateId,
        ErrCode::Busy,
        ErrCode::Unsupported,
        ErrCode::NotFound,
        ErrCode::PermissionDenied,
        ErrCode::OutsideRoot,
        ErrCode::ProtectedRoot,
        ErrCode::Cancelled,
        ErrCode::Io,
        ErrCode::Internal,
    ];
    for (i, code) in codes.iter().enumerate() {
        assert_eq!(enc(code), [i as u8], "{code:?}");
    }
}

#[test]
fn capability_bits_are_stable() {
    assert_eq!(Caps::DISK_USAGE.bits(), 1);
    assert_eq!(Caps::LISTING_PARTS.bits(), 2);
    assert_eq!(Caps::DEFERRED_STAT.bits(), 4);
    assert_eq!(Caps::COPY_MANIFEST.bits(), 8);
    assert_eq!(Caps::DIR_MTIME.bits(), 16);
    assert_eq!(Caps::RESUME.bits(), 32);
    // Encoded as the bits, a varint, not as flag names.
    assert_eq!(enc(&(Caps::DISK_USAGE | Caps::RESUME)), [33]);
}

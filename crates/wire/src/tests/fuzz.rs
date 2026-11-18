//! Property tests: every decoder that sees untrusted bytes must return an
//! error rather than panic or allocate past [`MAX_FRAME_BYTES`], and valid
//! input must still decode.

use base64::Engine;
use proptest::prelude::*;

use super::support::*;
use crate::apps::parse_apps;
use crate::*;

const TOKEN: Token = [7; TOKEN_LEN];

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 1000,
        ..ProptestConfig::default()
    }
}

/// One way to damage a valid encoding.
#[derive(Debug, Clone)]
enum Mutation {
    Flip(usize, u8),
    Insert(usize, u8),
    Delete(usize),
    Truncate(usize),
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (any::<usize>(), 1..=255u8).prop_map(|(i, x)| Mutation::Flip(i, x)),
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Mutation::Insert(i, b)),
        any::<usize>().prop_map(Mutation::Delete),
        any::<usize>().prop_map(Mutation::Truncate),
    ]
}

fn mutations() -> impl Strategy<Value = Vec<Mutation>> {
    prop::collection::vec(mutation(), 1..4)
}

fn mutate(mut bytes: Vec<u8>, muts: &[Mutation]) -> Vec<u8> {
    for m in muts {
        let n = bytes.len();
        match *m {
            Mutation::Flip(i, x) if n > 0 => bytes[i % n] ^= x,
            Mutation::Insert(i, b) => bytes.insert(i % (n + 1), b),
            Mutation::Delete(i) if n > 0 => {
                bytes.remove(i % n);
            }
            Mutation::Truncate(i) => bytes.truncate(i % (n + 1)),
            _ => {}
        }
    }
    bytes
}

/// Complete frames (length prefix and body) of every message type.
fn valid_frames() -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for msg in client_msgs() {
        frames.push(encode_frame(&ClientFrame { id: 3, msg }).unwrap());
    }
    for msg in server_msgs() {
        frames.push(encode_frame(&ServerFrame { id: 3, msg }).unwrap());
    }
    let scan = ServerFrame {
        id: 1,
        msg: ServerMsg::Scan(scan_events()),
    };
    frames.push(encode_frame(&scan).unwrap());
    frames.push(encode_frame(&Hello::new(TOKEN, [9; 16])).unwrap());
    for reply in hello_replies() {
        frames.push(encode_frame(&reply).unwrap());
    }
    frames
}

fn valid_bodies() -> Vec<Vec<u8>> {
    valid_frames().iter().map(|f| f[4..].to_vec()).collect()
}

/// Run every decoder over `bytes`, as a framed stream and as a bare body.
/// Results are ignored: the property is that none of them panics.
fn decode_everything(bytes: &[u8]) {
    let _ = read_frame::<_, ClientFrame>(&mut &bytes[..]);
    let _ = read_frame::<_, ServerFrame>(&mut &bytes[..]);
    let _ = read_frame::<_, Hello>(&mut &bytes[..]);
    let _ = read_frame::<_, HelloReply>(&mut &bytes[..]);
    let _ = read_frame_body(&mut &bytes[..]);
    let _ = postcard::from_bytes::<ClientMsg>(bytes);
    let _ = postcard::from_bytes::<ServerMsg>(bytes);
    let _ = postcard::from_bytes::<ClientFrame>(bytes);
    let _ = postcard::from_bytes::<ServerFrame>(bytes);
    let _ = postcard::from_bytes::<HelloReply>(bytes);
    let _ = postcard::from_bytes::<Hello>(bytes);
    let _ = check_hello(bytes, &TOKEN);
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn random_bytes_never_panic_any_decoder(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        decode_everything(&bytes);
    }

    #[test]
    fn mutated_valid_frames_decode_or_error_cleanly(
        pick in any::<prop::sample::Index>(),
        muts in prop::collection::vec(mutation(), 1..4),
    ) {
        let frames = valid_frames();
        decode_everything(&mutate(frames[pick.index(frames.len())].clone(), &muts));
    }

    #[test]
    fn mutated_valid_bodies_decode_or_error_cleanly(
        pick in any::<prop::sample::Index>(),
        muts in mutations(),
    ) {
        let bodies = valid_bodies();
        decode_everything(&mutate(bodies[pick.index(bodies.len())].clone(), &muts));
    }

    /// A prefix above the limit is refused from the four bytes alone: the
    /// stream holds no body, so an attempt to allocate or read one would
    /// surface as a different error.
    #[test]
    fn oversized_length_prefix_is_refused_before_reading(
        len in (MAX_FRAME_BYTES + 1)..=u32::MAX,
        tail in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        let mut bytes = len.to_le_bytes().to_vec();
        bytes.extend(tail);
        let err = read_frame_body(&mut &bytes[..]).unwrap_err();
        prop_assert!(matches!(&err, WireError::Malformed(m) if m.contains("exceeds")), "{err:?}");
        let err = read_frame::<_, ServerFrame>(&mut &bytes[..]).unwrap_err();
        prop_assert!(matches!(err, WireError::Malformed(_)));
    }

    /// A prefix within the limit promising more than the stream holds is a
    /// clean `Malformed`.
    #[test]
    fn prefix_longer_than_stream_is_malformed(
        len in 65..=MAX_FRAME_BYTES,
        body in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut bytes = len.to_le_bytes().to_vec();
        bytes.extend(body);
        let err = read_frame_body(&mut &bytes[..]).unwrap_err();
        prop_assert!(matches!(err, WireError::Malformed(_)));
    }

    /// A huge element count with almost no data behind it must be an error,
    /// not an allocation sized by the count.
    #[test]
    fn huge_collection_counts_are_rejected(count in (1u64 << 32)..u64::MAX) {
        let varint = postcard::to_stdvec(&count).unwrap();
        let tag = |msg: &ServerMsg| postcard::to_stdvec(msg).unwrap()[0];
        let mut scan = vec![tag(&ServerMsg::Scan(vec![]))];
        scan.extend(&varint);
        prop_assert!(postcard::from_bytes::<ServerMsg>(&scan).is_err());
        let mut start = vec![postcard::to_stdvec(&client_msgs()[1]).unwrap()[0]];
        start.extend(&varint);
        prop_assert!(postcard::from_bytes::<ClientMsg>(&start).is_err());
    }

    #[test]
    fn valid_frames_round_trip(id in any::<u32>(), nonce in any::<u32>(), items in any::<u64>()) {
        round_trip(&ClientFrame { id, msg: ClientMsg::Ping { nonce } });
        round_trip(&ServerFrame { id, msg: ServerMsg::DeleteProgress { items } });
        round_trip(&ServerFrame { id, msg: ServerMsg::Pong { nonce } });
    }

    #[test]
    fn check_hello_accepts_exactly_the_valid_hello(
        token in any::<Token>(),
        other in any::<Token>(),
        session in any::<SessionId>(),
        junk in prop::collection::vec(any::<u8>(), 0..4),
    ) {
        let hello = Hello::new(token, session);
        let body = postcard::to_stdvec(&hello).unwrap();
        prop_assert_eq!(check_hello(&body, &token), Some(hello));
        prop_assert_eq!(check_hello(&body, &other).is_some(), token == other);
        if !junk.is_empty() {
            let mut longer = body;
            longer.extend(junk);
            prop_assert!(check_hello(&longer, &token).is_none());
        }
    }

    /// A mutated `Hello` is accepted only if magic, token and version all
    /// survived.
    #[test]
    fn check_hello_accepts_a_mutated_hello_only_if_still_valid(
        muts in mutations(),
        session in any::<SessionId>(),
    ) {
        let body = postcard::to_stdvec(&Hello::new(TOKEN, session)).unwrap();
        if let Some(h) = check_hello(&mutate(body, &muts), &TOKEN) {
            prop_assert_eq!(h.magic, MAGIC);
            prop_assert_eq!(h.token, TOKEN);
            prop_assert_eq!(h.version, PROTOCOL_VERSION);
        }
    }
}

fn valid_helper_output() -> &'static str {
    "{\"v\":1}\n\
     {\"pkg\":\"com.whatsapp\",\"label\":\"WhatsApp\",\"system\":false,\"icon\":\"iVBORw0KGg==\"}\n\
     {\"pkg\":\"jp.line\",\"label\":\"ライン \\\"x\\\"\",\"system\":true,\"future\":[1,2]}\n\
     {\"pkg\":\"a.b\",\"label\":\"AB\"}\n"
}

fn app_fields() -> impl Strategy<Value = (String, String, bool, Option<Vec<u8>>)> {
    (
        "[a-z.]{1,20}",
        any::<String>(),
        any::<bool>(),
        prop::option::of(prop::collection::vec(any::<u8>(), 1..32)),
    )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn parse_apps_never_panics_on_arbitrary_strings(s in any::<String>()) {
        let _ = parse_apps(&s);
    }

    #[test]
    fn parse_apps_never_panics_on_lossy_bytes(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
        let _ = parse_apps(&String::from_utf8_lossy(&bytes));
    }

    #[test]
    fn parse_apps_survives_mutated_valid_output(muts in mutations()) {
        let bytes = mutate(valid_helper_output().as_bytes().to_vec(), &muts);
        let _ = parse_apps(&String::from_utf8_lossy(&bytes));
    }

    #[test]
    fn parse_apps_survives_jsonish_garbage(s in "[{}\\[\\]\":,a-z0-9\\\\ \\r\\n]{0,200}") {
        let _ = parse_apps(&s);
        let _ = parse_apps(&format!("{{\"v\":1}}\n{s}"));
    }

    #[test]
    fn parse_apps_round_trips_generated_apps(apps in prop::collection::vec(app_fields(), 0..8)) {
        let mut out = String::from("{\"v\":1}\n");
        for (pkg, label, system, icon) in &apps {
            let icon = icon
                .as_ref()
                .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
            let line = serde_json::json!({"pkg": pkg, "label": label, "system": system, "icon": icon});
            out.push_str(&format!("{line}\n"));
        }
        let parsed = parse_apps(&out).unwrap();
        prop_assert_eq!(parsed.len(), apps.len());
        for (p, (pkg, label, system, icon)) in parsed.iter().zip(&apps) {
            prop_assert_eq!(&p.pkg, pkg);
            prop_assert_eq!(&p.label, label);
            prop_assert_eq!(p.system, *system);
            prop_assert_eq!(&p.icon_png, icon);
        }
    }
}

#[test]
fn every_valid_frame_and_hello_still_decodes() {
    for frame in valid_frames() {
        let body = read_frame_body(&mut &frame[..]).unwrap().unwrap();
        assert_eq!(body.len() + 4, frame.len());
    }
    let hello = Hello::new(TOKEN, [9; 16]);
    let body = postcard::to_stdvec(&hello).unwrap();
    assert_eq!(check_hello(&body, &TOKEN), Some(hello));
}

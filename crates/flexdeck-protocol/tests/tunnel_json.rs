//! Pins the proto3 JSON mapping for the desktop <-> cloud tunnel.
//!
//! The tunnel is a trust boundary, so its shapes are not cosmetic. Three of the
//! assertions below are load-bearing in ways that are easy to miss when reading
//! the wire by eye, and each one has already bitten an API in this repository:
//!
//! 1. `false` and `""` are **absent**, so `ok === false` never matches.
//!    Identical to the `PairResponse` trap in `pairing.proto`.
//! 2. `uint64` ids and `int64` timestamps travel as **strings**, because
//!    JavaScript cannot represent the full range. The request-id case below
//!    uses 2^53 + 1 specifically, a value a JSON number would silently round.
//! 3. A `oneof` branch is selected by its **key**, so an all-default payload
//!    still round-trips as that branch rather than collapsing to `{}`.
//!
//! If any of these break, the portal and the desktop stop agreeing about the
//! same bytes, which is the failure mode this whole contract exists to prevent.

use flexdeck_protocol::flexdeck::v1 as pb;
use serde_json::Value;

#[test]
fn tunnel_fields_use_lower_camel_case() {
    let json = serde_json::to_value(pb::PairOffer {
        code_hash: "abc".to_string(),
        expires_at: 1,
    })
    .expect("serializes");

    assert_eq!(json["codeHash"], "abc");
    assert!(
        json.get("code_hash").is_none(),
        "snake_case names must not appear"
    );
}

#[test]
fn proof_bytes_travel_as_base64() {
    // `bytes` fields use the proto3 JSON mapping: standard base64 in a string.
    // The cloud decodes exactly what the desktop signed, so the encoding is
    // pinned here rather than assumed.
    let json = serde_json::to_value(pb::TunnelHello {
        protocol_version: 2,
        fingerprint: "abc".to_string(),
        name: String::new(),
        rev: 0,
        certificate: vec![0xde, 0xad, 0xbe, 0xef],
        signature: vec![0xff],
    })
    .expect("serializes");

    assert_eq!(json["certificate"], "3q2+7w==");
    assert_eq!(json["signature"], "/w==");
}

#[test]
fn sixty_four_bit_fields_travel_as_strings() {
    // 2^53 + 1: the smallest positive integer a f64 cannot represent. As a JSON
    // number it would arrive as ...992, and the response would never correlate.
    let json = serde_json::to_value(pb::TunnelRequest {
        id: 9_007_199_254_740_993,
        method: "GET".to_string(),
        path: "/api/config".to_string(),
        body: String::new(),
        token: String::new(),
    })
    .expect("serializes");
    assert_eq!(json["id"], Value::String("9007199254740993".to_string()));

    // int64 is a string too, not just uint64.
    let offer = serde_json::to_value(pb::PairOffer {
        code_hash: "abc".to_string(),
        expires_at: 1_700_000_000,
    })
    .expect("serializes");
    assert_eq!(offer["expiresAt"], Value::String("1700000000".to_string()));
}

#[test]
fn a_withdrawn_offer_omits_its_hash_entirely() {
    // The trap this file exists for: an empty string is ABSENT, not `""`. A
    // withdrawn offer is the absence of `codeHash`, and the cloud must test for
    // that rather than for an empty string.
    let json = serde_json::to_value(pb::PairOffer::default()).expect("serializes");
    assert!(json.get("codeHash").is_none(), "{json}");
}

#[test]
fn a_tunnel_frame_uses_the_field_name_as_its_key() {
    let frame = pb::TunnelFrame {
        kind: Some(pb::tunnel_frame::Kind::Hello(pb::TunnelHello {
            protocol_version: 1,
            fingerprint: "abc".to_string(),
            name: String::new(),
            rev: 0,
            certificate: Vec::new(),
            signature: Vec::new(),
        })),
    };

    let json = serde_json::to_value(&frame).expect("serializes");
    assert_eq!(json["hello"]["protocolVersion"], 1);
    assert!(
        json.get("request").is_none(),
        "the unset branch must be absent"
    );
    assert!(
        json.get("push").is_none(),
        "the unset branch must be absent"
    );
}

#[test]
fn every_frame_branch_round_trips() {
    // Every branch is built from DEFAULTS on purpose. An all-default payload
    // serialises to `{}`, and the branch survives only because the oneof key
    // carries the choice — so this is the test that proves an empty
    // `welcome`, `ping` or `pong` is distinguishable from any other empty
    // frame.
    let branches = [
        pb::tunnel_frame::Kind::Hello(pb::TunnelHello::default()),
        pb::tunnel_frame::Kind::Welcome(pb::TunnelWelcome::default()),
        pb::tunnel_frame::Kind::Rejected(pb::TunnelRejected::default()),
        pb::tunnel_frame::Kind::Challenge(pb::TunnelChallenge::default()),
        pb::tunnel_frame::Kind::PairOffer(pb::PairOffer::default()),
        pb::tunnel_frame::Kind::Request(pb::TunnelRequest::default()),
        pb::tunnel_frame::Kind::Response(pb::TunnelResponse::default()),
        pb::tunnel_frame::Kind::Push(pb::TunnelPush::default()),
        pb::tunnel_frame::Kind::Ping(pb::TunnelPing {}),
        pb::tunnel_frame::Kind::Pong(pb::TunnelPong {}),
    ];

    for kind in branches {
        let frame = pb::TunnelFrame { kind: Some(kind) };
        let json = serde_json::to_string(&frame).expect("serializes");
        let back: pb::TunnelFrame = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(frame, back, "a tunnel frame did not survive a round trip");
    }
}

#[test]
fn a_push_carries_the_same_payload_a_lan_socket_sends() {
    // The portal parses one shape for a live patch whether it arrived down a
    // tablet's socket or through the tunnel, so a push must wrap the very same
    // ServerMessage rather than restating it.
    let frame = pb::TunnelFrame {
        kind: Some(pb::tunnel_frame::Kind::Push(pb::TunnelPush {
            message: Some(pb::ServerMessage {
                kind: Some(pb::server_message::Kind::Patch(pb::ConfigPatch {
                    rev: 9,
                    patch: "[{\"op\":\"replace\",\"path\":\"/rev\",\"value\":\"9\"}]".to_string(),
                })),
            }),
        })),
    };

    let json = serde_json::to_value(&frame).expect("serializes");
    assert_eq!(
        json["push"]["message"]["patch"]["rev"],
        Value::String("9".to_string())
    );

    // The patch is DOUBLE-ENCODED, exactly as it is on a LAN socket: a JSON
    // string inside a JSON message. The portal must parse the envelope, then
    // parse the inner string. Neither side may "helpfully" unwrap it.
    let patch = json["push"]["message"]["patch"]["patch"]
        .as_str()
        .expect("the patch travels as a string");
    assert!(
        patch.contains("\"op\""),
        "the inner patch is still JSON text: {patch}"
    );
}

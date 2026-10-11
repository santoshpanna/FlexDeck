//! Converting `TunnelFrame`s to and from the wire.

use flexdeck_protocol::TunnelFrame;

use crate::TunnelError;

/// Serialise a frame for the wire.
pub fn encode(frame: &TunnelFrame) -> Result<String, TunnelError> {
    Ok(serde_json::to_string(frame)?)
}

/// Parse a frame from the wire.
///
/// A frame that will not parse is an **error**, not something to skip.
///
/// That is a deliberate difference from the LAN socket, which ignores anything
/// it does not recognise because the peer there may be an older tablet build and
/// the connection still has value for receiving. Here both ends are our own
/// software, so an unparseable frame means a version mismatch or a bug. Dropping
/// it quietly would turn either one into a request that never completes, which
/// is far harder to diagnose than a refusal that says so.
///
/// A frame that parses but names no branch is refused for the same reason — see
/// [`TunnelError::Empty`].
pub fn decode(text: &str) -> Result<TunnelFrame, TunnelError> {
    let frame: TunnelFrame = serde_json::from_str(text)?;

    if frame.kind.is_none() {
        return Err(TunnelError::Empty);
    }

    Ok(frame)
}

#[cfg(test)]
mod tests {
    use flexdeck_protocol::{tunnel_frame, TunnelFrame, TunnelHello, TunnelPing, TunnelPush};

    use super::*;

    fn frame(kind: tunnel_frame::Kind) -> TunnelFrame {
        TunnelFrame { kind: Some(kind) }
    }

    #[test]
    fn a_frame_survives_the_wire() {
        let original = frame(tunnel_frame::Kind::Hello(TunnelHello {
            protocol_version: crate::PROTOCOL_VERSION,
            fingerprint: "abc123".to_string(),
            name: "Studio PC".to_string(),
            rev: 42,
            certificate: vec![0x30, 0x82, 0x01],
            signature: vec![0x30, 0x45],
        }));

        let text = encode(&original).expect("encodes");
        let restored = decode(&text).expect("decodes");

        assert_eq!(original, restored);
    }

    #[test]
    fn an_empty_frame_still_names_its_branch() {
        // The case that would break if the oneof key were dropped: with an
        // all-default payload the JSON is just `{"ping":{}}`, and "which frame
        // is this" rests entirely on the key.
        let text = encode(&frame(tunnel_frame::Kind::Ping(TunnelPing {}))).expect("encodes");
        assert_eq!(text, r#"{"ping":{}}"#);

        let restored = decode(&text).expect("decodes");
        assert_eq!(restored, frame(tunnel_frame::Kind::Ping(TunnelPing {})));
    }

    #[test]
    fn garbage_is_refused_rather_than_ignored() {
        let error = decode("this is not json").expect_err("must refuse");
        assert!(matches!(error, TunnelError::Frame(_)));
    }

    #[test]
    fn a_frame_with_no_branch_is_refused() {
        // Syntactically valid JSON, but it does not say what it is. Treating
        // this as "nothing to do" would leave a request hanging forever.
        let error = decode("{}").expect_err("must refuse");
        assert!(matches!(error, TunnelError::Empty));
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        // This test was written the other way round first, asserting that an
        // unknown field is tolerated. It failed, and the finding is worth more
        // than the assumption was.
        //
        // The protobuf JSON spec says a reader SHOULD ignore a field it does
        // not know. **pbjson does not.** It generates the serde impls for this
        // contract, and it rejects the entire frame:
        //
        //     unknown field `somethingNew`, there are no fields
        //
        // So the tunnel cannot be extended by adding a field while old and new
        // builds are both in the field: the older end rejects every frame the
        // newer one sends. That is exactly what `PROTOCOL_VERSION` exists for —
        // any change to these messages bumps it, and a mismatch is refused at
        // the handshake with a stated reason instead of appearing later as
        // frames that mysteriously will not parse.
        //
        // The same strictness already bit this repository once, on `WidgetType`
        // unknown *enum* values. It applies to fields too.
        let error = decode(r#"{"pong":{"somethingNew":true}}"#).expect_err("must refuse");
        assert!(matches!(error, TunnelError::Frame(_)));
    }

    #[test]
    fn a_push_survives_as_the_same_payload_a_socket_sends() {
        let original = frame(tunnel_frame::Kind::Push(TunnelPush { message: None }));
        let text = encode(&original).expect("encodes");
        assert_eq!(text, r#"{"push":{}}"#);
        assert_eq!(decode(&text).expect("decodes"), original);
    }
}

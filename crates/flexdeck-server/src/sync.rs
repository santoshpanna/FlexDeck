//! Keeping clients in step with the agent's configuration.
//!
//! # The seam this module exists to close
//!
//! The store can produce an RFC 6902 patch, and the contract can carry one, but
//! those are two separate facts. What a client actually depends on is stronger:
//!
//! > take the document at revision *n*, apply the patch the agent sent, and you
//! > are holding exactly the document at revision *n+1*.
//!
//! Nothing verified that end to end until this module. It is worth being
//! explicit about why that gap is dangerous: a patch that is subtly wrong does
//! not fail loudly. The client renders a configuration that is *almost* right,
//! with a button in the wrong place, and nobody notices until a user does.
//!
//! # The double-encoding footgun
//!
//! A patch travels as a **string inside a JSON message**. So the wire bytes look
//! like this, with the inner JSON escaped:
//!
//! ```text
//! {"patch":{"rev":"2","patch":"[{\"op\":\"replace\",\"path\":\"/rev\",\"value\":2}]"}}
//! ```
//!
//! A client must parse the envelope, then parse the embedded string, then apply
//! it. Forgetting the second parse yields a string, not a document, and is an
//! easy mistake in any language. [`apply_patch`] is the reference implementation
//! of the client side, and the tests walk the whole path — encode, decode,
//! extract, apply — rather than comparing two in-memory values.
//!
//! # Unknown values are rejected, not ignored
//!
//! The generated deserialiser rejects a document that names a value it does not
//! recognise — an unknown `WidgetType`, for example. So an agent built before a
//! new widget type existed cannot read a configuration that uses one; it does
//! not degrade to "ignore that widget", it fails outright.
//!
//! That is a consequence of using generated types rather than hand-written
//! parsing, and it is the right default: silently dropping a widget would leave
//! a user with a button that vanished for no visible reason. But it constrains
//! how the schema may grow. Adding a widget type needs either a tolerant
//! deserialiser or a `schema` version the agent can check *before* parsing, so
//! it can say "this profile needs a newer agent" instead of failing obscurely.

use flexdeck_protocol::{server_message, ConfigPatch, ServerMessage};
use flexdeck_store::ConfigStore;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("configuration store error: {0}")]
    Store(#[from] flexdeck_store::StoreError),

    #[error("the agent has no configuration yet")]
    NoConfiguration,

    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("could not apply the patch: {0}")]
    Patch(String),
}

/// What a client needs in order to become current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The client is already at the current revision.
    UpToDate,

    /// Apply this patch and the client is current. The cheap case.
    Patch { rev: u64, patch: String },

    /// The client cannot be patched, so it must take the whole document.
    Snapshot { rev: u64, config: String },
}

/// Decide how to bring a client that last saw `from_rev` up to date.
///
/// Passing `None` means "I have never synced", which is the same situation as a
/// revision we cannot patch from.
pub fn catch_up(store: &ConfigStore, from_rev: Option<u64>) -> Result<SyncOutcome, SyncError> {
    let Some(current) = store.rev()? else {
        return Err(SyncError::NoConfiguration);
    };

    let Some(from) = from_rev else {
        return snapshot(store, current);
    };

    if from == current {
        return Ok(SyncOutcome::UpToDate);
    }

    // Patch only when the client is genuinely behind *and* its revision still
    // exists. A client that is ahead (the agent undid something) or that names a
    // revision which has been discarded cannot be patched from. Sending it a
    // patch anyway would corrupt it silently, so it gets the whole document.
    if from < current && store.snapshot_at(from)?.is_some() {
        return Ok(SyncOutcome::Patch {
            rev: current,
            patch: store.diff(from, current)?,
        });
    }

    snapshot(store, current)
}

fn snapshot(store: &ConfigStore, rev: u64) -> Result<SyncOutcome, SyncError> {
    let (_, config) = store.snapshot()?.ok_or(SyncError::NoConfiguration)?;
    Ok(SyncOutcome::Snapshot { rev, config })
}

/// Wrap a patch in the contract's wire envelope.
pub fn patch_message(rev: u64, patch: &str) -> ServerMessage {
    ServerMessage {
        kind: Some(server_message::Kind::Patch(ConfigPatch {
            rev,
            patch: patch.to_string(),
        })),
    }
}

/// The complete configuration for a client that has never synced.
pub fn welcome_message(
    rev: u64,
    profile_id: &str,
    config_json: &str,
) -> Result<ServerMessage, SyncError> {
    let config = serde_json::from_str(config_json)?;
    Ok(ServerMessage {
        kind: Some(server_message::Kind::Welcome(
            flexdeck_protocol::ServerWelcome {
                rev,
                profile_id: profile_id.to_string(),
                config: Some(config),
            },
        )),
    })
}

/// Encode a message exactly as it goes over the socket.
pub fn encode(message: &ServerMessage) -> Result<String, SyncError> {
    Ok(serde_json::to_string(message)?)
}

/// Decode a message as a client would.
pub fn decode(json: &str) -> Result<ServerMessage, SyncError> {
    Ok(serde_json::from_str(json)?)
}

/// Apply an RFC 6902 patch to a configuration document.
///
/// This is the **client side** of the protocol, written out in Rust so the round
/// trip can be tested rather than asserted. The Android and portal clients do
/// the same three steps: parse the document, parse the patch, apply.
pub fn apply_patch(document: &str, patch: &str) -> Result<String, SyncError> {
    let mut value: Value = serde_json::from_str(document)?;
    let operations: json_patch::Patch = serde_json::from_str(patch)?;
    json_patch::patch(&mut value, &operations)
        .map_err(|error| SyncError::Patch(error.to_string()))?;
    Ok(serde_json::to_string(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_store::ConfigStore;

    fn store_with(revisions: &[&str]) -> ConfigStore {
        let store = ConfigStore::open_in_memory().expect("in-memory store opens");
        for json in revisions {
            store.commit(json).expect("commits");
        }
        store
    }

    fn seeded() -> ConfigStore {
        store_with(&[
            r#"{"schema":2,"rev":1,"profiles":[]}"#,
            r#"{"schema":2,"rev":2,"profiles":[{"id":"tab1","name":"Kitchen"}]}"#,
            r#"{"schema":2,"rev":3,"profiles":[{"id":"tab1","name":"Kitchen"},{"id":"tab2","name":"Studio"}]}"#,
        ])
    }

    #[test]
    fn a_client_with_no_revision_gets_the_whole_document() {
        let store = seeded();
        let outcome = catch_up(&store, None).expect("decides");

        match outcome {
            SyncOutcome::Snapshot { rev, config } => {
                assert_eq!(rev, 3);
                assert_eq!(config, store.snapshot().unwrap().unwrap().1);
            }
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[test]
    fn a_current_client_is_told_it_is_current() {
        let store = seeded();
        assert_eq!(
            catch_up(&store, Some(3)).expect("decides"),
            SyncOutcome::UpToDate
        );
    }

    #[test]
    fn a_client_behind_by_one_gets_a_patch() {
        let store = seeded();
        match catch_up(&store, Some(2)).expect("decides") {
            SyncOutcome::Patch { rev, patch } => {
                assert_eq!(rev, 3, "the patch lands the client at the current revision");
                assert!(patch.starts_with('['), "a JSON Patch is an array: {patch}");
            }
            other => panic!("expected a patch, got {other:?}"),
        }
    }

    #[test]
    fn a_client_ahead_of_the_agent_gets_a_snapshot_not_a_patch() {
        // This happens for real: the agent undoes a change while a tablet is
        // still holding the newer revision. Patching backwards is not a thing,
        // so the client must be reset rather than left inconsistent.
        let store = seeded();
        match catch_up(&store, Some(99)).expect("decides") {
            SyncOutcome::Snapshot { rev, .. } => assert_eq!(rev, 3),
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[test]
    fn a_client_naming_a_discarded_revision_gets_a_snapshot() {
        let store = seeded();
        // Rewind to 2 and commit, which discards revision 3.
        store.undo().expect("undo");
        store
            .commit(r#"{"schema":2,"rev":2,"profiles":[]}"#)
            .expect("commit");

        match catch_up(&store, Some(3)).expect("decides") {
            SyncOutcome::Snapshot { rev, .. } => assert_eq!(rev, 4, "rev 4 is the new head"),
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[test]
    fn a_client_holding_a_discarded_revision_is_not_told_it_is_current() {
        // The bug this guards: if a commit after an undo reused the discarded
        // revision number, a client still holding that number would be told
        // "up to date" while holding entirely different content. Nothing
        // downstream could detect that, so it would present a stale layout
        // forever.
        let store = seeded(); // head 3
        store.undo().expect("undo"); // head 2
        store
            .commit(r#"{"schema":2,"rev":3,"profiles":[]}"#)
            .expect("commit");

        assert_ne!(
            catch_up(&store, Some(3)).expect("decides"),
            SyncOutcome::UpToDate
        );
    }

    #[test]
    fn an_empty_store_reports_no_configuration() {
        let store = ConfigStore::open_in_memory().expect("store");
        assert!(matches!(
            catch_up(&store, None),
            Err(SyncError::NoConfiguration)
        ));
    }

    /// The seam this module exists for.
    ///
    /// Note that this walks the *wire*: it encodes the message to JSON, decodes
    /// it back, pulls the patch out of the envelope, and only then applies it.
    /// Comparing `store.diff()` to an in-memory value would not have caught a
    /// client-visible encoding mistake, which is the whole point.
    #[test]
    fn a_patch_survives_the_wire_and_rebuilds_the_next_revision() {
        let store = seeded();

        for from in 1..3 {
            let to = from + 1;

            let patch = store.diff(from, to).expect("diffs");
            let message = patch_message(to, &patch);
            let wire = encode(&message).expect("encodes");
            let decoded = decode(&wire).expect("decodes");

            let server_message::Kind::Patch(carried) =
                decoded.kind.expect("a patch message has a body")
            else {
                panic!("expected a patch variant");
            };

            assert_eq!(
                carried.rev, to,
                "the envelope carries the destination revision"
            );

            let before = store.snapshot_at(from).expect("reads").expect("exists");
            let expected = store.snapshot_at(to).expect("reads").expect("exists");
            let rebuilt = apply_patch(&before, &carried.patch).expect("applies");

            // Compare as JSON, never as bytes. A JSON object is unordered and a
            // round trip through any parser may reorder keys, so byte equality
            // would fail on two documents that mean exactly the same thing.
            // Clients must not depend on key order either.
            let rebuilt: Value = serde_json::from_str(&rebuilt).expect("rebuilt parses");
            let expected: Value = serde_json::from_str(&expected).expect("expected parses");
            assert_eq!(
                rebuilt, expected,
                "applying the wire patch to rev {from} must produce rev {to}"
            );
        }
    }

    #[test]
    fn the_embedded_patch_string_is_itself_valid_json_after_decoding() {
        // The patch travels as a string inside JSON, so it gets escaped on the
        // wire. This asserts the inner document survives that, which is the step
        // a client author is most likely to get wrong.
        let store = seeded();
        let patch = store.diff(2, 3).expect("diffs");
        let wire = encode(&patch_message(3, &patch)).expect("encodes");

        // The raw wire bytes really do contain escaped inner JSON.
        assert!(
            wire.contains("\\\"op\\\""),
            "the patch should be escaped inside the envelope: {wire}"
        );

        let decoded = decode(&wire).expect("decodes");
        let server_message::Kind::Patch(carried) = decoded.kind.expect("has a body") else {
            panic!("expected a patch variant");
        };

        // After decoding, the inner string parses as a JSON Patch.
        let operations: Value = serde_json::from_str(&carried.patch).expect("inner JSON parses");
        assert!(
            operations.is_array(),
            "a JSON Patch is an array of operations"
        );
    }

    #[test]
    fn a_welcome_carries_a_real_config_document() {
        let store = seeded();
        let (rev, config) = store.snapshot().expect("reads").expect("exists");

        let message = welcome_message(rev, "tab1", &config).expect("builds");
        let decoded = decode(&encode(&message).expect("encodes")).expect("decodes");

        let server_message::Kind::Welcome(welcome) = decoded.kind.expect("has a body") else {
            panic!("expected a welcome variant");
        };

        assert_eq!(welcome.rev, 3);
        assert_eq!(welcome.profile_id, "tab1");
        let carried = welcome.config.expect("a welcome carries a config");
        assert_eq!(carried.rev, 3, "the nested config is a real document");
        assert_eq!(carried.profiles.len(), 2);
    }

    #[test]
    fn applying_a_patch_to_the_wrong_document_fails_loudly() {
        // Guards the failure mode that matters: a mismatched patch must error,
        // not quietly produce a plausible-looking document.
        let store = seeded();
        let patch = store.diff(2, 3).expect("diffs");
        let wrong_document = r#"{"totally":"different"}"#;

        assert!(apply_patch(wrong_document, &patch).is_err());
    }
}

//! Pairing: prove to a tablet that it is talking to *this* agent, then give it a
//! long-lived token.
//!
//! # The shape of the problem
//!
//! The agent listens on a LAN, so any device on that network can reach it.
//! "Reachable" must not mean "trusted". Pairing is the single moment where a
//! human authorises a device, and it has to work without a certificate
//! authority:
//!
//! 1. The agent holds a **self-signed** certificate and shows its
//!    **fingerprint** next to a PIN in its window, and in a QR code.
//! 2. The tablet connects and **pins that fingerprint**. If it does not match,
//!    the connection is refused. Pinning is what makes a self-signed
//!    certificate safe here — a man in the middle on the LAN has a different
//!    certificate, so it has a different fingerprint, so the tablet hangs up.
//! 3. The tablet presents either the PIN (typed by the human) or the QR's
//!    one-time secret. The agent replies with a long-lived device token.
//! 4. Only the **hash** of that token is stored, so a stolen database file does
//!    not hand over working credentials.
//!
//! # Time is a parameter, never a global
//!
//! Expiry is security-relevant, so `now` is passed into these APIs rather than
//! read from the system clock inside them. That is not fussiness: it is the
//! difference between "expiry is tested at the boundary and just past it" and
//! "expiry is never really tested at all", because the alternative is sleeping
//! in a test.

use std::fmt::Write as _;

use flexdeck_protocol::PairingInfo;
use sha2::{Digest, Sha256};

pub mod identity;
pub mod session;

pub use identity::{
    fingerprint_of_der, tunnel_proof_message, verify_proof, Identity, IdentityError, ProofError,
};
pub use session::{display_code, normalise_code, PairingDesk, PairingOutcome, SessionOffer};

/// Bytes of entropy in a device token.
const TOKEN_BYTES: usize = 32;

/// The wire version of the pairing payload. Bump only for a breaking change.
pub const QR_VERSION: &str = "1";

/// Mint a new device token.
///
/// The device is shown this value **once**. Persist [`hash_token`] of it, never
/// the token itself.
pub fn generate_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    rand::fill(&mut bytes);
    to_hex(&bytes)
}

/// Hash a device token for storage.
///
/// This is what goes in the database. Authenticating a client means hashing the
/// token it presents and looking that up.
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    to_hex(&hasher.finalize())
}

/// Bytes of entropy in a generated device id.
const DEVICE_ID_BYTES: usize = 8;

/// Mint an identifier for a device that has just paired.
///
/// The **agent** assigns this, not the client. A client-chosen id could collide
/// with an existing device or impersonate one, and the pairing reply is the
/// natural place to hand the authoritative value back.
pub fn new_device_id() -> String {
    let mut bytes = [0_u8; DEVICE_ID_BYTES];
    rand::fill(&mut bytes);
    to_hex(&bytes)
}

/// Build the JSON that goes into the pairing QR code.
///
/// This is the proto3 JSON mapping of [`PairingInfo`], so the Android client
/// decodes it with its generated types instead of hand-rolled parsing. That
/// matters: the QR is the one payload a client must read *before* it trusts the
/// agent, so it should be the least surprising format available.
pub fn qr_payload(
    identity: &Identity,
    host: &str,
    port: u16,
    offer: &SessionOffer,
) -> Result<String, serde_json::Error> {
    let info = PairingInfo {
        version: QR_VERSION.to_string(),
        host: host.to_string(),
        port: u32::from(port),
        fingerprint: identity.fingerprint(),
        session_id: offer.id.clone(),
        one_time_secret: offer.one_time_secret.clone(),
        expires_at: offer.expires_at,
    };
    serde_json::to_string(&info)
}

/// Decode a QR payload.
///
/// Exposed so tests and the portal's preview can verify the round trip without
/// duplicating the format.
pub fn decode_qr_payload(json: &str) -> Result<PairingInfo, serde_json::Error> {
    serde_json::from_str(json)
}

/// Compare two byte strings without leaking their contents through timing.
///
/// A plain `==` on slices stops at the first differing byte, so an attacker can
/// recover a PIN one digit at a time by measuring how long the comparison took.
/// This always inspects every byte.
///
/// Length differences are still observable, which is fine here: the codes being
/// compared are fixed-length, so a length mismatch is not a guess an attacker
/// can make progressively useful.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut difference = 0_u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Lowercase hex. Hand-rolled to avoid a dependency for six lines.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("writing into a String cannot fail");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::PairingDesk;

    #[test]
    fn hex_encoding_is_lowercase_and_zero_padded() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
        assert_eq!(to_hex(&[]), "");
    }

    #[test]
    fn tokens_are_hex_and_the_right_size() {
        let token = generate_token();
        assert_eq!(token.len(), TOKEN_BYTES * 2);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn two_tokens_differ() {
        // Not a proof of randomness, but it would catch a generator accidentally
        // seeded to a constant.
        assert_ne!(generate_token(), generate_token());
    }

    #[test]
    fn token_hashing_matches_a_known_sha256_vector() {
        // The published SHA-256 of "abc". If this fails, we are not computing
        // SHA-256 — which would silently weaken every stored credential.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn token_hashing_is_deterministic() {
        let token = "a-token";
        assert_eq!(hash_token(token), hash_token(token));
        assert_ne!(hash_token("a-token"), hash_token("b-token"));
    }

    #[test]
    fn constant_time_comparison_agrees_with_equality() {
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        // Differing in the first byte must be just as false as differing in the
        // last — no early exit that a timer could observe.
        assert!(!constant_time_eq(b"023456", b"123456"));
        assert!(!constant_time_eq(b"12345", b"123456"));
    }

    #[test]
    fn qr_payload_round_trips_through_the_contract_type() {
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        let mut desk = PairingDesk::default();
        let offer = desk.begin(1_700_000_000);

        let json = qr_payload(&identity, "192.168.1.10", 7878, &offer).expect("encodes");
        let decoded = decode_qr_payload(&json).expect("decodes");

        assert_eq!(decoded.version, QR_VERSION);
        assert_eq!(decoded.host, "192.168.1.10");
        assert_eq!(decoded.port, 7878);
        assert_eq!(decoded.fingerprint, identity.fingerprint());
        assert_eq!(decoded.session_id, offer.id);
        assert_eq!(decoded.one_time_secret, offer.one_time_secret);
        assert_eq!(decoded.expires_at, offer.expires_at);
    }

    #[test]
    fn the_qr_carries_the_fingerprint_a_client_pins() {
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        let mut desk = PairingDesk::default();
        let offer = desk.begin(0);

        let json = qr_payload(&identity, "10.0.0.5", 7878, &offer).expect("encodes");
        let decoded = decode_qr_payload(&json).expect("decodes");

        // 64 lowercase hex characters: a full SHA-256 digest.
        assert_eq!(decoded.fingerprint.len(), 64);
        assert!(decoded.fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(decoded.fingerprint, fingerprint_of_der(identity.cert_der()));
    }
}

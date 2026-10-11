//! The agent's TLS identity: a self-signed certificate plus its fingerprint.
//!
//! There is no certificate authority on a home LAN, so the agent signs its own
//! certificate. That is only safe because clients **pin the fingerprint** rather
//! than trusting a chain: a self-signed certificate proves nothing by itself,
//! but a *specific* self-signed certificate seen once, at pairing time, over a
//! QR code the user physically scanned, proves quite a lot.

use std::path::Path;

use ring::signature::{self, EcdsaKeyPair};
use sha2::{Digest, Sha256};

use crate::to_hex;

/// File names an identity is stored under, within its directory.
const CERT_DER_FILE: &str = "cert.der";
const CERT_PEM_FILE: &str = "cert.pem";
const KEY_PEM_FILE: &str = "key.pem";

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("could not generate a self-signed certificate: {0}")]
    Certificate(String),

    #[error("the identity could not be read or written: {0}")]
    Io(#[from] std::io::Error),

    #[error("the identity's private key could not be used: {0}")]
    Key(String),

    #[error(
        "the identity in {0} is incomplete; refusing to replace it, because every paired device \
         would have to pair again"
    )]
    Incomplete(String),
}

/// A generated TLS identity.
///
/// Held in memory once generated; loading and saving is the caller's job, so
/// this type carries no filesystem assumptions.
#[derive(Debug, Clone)]
pub struct Identity {
    cert_der: Vec<u8>,
    cert_pem: String,
    key_pem: String,
}

impl Identity {
    /// Generate a fresh self-signed certificate valid for the given names.
    pub fn generate(subject_alt_names: Vec<String>) -> Result<Self, IdentityError> {
        let certified = rcgen::generate_simple_self_signed(subject_alt_names)
            .map_err(|error| IdentityError::Certificate(error.to_string()))?;

        Ok(Self {
            cert_der: certified.cert.der().to_vec(),
            cert_pem: certified.cert.pem(),
            key_pem: certified.signing_key.serialize_pem(),
        })
    }

    /// Rebuild an identity from material loaded off disk.
    ///
    /// The DER form is required alongside the PEM because the fingerprint is
    /// taken over the DER encoding, and re-encoding the PEM to get it back would
    /// risk a mismatch with the certificate a client actually received.
    pub fn from_parts(cert_der: Vec<u8>, cert_pem: String, key_pem: String) -> Self {
        Self {
            cert_der,
            cert_pem,
            key_pem,
        }
    }

    /// Load the identity from `directory`, generating one if none is there yet.
    ///
    /// A *partially* present identity is an error rather than a reason to start
    /// over. Regenerating would change the fingerprint, and every device that
    /// had pinned the old one would begin rejecting the agent — with no visible
    /// cause, since the agent itself would look perfectly healthy.
    pub fn load_or_generate(
        directory: impl AsRef<Path>,
        subject_alt_names: Vec<String>,
    ) -> Result<Self, IdentityError> {
        let directory = directory.as_ref();
        let der_path = directory.join(CERT_DER_FILE);
        let cert_path = directory.join(CERT_PEM_FILE);
        let key_path = directory.join(KEY_PEM_FILE);

        let present = [&der_path, &cert_path, &key_path]
            .iter()
            .filter(|path| path.exists())
            .count();

        if present == 0 {
            let identity = Self::generate(subject_alt_names)?;
            identity.save(directory)?;
            return Ok(identity);
        }

        if present < 3 {
            return Err(IdentityError::Incomplete(directory.display().to_string()));
        }

        Ok(Self::from_parts(
            std::fs::read(&der_path)?,
            std::fs::read_to_string(&cert_path)?,
            std::fs::read_to_string(&key_path)?,
        ))
    }

    /// Write this identity into `directory`, creating it if needed.
    ///
    /// The DER encoding is written alongside the PEM rather than re-derived on
    /// load. The fingerprint is taken over the DER and clients pin it, so
    /// storing the exact bytes removes any chance of the two disagreeing.
    ///
    /// Note that the private key is written with default permissions. Tightening
    /// that is platform-specific and deliberately left to the shell.
    pub fn save(&self, directory: impl AsRef<Path>) -> Result<(), IdentityError> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory)?;

        std::fs::write(directory.join(CERT_DER_FILE), &self.cert_der)?;
        std::fs::write(directory.join(CERT_PEM_FILE), &self.cert_pem)?;
        std::fs::write(directory.join(KEY_PEM_FILE), &self.key_pem)?;
        Ok(())
    }

    /// SHA-256 of the DER certificate, as lowercase hex.
    ///
    /// This is the value a client pins, and the value carried in the pairing QR.
    /// Two identities with the same fingerprint have the same certificate.
    pub fn fingerprint(&self) -> String {
        fingerprint_of_der(&self.cert_der)
    }

    pub fn cert_der(&self) -> &[u8] {
        &self.cert_der
    }

    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    pub fn key_pem(&self) -> &str {
        &self.key_pem
    }

    /// Sign `message` with this identity's private key (ECDSA P-256, SHA-256,
    /// ASN.1 DER signature).
    ///
    /// This is how the agent proves to the cloud that it *holds* the key behind
    /// the fingerprint it claims, rather than merely knowing the fingerprint —
    /// which is not a secret: every paired tablet has it.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, IdentityError> {
        let key = rcgen::KeyPair::from_pem(&self.key_pem)
            .map_err(|error| IdentityError::Key(error.to_string()))?;
        let rng = ring::rand::SystemRandom::new();
        let pair = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &key.serialize_der(),
            &rng,
        )
        .map_err(|error| IdentityError::Key(error.to_string()))?;

        pair.sign(&rng, message)
            .map(|signed| signed.as_ref().to_vec())
            .map_err(|_| IdentityError::Key("signing failed".to_string()))
    }
}

/// Why a proof of identity was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProofError {
    #[error("the certificate does not have the fingerprint it was presented under")]
    FingerprintMismatch,

    #[error("the certificate could not be read")]
    Certificate,

    #[error("the signature does not verify against the certificate's key")]
    Signature,
}

/// Check that whoever produced `signature` over `message` holds the private key
/// of `cert_der`, and that `cert_der` is the certificate `fingerprint` names.
///
/// Both halves matter. The signature alone proves possession of *a* key; the
/// fingerprint check binds that key to the identity tablets have pinned and the
/// cloud routes by.
pub fn verify_proof(
    cert_der: &[u8],
    fingerprint: &str,
    message: &[u8],
    signature: &[u8],
) -> Result<(), ProofError> {
    if !crate::constant_time_eq(
        fingerprint_of_der(cert_der).as_bytes(),
        fingerprint.as_bytes(),
    ) {
        return Err(ProofError::FingerprintMismatch);
    }

    let (_, certificate) =
        x509_parser::parse_x509_certificate(cert_der).map_err(|_| ProofError::Certificate)?;
    let point = &certificate.public_key().subject_public_key.data;

    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, point.as_ref())
        .verify(message, signature)
        .map_err(|_| ProofError::Signature)
}

/// The exact bytes a tunnel signs to prove its identity to the cloud.
///
/// A fixed context string keeps this signature from being replayable anywhere
/// else the key is used, and the fingerprint is included so a proof for one
/// identity can never be presented under another.
pub fn tunnel_proof_message(nonce: &str, fingerprint: &str) -> Vec<u8> {
    format!("flexdeck-tunnel-v2\n{nonce}\n{fingerprint}").into_bytes()
}

/// SHA-256 of a DER-encoded certificate, as lowercase hex.
///
/// Deliberately free-standing and total: it takes bytes and returns a string, so
/// it can be checked against a published test vector without generating a
/// certificate first.
pub fn fingerprint_of_der(der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(der);
    to_hex(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprinting_matches_a_known_sha256_vector() {
        // The published SHA-256 of the empty input. If this fails we are not
        // computing SHA-256, and every pinned fingerprint would be wrong in the
        // same way — so it is worth pinning the primitive itself.
        assert_eq!(
            fingerprint_of_der(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    fn identity() -> Identity {
        Identity::generate(vec!["localhost".to_string()]).expect("generates")
    }

    #[test]
    fn a_signature_proves_possession_of_the_fingerprinted_key() {
        let identity = identity();
        let message = tunnel_proof_message("nonce-1", &identity.fingerprint());
        let signature = identity.sign(&message).expect("signs");

        verify_proof(
            identity.cert_der(),
            &identity.fingerprint(),
            &message,
            &signature,
        )
        .expect("a genuine proof verifies");
    }

    #[test]
    fn knowing_a_fingerprint_is_not_enough_to_prove_it() {
        // The attack this exists for: present someone else's certificate and
        // fingerprint — both public — and sign with your own key.
        let victim = identity();
        let attacker = identity();
        let message = tunnel_proof_message("nonce-1", &victim.fingerprint());
        let signature = attacker.sign(&message).expect("signs");

        assert_eq!(
            verify_proof(
                victim.cert_der(),
                &victim.fingerprint(),
                &message,
                &signature
            ),
            Err(ProofError::Signature)
        );
    }

    #[test]
    fn a_certificate_presented_under_another_fingerprint_is_refused() {
        let real = identity();
        let other = identity();
        let message = tunnel_proof_message("nonce-1", &other.fingerprint());
        let signature = real.sign(&message).expect("signs");

        assert_eq!(
            verify_proof(real.cert_der(), &other.fingerprint(), &message, &signature),
            Err(ProofError::FingerprintMismatch)
        );
    }

    #[test]
    fn a_proof_for_one_nonce_does_not_answer_another() {
        let identity = identity();
        let fingerprint = identity.fingerprint();
        let signature = identity
            .sign(&tunnel_proof_message("nonce-1", &fingerprint))
            .expect("signs");

        assert_eq!(
            verify_proof(
                identity.cert_der(),
                &fingerprint,
                &tunnel_proof_message("nonce-2", &fingerprint),
                &signature
            ),
            Err(ProofError::Signature)
        );
    }

    #[test]
    fn an_identity_loaded_from_disk_still_signs() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let written = Identity::load_or_generate(directory.path(), vec!["localhost".into()])
            .expect("generates");
        let read =
            Identity::load_or_generate(directory.path(), vec!["localhost".into()]).expect("loads");
        let message = b"hello";

        verify_proof(
            written.cert_der(),
            &written.fingerprint(),
            message,
            &read.sign(message).expect("signs"),
        )
        .expect("the stored key is the certificate's key");
    }

    #[test]
    fn fingerprinting_is_deterministic_and_sensitive_to_input() {
        assert_eq!(fingerprint_of_der(b"abc"), fingerprint_of_der(b"abc"));
        assert_ne!(fingerprint_of_der(b"abc"), fingerprint_of_der(b"abd"));
    }

    #[test]
    fn a_fingerprint_is_a_full_lowercase_digest() {
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("generates");
        let fingerprint = identity.fingerprint();

        assert_eq!(fingerprint.len(), 64, "a SHA-256 digest is 32 bytes");
        assert!(
            fingerprint
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "fingerprints are lowercase hex: {fingerprint}"
        );
    }

    #[test]
    fn a_generated_identity_has_usable_pem_material() {
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("generates");

        assert!(identity
            .cert_pem()
            .starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(identity
            .cert_pem()
            .trim_end()
            .ends_with("-----END CERTIFICATE-----"));
        assert!(identity.key_pem().contains("PRIVATE KEY"));
        assert!(!identity.cert_der().is_empty());
    }

    #[test]
    fn the_fingerprint_follows_the_der_not_the_pem() {
        let generated = Identity::generate(vec!["localhost".to_string()]).expect("generates");

        // Rebuilding from the same DER must reproduce the fingerprint; that is
        // what lets an identity be persisted and reloaded without invalidating
        // clients that already pinned it.
        let reloaded = Identity::from_parts(
            generated.cert_der().to_vec(),
            generated.cert_pem().to_string(),
            generated.key_pem().to_string(),
        );

        assert_eq!(reloaded.fingerprint(), generated.fingerprint());
    }

    #[test]
    fn two_generated_identities_are_different() {
        let first = Identity::generate(vec!["localhost".to_string()]).expect("generates");
        let second = Identity::generate(vec!["localhost".to_string()]).expect("generates");

        // Each pairing session should present a fresh certificate; sharing one
        // across installs would make the fingerprint meaningless as a pin.
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn an_identity_is_generated_once_and_loaded_afterwards() {
        let directory = tempfile::tempdir().expect("temp dir");
        let names = vec!["localhost".to_string()];

        let first = Identity::load_or_generate(directory.path(), names.clone()).expect("generates");
        let second = Identity::load_or_generate(directory.path(), names).expect("loads");

        // The fingerprint a client pinned has to survive a restart, otherwise
        // every paired device would start rejecting the agent.
        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn generating_creates_the_directory_and_its_files() {
        let root = tempfile::tempdir().expect("temp dir");
        let directory = root.path().join("nested").join("identity");

        Identity::load_or_generate(&directory, vec!["localhost".to_string()]).expect("generates");

        for name in [CERT_DER_FILE, CERT_PEM_FILE, KEY_PEM_FILE] {
            assert!(directory.join(name).exists(), "{name} was not written");
        }
    }

    #[test]
    fn a_partial_identity_is_refused_rather_than_replaced() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            directory.path().join(CERT_PEM_FILE),
            "-----BEGIN CERTIFICATE-----",
        )
        .expect("writes");

        let error = Identity::load_or_generate(directory.path(), vec!["localhost".to_string()])
            .expect_err("should refuse");

        assert!(matches!(error, IdentityError::Incomplete(_)), "{error}");
        // And it left the partial file exactly where it was, for the user to fix.
        assert!(directory.path().join(CERT_PEM_FILE).exists());
    }

    #[test]
    fn the_stored_fingerprint_is_taken_over_the_stored_der() {
        let directory = tempfile::tempdir().expect("temp dir");
        let identity = Identity::load_or_generate(directory.path(), vec!["localhost".to_string()])
            .expect("generates");

        let stored = std::fs::read(directory.path().join(CERT_DER_FILE)).expect("reads");

        assert_eq!(fingerprint_of_der(&stored), identity.fingerprint());
    }
}

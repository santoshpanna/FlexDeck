//! Serving the agent over TLS.
//!
//! The agent already has a self-signed identity — it exists so a client can pin
//! the fingerprint at pairing. Using it for *this* matters more than it might
//! look: every API call carries `Authorization: Bearer <token>` in a header, and
//! over plain HTTP that token is readable by anything on the network. The whole
//! point of the authentication added earlier would be undone by putting it on an
//! unencrypted wire.
//!
//! There is no certificate chain to validate, and that is deliberate: the
//! certificate is self-signed, and the fingerprint — shown by the agent and
//! pinned by the client — is what actually establishes identity.

use axum_server::tls_rustls::RustlsConfig;
use flexdeck_pairing::Identity;

/// Build a rustls configuration from the agent's own identity.
///
/// Fails when the certificate and key do not match, which is worth catching at
/// start-up rather than when the first client connects: an agent that cannot
/// serve TLS should say so immediately, not appear to be running.
pub async fn rustls_config(identity: &Identity) -> std::io::Result<RustlsConfig> {
    RustlsConfig::from_pem(
        identity.cert_pem().as_bytes().to_vec(),
        identity.key_pem().as_bytes().to_vec(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_agents_own_identity_makes_a_usable_tls_configuration() {
        // Proves the PEM the agent writes is something rustls will actually
        // accept — in particular that the certificate and key belong together.
        // A mismatch would otherwise surface as every client failing to connect.
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("an identity");

        assert!(rustls_config(&identity).await.is_ok());
    }

    #[tokio::test]
    async fn an_identity_loaded_from_disk_also_works() {
        // The one that runs for real on every restart after the first.
        let directory = tempfile::tempdir().expect("a temp dir");
        let generated = Identity::load_or_generate(directory.path(), vec!["localhost".to_string()])
            .expect("generates");

        let reloaded = Identity::load_or_generate(directory.path(), vec!["localhost".to_string()])
            .expect("loads");

        assert!(rustls_config(&reloaded).await.is_ok());
        assert_eq!(generated.fingerprint(), reloaded.fingerprint());
    }

    #[tokio::test]
    async fn a_nonsense_identity_is_refused_rather_than_served() {
        let identity = Identity::from_parts(
            vec![1, 2, 3],
            "not a pem".to_string(),
            "nor this".to_string(),
        );

        assert!(rustls_config(&identity).await.is_err());
    }
}

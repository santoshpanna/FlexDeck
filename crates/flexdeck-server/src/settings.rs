//! The agent's own settings, and where they live.
//!
//! These are separate from the *configuration* — which describes what the deck
//! looks like — because they answer a different question: how this particular
//! installation is reached.
//!
//! They are persisted in the store's metadata table, so a restart does not move
//! the agent to a different port and quietly strand every device that had it
//! bookmarked.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use flexdeck_store::{ConfigStore, StoreError};
use serde::{Deserialize, Serialize};

/// The metadata key these settings live under.
pub const SETTINGS_KEY: &str = "settings";

/// Metadata key: whether web access has been turned on.
pub const WEB_ACCESS_KEY: &str = "web_access";

/// Metadata key: the credential the portal issued when this desktop was linked.
///
/// Deliberately **not** a field on [`Settings`]: that blob is configuration a
/// human may read back, and a credential is not.
pub const LINK_TOKEN_KEY: &str = "link_token";

/// Web access is on. Stored as text rather than inferred from a key's presence,
/// because there is no way to delete a metadata key — only to overwrite it.
pub const WEB_ACCESS_ON: &str = "on";

/// Web access is off.
pub const WEB_ACCESS_OFF: &str = "off";

/// The port a fresh installation listens on.
pub const DEFAULT_PORT: u16 = 7878;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error("stored settings could not be read: {0}")]
    Corrupt(serde_json::Error),

    #[error("settings could not be written: {0}")]
    Encode(serde_json::Error),

    #[error("{0}")]
    Invalid(String),
}

/// How this installation is reached.
///
/// Every field carries a `serde` default, so a settings blob written by an older
/// build still loads: a field added later simply takes its default instead of
/// making the whole thing unreadable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The port to listen on, and the port advertised in the pairing QR.
    pub port: u16,

    /// Which interface to accept connections on.
    ///
    /// The default, `0.0.0.0`, accepts them on every interface. That is what a
    /// LAN tool normally wants: a tablet can reach the agent whatever the
    /// machine's address turns out to be, and it is the only bind that can never
    /// disagree with the advertised host.
    pub bind_address: IpAddr,

    /// Overrides the address placed in the pairing QR.
    ///
    /// `None` means "work it out", which is right almost always. It exists
    /// because automatic discovery picks a single interface, and on a machine
    /// with several — a VPN, a second network card — it can pick one a tablet
    /// cannot use.
    pub advertised_host: Option<String>,

    /// The portal this desktop connects to when web access is on.
    ///
    /// `None` means this installation has no portal to talk to. That is not a
    /// failure — a purely local agent is a perfectly good agent — it just means
    /// web access is not *offered*, rather than offered and then broken.
    pub cloud_url: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            bind_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            advertised_host: None,
            cloud_url: None,
        }
    }
}

impl Settings {
    /// Load the stored settings, or the defaults if nothing has been saved.
    ///
    /// Unreadable settings are an **error**, not a silent fallback to defaults.
    /// Resetting the port because a blob was corrupted would strand every paired
    /// device, and the user would have no way to connect that symptom to its
    /// cause.
    pub fn load(store: &ConfigStore) -> Result<Self, SettingsError> {
        let Some(raw) = store.meta(SETTINGS_KEY)? else {
            return Ok(Self::default());
        };

        serde_json::from_str(&raw).map_err(SettingsError::Corrupt)
    }

    /// Persist these settings, after checking that they make sense.
    ///
    /// Validation happens here rather than at load, so an invalid value can
    /// never reach the disk and need cleaning up later.
    pub fn save(&self, store: &ConfigStore) -> Result<(), SettingsError> {
        self.validate()?;

        let raw = serde_json::to_string(self).map_err(SettingsError::Encode)?;
        store.set_meta(SETTINGS_KEY, &raw)?;
        Ok(())
    }

    /// Reject settings that would leave the agent unreachable or ambiguous.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.port == 0 {
            // Port 0 asks the operating system to choose, which is useful in a
            // test and useless as a setting: the chosen port is not known until
            // after the bind, so the pairing QR could never name it.
            return Err(SettingsError::Invalid(
                "port 0 lets the operating system choose, so the pairing QR could not name it"
                    .to_string(),
            ));
        }

        if let Some(host) = &self.advertised_host {
            if host.trim().is_empty() {
                return Err(SettingsError::Invalid(
                    "an advertised host override must not be blank; leave it unset to discover one"
                        .to_string(),
                ));
            }
        }

        if let Some(url) = &self.cloud_url {
            let url = url.trim();

            if url.is_empty() {
                return Err(SettingsError::Invalid(
                    "the portal address must not be blank; clear it instead".to_string(),
                ));
            }

            // Checked HERE as well as at the command line, because the window sets
            // this too. A plain-http address against a service that speaks
            // websockets fails as a dial that never succeeds, which is
            // indistinguishable from a portal that is switched off.
            crate::net::check_portal_url(url).map_err(SettingsError::Invalid)?;
        }

        Ok(())
    }

    /// The address to listen on.
    pub fn bind(&self) -> SocketAddr {
        SocketAddr::new(self.bind_address, self.port)
    }

    /// The host to place in the pairing QR.
    ///
    /// An explicit override wins. Failing that, a *specific* bind address is the
    /// most honest answer available: it is exactly where the agent is listening,
    /// so the QR and the listener cannot disagree. Only a wildcard bind needs
    /// discovery, because "every interface" is not something a client can dial.
    ///
    /// This was not obvious until the agent was actually run. Binding to
    /// `127.0.0.1` while discovery advertised the LAN address produced a QR
    /// pointing at an address nothing was listening on — the very mismatch
    /// `startup_warnings` exists to report.
    pub fn advertised_host(&self) -> String {
        if let Some(host) = &self.advertised_host {
            return host.trim().to_string();
        }

        if !self.bind_address.is_unspecified() {
            return self.bind_address.to_string();
        }

        crate::net::advertised_host()
    }

    /// Whether the advertised host is an override rather than a guess.
    ///
    /// Worth exposing, because a stale override is the one problem here that the
    /// agent cannot detect on its own: it has no way to know whether
    /// `192.168.1.10` is still where it lives.
    pub fn host_is_overridden(&self) -> bool {
        self.advertised_host.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ConfigStore {
        ConfigStore::open_in_memory().expect("store opens")
    }

    #[test]
    fn a_fresh_installation_gets_sensible_defaults() {
        let settings = Settings::load(&store()).expect("loads");

        assert_eq!(settings.port, DEFAULT_PORT);
        assert_eq!(settings.bind_address, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(settings.advertised_host, None);
        assert!(!settings.host_is_overridden());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let store = store();
        let settings = Settings {
            port: 9000,
            bind_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            advertised_host: Some("deckpad.local".to_string()),
            cloud_url: Some("wss://portal.example.com/tunnel".to_string()),
        };

        settings.save(&store).expect("saves");

        assert_eq!(Settings::load(&store).expect("loads"), settings);
    }

    #[test]
    fn a_blob_written_by_an_older_build_still_loads() {
        // Forward compatibility: a field added later must take its default, not
        // render the whole blob unreadable.
        let store = store();
        store
            .set_meta(SETTINGS_KEY, r#"{"port": 9000}"#)
            .expect("writes");

        let settings = Settings::load(&store).expect("loads");
        assert_eq!(settings.port, 9000);
        assert_eq!(settings.bind_address, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(settings.advertised_host, None);
    }

    #[test]
    fn unreadable_settings_are_an_error_rather_than_a_silent_reset() {
        // Falling back to defaults here would move the agent to a different port
        // without telling anyone, and every paired device would simply stop
        // working with no explanation.
        let store = store();
        store
            .set_meta(SETTINGS_KEY, "not json at all")
            .expect("writes");

        assert!(matches!(
            Settings::load(&store),
            Err(SettingsError::Corrupt(_))
        ));
    }

    #[test]
    fn port_zero_is_rejected() {
        let settings = Settings {
            port: 0,
            ..Settings::default()
        };

        assert!(settings.validate().is_err());
        // And it must not reach the disk.
        assert!(settings.save(&store()).is_err());
    }

    #[test]
    fn a_blank_host_override_is_rejected() {
        // Blank is not the same as unset: one means "discover", the other is a
        // mistake that would put an empty host in the QR.
        let settings = Settings {
            advertised_host: Some("   ".to_string()),
            ..Settings::default()
        };

        assert!(settings.validate().is_err());
        assert!(Settings::default().validate().is_ok(), "unset is fine");
    }

    #[test]
    fn a_portal_address_must_be_a_websocket_address() {
        // Checked here because the WINDOW sets this too, not only the command
        // line. A plain-http address against a service that speaks websockets
        // fails as a dial that never succeeds, which is indistinguishable from a
        // portal that is switched off.
        let with_url = |url: &str| Settings {
            cloud_url: Some(url.to_string()),
            ..Settings::default()
        };

        assert!(
            matches!(
                with_url("http://127.0.0.1:8787/tunnel").validate(),
                Err(SettingsError::Invalid(_))
            ),
            "a http:// address would never connect"
        );

        assert!(
            with_url("   ").validate().is_err(),
            "blank is not an address"
        );
        assert!(with_url("wss://portal.example.com/tunnel")
            .validate()
            .is_ok());
        assert!(
            Settings::default().validate().is_ok(),
            "having no portal is a perfectly good configuration"
        );
    }

    #[test]
    fn binding_combines_the_address_and_the_port() {
        let settings = Settings {
            port: 9000,
            bind_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            advertised_host: None,
            cloud_url: None,
        };

        assert_eq!(settings.bind().to_string(), "10.0.0.5:9000");
    }

    #[test]
    fn an_override_wins_over_discovery() {
        let settings = Settings {
            advertised_host: Some("  192.168.1.99  ".to_string()),
            ..Settings::default()
        };

        // Trimmed, because a stray space would produce a QR nobody could use.
        assert_eq!(settings.advertised_host(), "192.168.1.99");
        assert!(settings.host_is_overridden());
    }

    #[test]
    fn a_specific_bind_address_means_no_discovery_is_needed() {
        // Discovery picks a single interface, and on a machine with several it
        // can pick one that is not the one being listened on.
        let settings = Settings {
            bind_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            ..Settings::default()
        };

        assert_eq!(settings.advertised_host(), "192.168.1.50");
    }

    #[test]
    fn a_wildcard_bind_still_discovers_a_real_address() {
        // "Every interface" is not something a tablet can dial, so this is the
        // one case where discovery is the only option.
        let host = Settings::default().advertised_host();

        assert!(!host.is_empty());
        assert!(
            !host.starts_with("0.0.0.0") && host != "::",
            "a wildcard is not an address to hand out: {host}"
        );
        assert!(
            crate::net::is_reachable_host(&host) || host == "127.0.0.1",
            "unexpected discovered host: {host}"
        );
    }

    #[test]
    fn an_override_still_beats_a_specific_bind_address() {
        // The override exists precisely to second-guess the automatic answer.
        let settings = Settings {
            bind_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            advertised_host: Some("deckpad.local".to_string()),
            ..Settings::default()
        };

        assert_eq!(settings.advertised_host(), "deckpad.local");
    }

    #[test]
    fn saving_does_not_disturb_the_revision_log() {
        // Settings and the configuration share one metadata table, so this is a
        // real way to corrupt the store if the keys ever collided.
        let store = store();
        store
            .commit(r#"{"schema":1,"profiles":[]}"#)
            .expect("commits");

        Settings {
            port: 9000,
            ..Settings::default()
        }
        .save(&store)
        .expect("saves");

        assert_eq!(store.rev().expect("reads"), Some(1));
        assert_eq!(Settings::load(&store).expect("loads").port, 9000);
    }
}

//! Checks worth running before the agent starts serving.
//!
//! Every warning here has a legitimate reason to be true — same-machine testing,
//! a deliberate dry run — so none of them is an error. But each one produces a
//! failure that is genuinely confusing to diagnose later, and they share a
//! property that makes them worth catching up front: **nothing looks wrong from
//! the machine running the agent.** The QR renders, the server is listening, the
//! logs are quiet. The problem only exists from a tablet across the room.
//!
//! So they are collected and handed to the caller, which decides what to show.

use std::net::{IpAddr, SocketAddr};

/// Something worth telling the user about before the agent starts serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupWarning {
    /// The address in the pairing QR cannot be reached from another device, so
    /// scanning it points a tablet at nothing.
    QrAddressUnreachable,

    /// The agent advertises one address but listens on another, so a client will
    /// knock on a door with nothing behind it. This is the nastier of the two:
    /// the QR looks perfectly plausible.
    AdvertisedAddressNotBound,

    /// No live executor is installed, so presses are accepted and do nothing.
    PressesDoNothing,
}

impl StartupWarning {
    /// A short explanation, so a shell can surface this without inventing
    /// wording — and without the wording drifting from what the check means.
    pub fn message(self) -> &'static str {
        match self {
            Self::QrAddressUnreachable => {
                "the address in the pairing QR is not reachable from another device"
            }
            Self::AdvertisedAddressNotBound => {
                "the agent advertises an address it is not listening on"
            }
            Self::PressesDoNothing => {
                "no live action executor is installed, so button presses will do nothing"
            }
        }
    }
}

/// Check a listener address, an advertised host, and the executor state together.
pub fn check(
    bind: SocketAddr,
    advertised_host: &str,
    has_live_executor: bool,
) -> Vec<StartupWarning> {
    let mut warnings = Vec::new();

    if !crate::net::is_reachable_host(advertised_host) {
        warnings.push(StartupWarning::QrAddressUnreachable);
    }

    if is_mismatched(bind, advertised_host) {
        warnings.push(StartupWarning::AdvertisedAddressNotBound);
    }

    if !has_live_executor {
        warnings.push(StartupWarning::PressesDoNothing);
    }

    warnings
}

/// Whether the listener would refuse connections to the address being advertised.
///
/// A wildcard bind (`0.0.0.0` or `::`) listens on every interface, so it can
/// never mismatch. A hostname is taken at its word: resolving it is not this
/// function's job, and guessing would produce false alarms.
fn is_mismatched(bind: SocketAddr, advertised_host: &str) -> bool {
    let bind_ip = bind.ip();
    if bind_ip.is_unspecified() {
        return false;
    }

    match advertised_host.parse::<IpAddr>() {
        Ok(advertised) => advertised != bind_ip,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(text: &str) -> SocketAddr {
        text.parse().expect("a literal socket address")
    }

    #[test]
    fn a_working_lan_setup_produces_no_warnings() {
        let warnings = check(bind("0.0.0.0:7878"), "192.168.1.10", true);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_wildcard_bind_never_mismatches() {
        // Listening on everything cannot fail to include the advertised address,
        // whatever it is.
        for host in ["192.168.1.10", "10.0.0.5", "203.0.113.7"] {
            let warnings = check(bind("0.0.0.0:7878"), host, true);
            assert!(warnings.is_empty(), "{host} produced {warnings:?}");
        }
    }

    #[test]
    fn advertising_loopback_flags_the_qr_as_unusable() {
        let warnings = check(bind("127.0.0.1:7878"), "127.0.0.1", true);

        // Binding and advertising loopback is *consistent*, so there is no
        // mismatch — but a QR built from it still cannot work from a tablet.
        assert_eq!(warnings, vec![StartupWarning::QrAddressUnreachable]);
    }

    #[test]
    fn advertising_a_lan_address_but_listening_only_on_loopback_is_flagged() {
        // The scenario that looks completely fine from the agent's own machine.
        let warnings = check(bind("127.0.0.1:7878"), "192.168.1.10", true);

        assert!(warnings.contains(&StartupWarning::AdvertisedAddressNotBound));
        assert!(
            !warnings.contains(&StartupWarning::QrAddressUnreachable),
            "the advertised address is perfectly reachable; the agent just is not there"
        );
    }

    #[test]
    fn listening_on_a_different_lan_address_is_flagged() {
        // Two interfaces, and the wrong one chosen.
        let warnings = check(bind("192.168.1.10:7878"), "192.168.1.99", true);
        assert_eq!(warnings, vec![StartupWarning::AdvertisedAddressNotBound]);
    }

    #[test]
    fn listening_on_the_advertised_address_is_not_flagged() {
        let warnings = check(bind("192.168.1.10:7878"), "192.168.1.10", true);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_hostname_is_not_second_guessed() {
        // "deckpad.local" might resolve to anything; guessing would produce
        // false alarms, which is worse than staying quiet.
        let warnings = check(bind("192.168.1.10:7878"), "deckpad.local", true);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_missing_live_executor_is_reported_on_its_own() {
        let warnings = check(bind("0.0.0.0:7878"), "192.168.1.10", false);
        assert_eq!(warnings, vec![StartupWarning::PressesDoNothing]);
    }

    #[test]
    fn several_problems_are_all_reported() {
        // A caller fixing one thing at a time should not have to re-run to find
        // the next, so the check does not stop at the first problem.
        let warnings = check(bind("127.0.0.1:7878"), "127.0.0.1", false);

        assert_eq!(
            warnings,
            vec![
                StartupWarning::QrAddressUnreachable,
                StartupWarning::PressesDoNothing,
            ]
        );
    }

    #[test]
    fn every_warning_explains_itself_distinctly() {
        let messages: Vec<&str> = [
            StartupWarning::QrAddressUnreachable,
            StartupWarning::AdvertisedAddressNotBound,
            StartupWarning::PressesDoNothing,
        ]
        .iter()
        .map(|warning| warning.message())
        .collect();

        for message in &messages {
            assert!(message.len() > 20, "too terse to be useful: {message}");
        }

        // Distinct, so a copy-paste slip in the match cannot hide behind a test
        // that only checks each one is non-empty.
        let mut sorted = messages.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            messages.len(),
            "duplicate wording in {messages:?}"
        );
    }
}

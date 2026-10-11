//! Working out which address to advertise to clients.
//!
//! The pairing QR has to contain an address a tablet can actually reach, and
//! getting that wrong is unusually unpleasant to debug: a QR carrying
//! `127.0.0.1` scans *perfectly*, the tablet connects to itself, and the failure
//! surfaces as an unexplained timeout with nothing in any log.
//!
//! So discovery is explicit and its result is checkable, rather than defaulting
//! to a loopback address that looks like it worked.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// Could another device on the network reach this address?
///
/// Deliberately *not* "is it private". A machine behind NAT with a public
/// address is perfectly reachable from its own LAN, so requiring a private range
/// would reject working setups. What is excluded is everything that cannot be
/// reached from another device at all — loopback above all, because that is the
/// one that produces a QR nobody can use.
pub fn is_usable_lan_address(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            !v4.is_loopback()
                && !v4.is_unspecified()
                && !v4.is_broadcast()
                && !v4.is_multicast()
                && !v4.is_link_local()
        }
        IpAddr::V6(v6) => {
            !v6.is_loopback()
                && !v6.is_unspecified()
                && !v6.is_multicast()
                && !v6.is_unicast_link_local()
        }
    }
}

/// Whether a host string is worth putting in a pairing QR.
///
/// A hostname is taken at its word: resolving names is not this function's job,
/// and `localhost` is excluded by name because it resolves to loopback on
/// essentially every machine, which is exactly the trap this module exists to
/// avoid.
pub fn is_reachable_host(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return false;
    }

    match host.parse::<IpAddr>() {
        Ok(address) => is_usable_lan_address(&address),
        // A name we did not recognise is assumed resolvable by the client.
        Err(_) => true,
    }
}

/// Ask the OS which local address it would use to reach the network.
///
/// This opens a UDP socket and *connects* it. `connect` on a UDP socket sends
/// nothing — it sets the default peer and, in doing so, makes the kernel resolve
/// which local interface the route would leave through. So this is safe offline
/// and contacts no external host.
///
/// `None` is the honest answer on a machine with no usable route.
pub fn discover_lan_address() -> Option<IpAddr> {
    // TEST-NET-1 from RFC 5737: a documentation-only range, guaranteed not to be
    // a real host. Used precisely so that even if a packet were somehow emitted,
    // it would go nowhere observable. Probing something like 8.8.8.8 would be
    // both leaky and rude.
    let probe = SocketAddr::from(([192, 0, 2, 1_u8], 9));

    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(probe).ok()?;

    let address = socket.local_addr().ok()?.ip();
    is_usable_lan_address(&address).then_some(address)
}

/// The address to advertise, falling back to loopback when there is no route.
///
/// The fallback keeps the agent usable for same-machine testing. Callers that
/// are about to show a QR should check [`is_reachable_host`] first.
pub fn advertised_host() -> String {
    discover_lan_address()
        .map(|address| address.to_string())
        .unwrap_or_else(|| Ipv4Addr::LOCALHOST.to_string())
}

/// Whether a URL's host is this machine: `localhost`, `127.0.0.0/8` or `::1`.
pub fn is_loopback_url(url: &str) -> bool {
    let Some((_, rest)) = url.trim().split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Strip any userinfo, then the port — minding IPv6's brackets.
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = if let Some(inner) = host_port.strip_prefix('[') {
        inner.split(']').next().unwrap_or_default()
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Check a portal (tunnel) address: `wss://` anywhere, plain `ws://` only to
/// this machine.
///
/// Over plain `ws://` anyone on the path can read and inject tunnel frames,
/// including the relayed requests that configure this desktop. Loopback is allowed because that is how the cloud is
/// developed and tested, and nothing on a network sees it.
pub fn check_portal_url(url: &str) -> Result<(), String> {
    if url.starts_with("wss://") {
        return Ok(());
    }
    if url.starts_with("ws://") {
        return if is_loopback_url(url) {
            Ok(())
        } else {
            Err(format!(
                "'{url}' is not encrypted; use wss:// (plain ws:// is only allowed to this computer)"
            ))
        };
    }
    Err(format!(
        "'{url}' is not a websocket address; it must start with wss://"
    ))
}

/// Check a download address: `https://` anywhere, plain `http://` only to this
/// machine. A plugin is code that will run here; fetching it in the clear lets
/// anyone on the path swap it.
pub fn check_download_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || (url.starts_with("http://") && is_loopback_url(url)) {
        Ok(())
    } else if url.starts_with("http://") {
        Err(format!(
            "'{url}' is not encrypted; use https:// (plain http:// is only allowed to this computer)"
        ))
    } else {
        Err(format!("'{url}' is not a web address"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("a literal address")
    }

    #[test]
    fn loopback_is_never_usable() {
        // The whole reason this module exists.
        assert!(!is_usable_lan_address(&ip("127.0.0.1")));
        assert!(!is_usable_lan_address(&ip("127.1.2.3")));
        assert!(!is_usable_lan_address(&ip("::1")));
    }

    #[test]
    fn addresses_no_device_could_reach_are_rejected() {
        assert!(!is_usable_lan_address(&ip("0.0.0.0")), "unspecified");
        assert!(!is_usable_lan_address(&ip("255.255.255.255")), "broadcast");
        assert!(!is_usable_lan_address(&ip("224.0.0.1")), "IPv4 multicast");
        assert!(!is_usable_lan_address(&ip("ff02::1")), "IPv6 multicast");
        assert!(
            !is_usable_lan_address(&ip("169.254.10.1")),
            "IPv4 link-local"
        );
        assert!(!is_usable_lan_address(&ip("fe80::1")), "IPv6 link-local");
    }

    #[test]
    fn ordinary_lan_addresses_are_usable() {
        for address in ["192.168.1.10", "10.0.0.5", "172.16.4.4", "203.0.113.7"] {
            assert!(is_usable_lan_address(&ip(address)), "{address}");
        }
    }

    #[test]
    fn a_public_address_is_usable_because_nat_exists() {
        // Rejecting non-private addresses would break a machine that is simply
        // not behind NAT.
        assert!(is_usable_lan_address(&ip("198.51.100.9")));
    }

    #[test]
    fn the_localhost_name_is_rejected_as_well_as_the_address() {
        // "localhost" parses as a *name*, not an IP, so the address check alone
        // would wave it through.
        assert!(!is_reachable_host("localhost"));
        assert!(!is_reachable_host("LOCALHOST"));
        assert!(!is_reachable_host("  localhost  "));
        assert!(!is_reachable_host("127.0.0.1"));
    }

    #[test]
    fn an_empty_host_is_not_reachable() {
        assert!(!is_reachable_host(""));
        assert!(!is_reachable_host("   "));
    }

    #[test]
    fn a_lan_address_or_a_hostname_is_reachable() {
        assert!(is_reachable_host("192.168.1.10"));
        assert!(is_reachable_host("deckpad.local"));
    }

    #[test]
    fn discovery_never_reports_an_address_a_device_could_not_reach() {
        // The outcome depends on this machine's network, which is fine: the
        // property that must hold either way is that the answer is honest. A
        // machine with no route should say so rather than offer loopback.
        if let Some(address) = discover_lan_address() {
            assert!(is_usable_lan_address(&address), "discovered {address}");
        }
    }

    #[test]
    fn the_advertised_host_is_always_something_we_could_show_a_user() {
        let host = advertised_host();
        assert!(!host.is_empty());
        // Reachable or loopback, but never nonsense.
        assert!(
            is_reachable_host(&host) || host == "127.0.0.1",
            "unexpected advertised host: {host}"
        );
    }

    #[test]
    fn loopback_urls_are_recognised_and_lookalikes_are_not() {
        for url in [
            "ws://127.0.0.1:8787/tunnel",
            "ws://localhost/tunnel",
            "http://[::1]:80/x",
            "ws://127.9.9.9/t",
        ] {
            assert!(is_loopback_url(url), "{url}");
        }
        for url in [
            "ws://portal.example/tunnel",
            "ws://127.0.0.1.evil.example/t",
            "ws://localhost.evil.example/t",
            "ws://evil.example/?h=127.0.0.1",
            "not a url",
        ] {
            assert!(!is_loopback_url(url), "{url}");
        }
    }

    #[test]
    fn plain_text_is_allowed_only_to_this_computer() {
        assert!(check_portal_url("wss://portal.example/tunnel").is_ok());
        assert!(check_portal_url("ws://127.0.0.1:8787/tunnel").is_ok());
        assert!(check_portal_url("ws://portal.example/tunnel").is_err());
        assert!(check_portal_url("https://portal.example").is_err());

        assert!(check_download_url("https://example.com/p.fdpkg").is_ok());
        assert!(check_download_url("http://127.0.0.1:8787/p.fdpkg").is_ok());
        assert!(check_download_url("http://example.com/p.fdpkg").is_err());
        assert!(check_download_url("file:///etc/passwd").is_err());
    }
}

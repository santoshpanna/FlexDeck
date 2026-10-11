//! Browser pairing codes, as the cloud routes them.
//!
//! A person reads a code off their desktop and types it into the website. The
//! website has no other way to know which desktop they mean — there are no
//! accounts — so the desktop tells the cloud, up its proven tunnel, "a code
//! whose hash is H is open here until T". The cloud keeps only that mapping.
//!
//! The hash, not the code: the cloud needs to *find* the desktop, not to know
//! the code. The code itself crosses the cloud once, when a browser redeems it,
//! and is checked by the desktop.
//!
//! Each desktop holds at most one offer, and offers die with their tunnel, so
//! this map is bounded by the number of connected desktops.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Offer {
    fingerprint: String,
    expires_at: i64,
}

/// Open pairing codes, by the hash of the code.
#[derive(Debug, Default)]
pub struct Offers {
    by_hash: HashMap<String, Offer>,
    /// The hash each desktop currently offers, so a new offer replaces its old one.
    by_fingerprint: HashMap<String, String>,
}

impl Offers {
    /// Record what `fingerprint` is offering, replacing anything it offered
    /// before. An empty hash, or an expiry already past, withdraws its offer.
    pub fn offer(&mut self, fingerprint: &str, code_hash: &str, expires_at: i64, now: i64) {
        self.withdraw(fingerprint);
        if code_hash.is_empty() || expires_at <= now {
            return;
        }

        // Two desktops can only collide on a hash if they drew the same 40-bit
        // code in the same few minutes. Refusing to overwrite keeps the first
        // desktop's code working; the second's simply will not route.
        if self.by_hash.contains_key(code_hash) {
            return;
        }

        self.by_hash.insert(
            code_hash.to_string(),
            Offer {
                fingerprint: fingerprint.to_string(),
                expires_at,
            },
        );
        self.by_fingerprint
            .insert(fingerprint.to_string(), code_hash.to_string());
    }

    /// Forget whatever `fingerprint` offered: its tunnel has gone.
    pub fn withdraw(&mut self, fingerprint: &str) {
        if let Some(hash) = self.by_fingerprint.remove(fingerprint) {
            self.by_hash.remove(&hash);
        }
    }

    /// The desktop a typed code belongs to, if one is open for it right now.
    pub fn route(&mut self, code_hash: &str, now: i64) -> Option<String> {
        let offer = self.by_hash.get(code_hash)?.clone();
        if offer.expires_at <= now {
            self.withdraw(&offer.fingerprint);
            return None;
        }
        Some(offer.fingerprint)
    }

    /// Drop every expired offer.
    pub fn sweep(&mut self, now: i64) {
        let expired: Vec<String> = self
            .by_hash
            .values()
            .filter(|offer| offer.expires_at <= now)
            .map(|offer| offer.fingerprint.clone())
            .collect();
        for fingerprint in expired {
            self.withdraw(&fingerprint);
        }
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000;

    #[test]
    fn an_open_code_routes_to_its_desktop_until_it_expires() {
        let mut offers = Offers::default();
        offers.offer("desk-a", "hash-1", NOW + 300, NOW);

        assert_eq!(offers.route("hash-1", NOW).as_deref(), Some("desk-a"));
        assert_eq!(
            offers.route("hash-1", NOW + 300),
            None,
            "the instant is too late"
        );
        assert!(offers.is_empty(), "an expired offer is dropped when found");
    }

    #[test]
    fn a_new_offer_replaces_the_desktops_old_one() {
        let mut offers = Offers::default();
        offers.offer("desk-a", "hash-1", NOW + 300, NOW);
        offers.offer("desk-a", "hash-2", NOW + 300, NOW);

        assert_eq!(offers.route("hash-1", NOW), None);
        assert_eq!(offers.route("hash-2", NOW).as_deref(), Some("desk-a"));
        assert_eq!(offers.len(), 1);
    }

    #[test]
    fn an_empty_hash_withdraws_and_a_departed_tunnel_takes_its_offer() {
        let mut offers = Offers::default();
        offers.offer("desk-a", "hash-1", NOW + 300, NOW);
        offers.offer("desk-a", "", 0, NOW);
        assert!(offers.is_empty());

        offers.offer("desk-b", "hash-2", NOW + 300, NOW);
        offers.withdraw("desk-b");
        assert_eq!(offers.route("hash-2", NOW), None);
    }

    #[test]
    fn a_colliding_hash_does_not_steal_the_first_desktops_code() {
        let mut offers = Offers::default();
        offers.offer("desk-a", "same", NOW + 300, NOW);
        offers.offer("desk-b", "same", NOW + 300, NOW);

        assert_eq!(offers.route("same", NOW).as_deref(), Some("desk-a"));
    }

    #[test]
    fn sweeping_drops_only_what_has_expired() {
        let mut offers = Offers::default();
        offers.offer("desk-a", "old", NOW + 10, NOW);
        offers.offer("desk-b", "new", NOW + 300, NOW);

        offers.sweep(NOW + 10);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers.route("new", NOW + 10).as_deref(), Some("desk-b"));
    }
}

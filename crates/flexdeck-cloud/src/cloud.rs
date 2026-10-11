//! The cloud's shared state.
//!
//! Deliberately small. There are no accounts and nothing that can act on a
//! desktop: the cloud keeps the live tunnels, the pairing codes desktops have
//! asked it to route, and the limits that keep both from being abused.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::Semaphore;

use crate::hub::Hub;
use crate::limit::RateLimiter;
use crate::offers::Offers;

/// How many desktops may hold a tunnel at once.
///
/// A ceiling, not a target: each tunnel is a task and a socket, and an
/// unauthenticated client could otherwise open them until the process fell over.
pub const MAX_TUNNELS: usize = 1_000;

/// How many pairing attempts one address may make per minute.
///
/// A person types a code once, maybe twice. Ten a minute is generous to them
/// and, against a 40-bit code, makes guessing hopeless.
pub const PAIR_ATTEMPTS_PER_MINUTE: u32 = 10;

/// How many relayed requests one address may make per minute.
pub const RELAYS_PER_MINUTE: u32 = 600;

/// Everything the cloud's handlers share.
pub struct Cloud {
    hub: Hub,
    offers: Mutex<Offers>,
    tunnels: Arc<Semaphore>,
    pair_limit: RateLimiter,
    relay_limit: RateLimiter,
    /// Whether to believe `X-Forwarded-For` for the client's address. Only
    /// behind a reverse proxy that sets it; otherwise any client could claim
    /// any address and walk around the limits.
    trust_proxy: bool,
}

impl Default for Cloud {
    fn default() -> Self {
        Self::new()
    }
}

impl Cloud {
    pub fn new() -> Self {
        Self::with_hub(Hub::default())
    }

    /// A cloud with a particular hub, so a test can shorten its timeout.
    pub fn with_hub(hub: Hub) -> Self {
        Self {
            hub,
            offers: Mutex::new(Offers::default()),
            tunnels: Arc::new(Semaphore::new(MAX_TUNNELS)),
            pair_limit: RateLimiter::new(PAIR_ATTEMPTS_PER_MINUTE, Duration::from_secs(60)),
            relay_limit: RateLimiter::new(RELAYS_PER_MINUTE, Duration::from_secs(60)),
            trust_proxy: false,
        }
    }

    /// Believe `X-Forwarded-For` for client addresses.
    pub fn trusting_proxy(mut self) -> Self {
        self.trust_proxy = true;
        self
    }

    /// Live tunnels. Takes `&self` because the hub does its own locking.
    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    /// Open pairing codes, by hash.
    pub fn offers(&self) -> MutexGuard<'_, Offers> {
        self.offers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Slots for tunnels. A connection holds one for as long as it is open.
    pub fn tunnel_slots(&self) -> Arc<Semaphore> {
        Arc::clone(&self.tunnels)
    }

    pub fn pair_limit(&self) -> &RateLimiter {
        &self.pair_limit
    }

    pub fn relay_limit(&self) -> &RateLimiter {
        &self.relay_limit
    }

    pub fn trusts_proxy(&self) -> bool {
        self.trust_proxy
    }
}

/// The current time in unix seconds.
///
/// Passed *into* the logic that uses it rather than read inside it, which is
/// what keeps expiry testable at its boundaries without sleeping.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

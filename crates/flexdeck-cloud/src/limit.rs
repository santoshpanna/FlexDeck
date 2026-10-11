//! A small per-address rate limiter.
//!
//! The routes that matter are guessable: a pairing code is 40 bits, and a
//! limiter is what turns "40 bits" into "not in this lifetime". Fixed windows,
//! counted per client address, in memory — enough for one process, which is
//! what the cloud is.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Past this many tracked addresses, stale entries are swept before adding one,
/// so a flood of distinct addresses cannot grow the map without bound.
const SWEEP_AT: usize = 10_000;

/// At most `limit` hits per address in any `window`.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    hits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Count a hit from `address`, and say whether it is within the limit.
    pub fn allow(&self, address: IpAddr) -> bool {
        self.allow_at(address, Instant::now())
    }

    fn allow_at(&self, address: IpAddr, now: Instant) -> bool {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());

        if hits.len() >= SWEEP_AT && !hits.contains_key(&address) {
            let window = self.window;
            hits.retain(|_, (started, _)| now.duration_since(*started) < window);
        }

        let entry = hits.entry(address).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(last: u8) -> IpAddr {
        IpAddr::from([192, 0, 2, last])
    }

    #[test]
    fn the_limit_holds_within_a_window_and_resets_after_it() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        let start = Instant::now();

        for _ in 0..3 {
            assert!(limiter.allow_at(address(1), start));
        }
        assert!(
            !limiter.allow_at(address(1), start),
            "the fourth is refused"
        );
        assert!(
            limiter.allow_at(address(2), start),
            "per address, not global"
        );

        assert!(limiter.allow_at(address(1), start + Duration::from_secs(60)));
    }
}

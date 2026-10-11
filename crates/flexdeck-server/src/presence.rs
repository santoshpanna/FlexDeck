//! Who is connected right now, as opposed to who is merely paired.
//!
//! The store answers "which devices have redeemed a PIN". This answers "which of
//! them has a socket open at this moment", which is a different question and the
//! one a user actually asks when they want to know whether their tablet is
//! talking to this machine.
//!
//! # Why a count and not a boolean
//!
//! One device can hold more than one socket — a reconnect that overlaps the old
//! connection is ordinary, not exceptional. A boolean set on connect and cleared
//! on disconnect would report the device offline the instant the *first* of two
//! sockets closed. A count per device does not, and the device is offline only
//! when the last one goes.
//!
//! # Why a guard and not a cleanup call
//!
//! The count is changed by [`Presence::connect`] and restored by the returned
//! [`PresenceGuard`]'s `Drop`. A caller therefore cannot leave a device stuck
//! "online" by forgetting a call on one of several exit paths — the socket's own
//! task owns the guard, so when the task ends, for any reason, the count follows.
//!
//! # Why a signal and not a payload
//!
//! [`Presence::changed`] announces that something changed and carries nothing,
//! so a subscriber re-reads the current state rather than applying a delta. That
//! is the same reasoning as the configuration channel carrying a revision: a
//! subscriber that missed a change still gets one correct answer instead of a
//! delta it cannot place.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

/// How many change notifications may be outstanding before a slow subscriber is
/// told it lagged. Lagging is safe — the subscriber re-reads the state — so this
/// can be small.
const CHANGE_BUFFER: usize = 32;

/// The live connections, keyed by device id.
pub struct Presence {
    open: Mutex<HashMap<String, usize>>,
    changes: broadcast::Sender<()>,
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence {
    pub fn new() -> Self {
        let (changes, _) = broadcast::channel(CHANGE_BUFFER);
        Self {
            open: Mutex::new(HashMap::new()),
            changes,
        }
    }

    /// Record a live socket for `device_id`, keeping it recorded until the
    /// returned guard is dropped.
    ///
    /// Takes `&Arc<Self>` rather than `&self` because the guard must outlive the
    /// borrow: it owns a share of this registry so that dropping it can update
    /// the count.
    pub fn connect(self: &Arc<Self>, device_id: &str) -> PresenceGuard {
        {
            let mut open = self.lock();
            *open.entry(device_id.to_string()).or_insert(0) += 1;
        }

        self.announce();

        PresenceGuard {
            presence: Arc::clone(self),
            device_id: device_id.to_string(),
        }
    }

    /// Whether `device_id` has at least one socket open.
    pub fn is_online(&self, device_id: &str) -> bool {
        self.lock()
            .get(device_id)
            .copied()
            .is_some_and(|count| count > 0)
    }

    /// Watch for connections opening and closing.
    pub fn changed(&self) -> broadcast::Receiver<()> {
        self.changes.subscribe()
    }

    /// How many devices are tracked. Zero means nobody is connected.
    fn tracked(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, usize>> {
        self.open.lock().expect("the presence lock is not poisoned")
    }

    /// Tell subscribers something moved. No listeners is normal, not an error.
    fn announce(&self) {
        let _ = self.changes.send(());
    }
}

/// Deliberately implemented by hand rather than derived.
///
/// A derived `Debug` would print the whole map, so any `{:?}` in a log line or a
/// panic message would become a list of which of the user's devices are
/// connected. Only the count is shown.
impl fmt::Debug for Presence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Presence")
            .field("connected", &self.tracked())
            .finish()
    }
}

/// A live connection. Dropping it is what takes the device offline.
#[derive(Debug)]
pub struct PresenceGuard {
    presence: Arc<Presence>,
    device_id: String,
}

impl Drop for PresenceGuard {
    fn drop(&mut self) {
        {
            let mut open = self.presence.lock();
            // `saturating_sub` because underflow here would report a connected
            // device as very much offline, and removing the key at zero keeps
            // "tracked" meaning "has a live socket" rather than "was seen at
            // some point".
            if let Some(count) = open.get_mut(&self.device_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    open.remove(&self.device_id);
                }
            }
        }

        self.presence.announce();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_is_online_while_its_socket_is_open() {
        let presence = Arc::new(Presence::new());
        assert!(!presence.is_online("tab1"));

        let guard = presence.connect("tab1");
        assert!(presence.is_online("tab1"));

        drop(guard);
        assert!(
            !presence.is_online("tab1"),
            "dropping the socket takes the device offline"
        );
    }

    #[test]
    fn dropping_one_of_two_sockets_leaves_the_device_online() {
        // The bug this exists to catch: setting the count to zero instead of
        // decrementing, which would report a still-connected device as offline
        // the moment any one of its sockets closed.
        let presence = Arc::new(Presence::new());
        let first = presence.connect("tab1");
        let second = presence.connect("tab1");

        drop(first);
        assert!(presence.is_online("tab1"), "one socket is still open");

        drop(second);
        assert!(!presence.is_online("tab1"), "the last one is gone");
    }

    #[test]
    fn devices_are_tracked_independently() {
        let presence = Arc::new(Presence::new());
        let first = presence.connect("tab1");
        let _second = presence.connect("tab2");

        drop(first);
        assert!(!presence.is_online("tab1"));
        assert!(presence.is_online("tab2"));
    }

    #[test]
    fn a_dropped_guard_leaves_no_trace() {
        // A zero-count entry left in the map would be indistinguishable from a
        // live one to anything that reads the map without going through
        // `is_online`.
        let presence = Arc::new(Presence::new());
        let guard = presence.connect("tab1");

        drop(guard);

        assert_eq!(presence.tracked(), 0);
    }

    #[tokio::test]
    async fn a_change_is_announced_both_when_a_socket_opens_and_when_it_closes() {
        // Without this, a window would only ever see the state it happened to
        // read first, which is exactly the "stale presence" failure.
        let presence = Arc::new(Presence::new());
        let mut changes = presence.changed();

        let guard = presence.connect("tab1");
        changes.recv().await.expect("announced on connect");

        drop(guard);
        changes.recv().await.expect("announced on drop");
    }

    #[test]
    fn debug_never_prints_a_device_id() {
        // A log line or a panic message must not become a list of which of the
        // user's devices are connected.
        let presence = Arc::new(Presence::new());
        let _guard = presence.connect("kitchen-tablet");

        let printed = format!("{presence:?}");

        assert!(!printed.contains("kitchen-tablet"), "{printed}");
    }
}

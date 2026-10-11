//! Matching responses to the requests that are waiting for them.
//!
//! The cloud issues a request down a tunnel and has to hold a caller while the
//! desktop answers. That is a small amount of bookkeeping with three failure
//! modes worth naming, all of which are tested here without a socket:
//!
//! * **A response nobody is waiting for.** Either the caller gave up (normal) or
//!   the id was invented (a bug). Collapsing those two into one silent `false`
//!   would hide the second, which is the informative one.
//! * **An id reused too early.** If an id that is still in flight were handed
//!   out again, a late response would resolve the *wrong* request. Ids are
//!   therefore monotonic and never reused — the same rule the revision log
//!   needed, for the same reason.
//! * **A tunnel that dies with work in flight.** Every waiter must be released,
//!   or a portal request hangs until its client times out and the map leaks a
//!   sender per dead request.

use std::collections::HashMap;

use flexdeck_protocol::TunnelResponse;
use tokio::sync::oneshot;

use crate::TunnelError;

/// The most requests one tunnel may have in flight at once.
///
/// A portal that issues requests faster than the desktop answers would
/// otherwise grow the pending map without bound, from a client we are not
/// supposed to trust with that much. Reaching the cap is a *refusal*, so the
/// pressure shows up as an error the portal can report instead of as memory the
/// desktop never gets back.
pub const MAX_IN_FLIGHT: usize = 64;

/// What became of a response we were asked to deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    /// Handed to whoever was waiting.
    Delivered,

    /// The caller had already stopped waiting — normally because the portal
    /// disconnected while the desktop was still working on the request.
    Abandoned,

    /// No request ever carried this id. That is not a race; it is a bug on the
    /// other end, and the caller is told so it can be logged as one.
    Unknown,
}

/// Allocates request ids and matches responses back to their waiters.
#[derive(Debug, Default)]
pub struct Correlator {
    next_id: u64,
    pending: HashMap<u64, oneshot::Sender<TunnelResponse>>,
}

impl Correlator {
    pub fn new() -> Self {
        Self {
            // Start at 1 so that 0 is never a valid id. `TunnelResponse.id` is a
            // proto3 scalar, and a default is *omitted* rather than sent as
            // zero — so a zero id would be indistinguishable from a malformed
            // response on the wire.
            next_id: 1,
            pending: HashMap::new(),
        }
    }

    /// Reserve an id and a receiver for its response.
    pub fn issue(&mut self) -> Result<(u64, oneshot::Receiver<TunnelResponse>), TunnelError> {
        if self.pending.len() >= MAX_IN_FLIGHT {
            return Err(TunnelError::TooManyInFlight {
                limit: MAX_IN_FLIGHT,
            });
        }

        let id = self.next_id;
        // Monotonic, and never rewound even when a request is abandoned. An id
        // that were reused while a stale response was still in flight would
        // deliver that response to the wrong caller, and the wrong answer is
        // worse than no answer.
        self.next_id += 1;

        let (sender, receiver) = oneshot::channel();
        self.pending.insert(id, sender);

        Ok((id, receiver))
    }

    /// Hand a response to whoever is waiting for it.
    pub fn resolve(&mut self, response: TunnelResponse) -> Resolved {
        match self.pending.remove(&response.id) {
            Some(waiter) => match waiter.send(response) {
                Ok(()) => Resolved::Delivered,
                // The receiver was dropped: the caller gave up first.
                Err(_) => Resolved::Abandoned,
            },
            None => Resolved::Unknown,
        }
    }

    /// Give up on a request before its response arrives.
    ///
    /// Called when a request times out. Without this the entry would sit in the
    /// map until the tunnel died, counting against the in-flight cap and
    /// eventually refusing new work for a desktop that is merely slow — a slow
    /// leak that looks like "suddenly everything is refused".
    ///
    /// The id is still never reused, so a late response for it resolves nothing.
    /// Returns whether anything was actually waiting.
    pub fn abandon(&mut self, id: u64) -> bool {
        self.pending.remove(&id).is_some()
    }

    /// Release everyone waiting, because the tunnel is gone.
    ///
    /// Dropping the senders makes each awaiting receiver fail, so a portal
    /// request returns an error instead of hanging. Skipping this is how a dead
    /// tunnel turns into a hung request plus a leaked sender for every request
    /// that was in flight.
    pub fn fail_all(&mut self) {
        self.pending.clear();
    }

    /// How many requests are waiting for an answer.
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use flexdeck_protocol::TunnelResponse;
    use tokio::sync::oneshot::error::TryRecvError;

    use super::*;

    fn response(id: u64) -> TunnelResponse {
        TunnelResponse {
            id,
            status: 200,
            body: "{}".to_string(),
        }
    }

    #[test]
    fn a_response_reaches_the_caller_that_asked() {
        let mut correlator = Correlator::new();
        let (id, mut receiver) = correlator.issue().expect("issues");

        assert_eq!(correlator.resolve(response(id)), Resolved::Delivered);
        assert_eq!(receiver.try_recv().expect("arrived"), response(id));
        assert_eq!(correlator.in_flight(), 0);
    }

    #[test]
    fn a_response_for_an_abandoned_request_says_so() {
        // The portal disconnected mid-request. The desktop's answer is still
        // correct, there is simply nobody left to take it.
        let mut correlator = Correlator::new();
        let (id, receiver) = correlator.issue().expect("issues");
        drop(receiver);

        assert_eq!(correlator.resolve(response(id)), Resolved::Abandoned);
    }

    #[test]
    fn an_invented_id_is_reported_as_a_bug_not_a_race() {
        let mut correlator = Correlator::new();
        assert_eq!(correlator.resolve(response(9999)), Resolved::Unknown);
    }

    #[test]
    fn a_response_is_never_delivered_twice() {
        let mut correlator = Correlator::new();
        let (id, _receiver) = correlator.issue().expect("issues");

        assert_eq!(correlator.resolve(response(id)), Resolved::Delivered);
        assert_eq!(correlator.resolve(response(id)), Resolved::Unknown);
    }

    #[test]
    fn an_id_is_never_reused_even_after_the_request_is_abandoned() {
        // The bug this prevents: reuse id N for a new request while a late
        // response for the old N is still travelling. The late answer would be
        // delivered to the new caller, who would believe it.
        let mut correlator = Correlator::new();

        let (first, first_receiver) = correlator.issue().expect("issues");
        drop(first_receiver);
        correlator.resolve(response(first));

        let (second, _second_receiver) = correlator.issue().expect("issues");
        assert_ne!(first, second, "an abandoned id must not come round again");

        // And the stale response, arriving late, resolves nothing.
        assert_eq!(correlator.resolve(response(first)), Resolved::Unknown);
    }

    #[test]
    fn ids_are_never_zero() {
        // A zero id would be omitted by the proto3 JSON mapping, making a real
        // response indistinguishable from a malformed one.
        let mut correlator = Correlator::new();
        let (id, _receiver) = correlator.issue().expect("issues");
        assert_eq!(id, 1);
    }

    #[test]
    fn abandoning_a_request_frees_its_slot_and_releases_its_caller() {
        // A timeout must not be a slow leak: the abandoned entry has to stop
        // counting against the cap, or a desktop that is merely slow eventually
        // gets every new request refused — which reads as "everything suddenly
        // broke" rather than "that one request timed out".
        let mut correlator = Correlator::new();

        let mut ids = Vec::new();
        let mut receivers = Vec::new();
        for _ in 0..MAX_IN_FLIGHT {
            let (id, receiver) = correlator.issue().expect("within the cap");
            ids.push(id);
            receivers.push(receiver);
        }
        assert!(correlator.issue().is_err(), "the cap is reached");

        assert!(correlator.abandon(ids[0]), "there was a request to abandon");

        assert!(
            matches!(receivers[0].try_recv(), Err(TryRecvError::Closed)),
            "the caller must be released rather than left hanging"
        );

        // The freed slot can be used, and the abandoned id is never handed out
        // again — so a late answer for it cannot be delivered to this caller.
        let (next, _receiver) = correlator.issue().expect("the slot must be free again");
        assert!(next > ids[0], "an abandoned id must not come round again");

        // A late answer for the abandoned id resolves nothing.
        assert_eq!(correlator.resolve(response(ids[0])), Resolved::Unknown);
    }

    #[test]
    fn abandoning_something_that_was_never_issued_is_harmless() {
        let mut correlator = Correlator::new();
        assert!(!correlator.abandon(4242));
    }

    #[test]
    fn a_dead_tunnel_releases_every_caller() {
        // Without this, a portal request waits forever for an answer that can
        // never arrive.
        let mut correlator = Correlator::new();
        let (_first_id, mut first) = correlator.issue().expect("issues");
        let (_second_id, mut second) = correlator.issue().expect("issues");

        correlator.fail_all();

        assert!(matches!(first.try_recv(), Err(TryRecvError::Closed)));
        assert!(matches!(second.try_recv(), Err(TryRecvError::Closed)));
        assert_eq!(correlator.in_flight(), 0);
    }

    #[test]
    fn the_in_flight_cap_is_enforced_and_then_lifted() {
        let mut correlator = Correlator::new();

        let mut ids = Vec::new();
        // The receivers are kept alive deliberately. Dropping one is what makes
        // an answer `Abandoned`, and this test is about the cap, not about
        // abandonment — so the first version of it failed for the wrong reason.
        let mut _receivers = Vec::new();

        for _ in 0..MAX_IN_FLIGHT {
            let (id, receiver) = correlator.issue().expect("within the cap");
            ids.push(id);
            _receivers.push(receiver);
        }

        let refused = correlator.issue().expect_err("the cap must refuse");
        assert!(matches!(refused, TunnelError::TooManyInFlight { .. }));

        // Answering one frees a slot: the cap is pressure, not a latch.
        assert_eq!(correlator.resolve(response(ids[0])), Resolved::Delivered);
        assert!(correlator.issue().is_ok());
    }
}

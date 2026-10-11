//! The relay: live tunnels, and the requests that travel down them.
//!
//! The portal asks for something; it has to reach the *user's* desktop, not this
//! server. So the cloud keeps a handle per live tunnel and turns a portal
//! request into a frame, then waits for the frame that answers it.
//!
//! # Tested without sockets, on purpose
//!
//! A tunnel here is a channel, not a TCP connection. Everything worth testing —
//! correlation, a request to an offline desktop, a reconnect releasing the
//! requests still waiting on the old socket, a desktop that never answers — is
//! therefore provable with an in-memory `attach` and no networking at all. The
//! socket layer on top only has to read frames and hand them to [`Hub::resolve`].
//!
//! # Every wait has an end
//!
//! Three ways a request can stop waiting, and all three are deliberate:
//!
//! * the desktop answers,
//! * the tunnel is detached or replaced, which releases the waiter immediately,
//! * the timeout expires, which *also* removes the entry from the correlator so
//!   a slow desktop cannot slowly consume the in-flight budget.
//!
//! The failure this avoids is a portal request that waits forever because the
//! desktop is wedged, which presents to the user as a hung page and to us as
//! nothing at all.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flexdeck_protocol::{tunnel_frame, ServerMessage, TunnelFrame, TunnelRequest, TunnelResponse};
use flexdeck_tunnel::{Correlator, Resolved, TunnelError};
use tokio::sync::{broadcast, mpsc};

/// How many frames may be queued to one desktop before a sender waits.
///
/// Bounded so a portal that cannot be answered cannot make this server buffer
/// without limit on the desktop's behalf.
const TUNNEL_BUFFER: usize = 32;

/// How many pushes one desktop may have queued per portal subscriber.
const PUSH_BUFFER: usize = 64;

/// How long a relayed request waits for the desktop before giving up.
///
/// Generous, because the desktop is on someone else's machine and may be busy.
/// Finite, because "no answer" must eventually become an answer.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a request could not be relayed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayError {
    /// No tunnel is attached for that desktop.
    #[error("that desktop is not connected")]
    Offline,

    /// The tunnel closed while the request was in flight or being sent.
    #[error("the desktop's tunnel closed before the request completed")]
    Closed,

    /// The desktop is not keeping up with the requests being made of it.
    #[error("the desktop already has {limit} requests in flight")]
    TooManyInFlight { limit: usize },

    /// The desktop did not answer in time. The request may still arrive there.
    #[error("the desktop did not answer in time")]
    Timeout,
}

/// One live tunnel's outbound half plus its bookkeeping.
struct Tunnel {
    /// Frames to send down the socket.
    frames: mpsc::Sender<TunnelFrame>,

    /// Matches responses back to the callers waiting for them.
    correlator: Arc<Mutex<Correlator>>,

    /// Unsolicited frames from the desktop, to everyone watching this one.
    pushes: broadcast::Sender<ServerMessage>,

    /// The label the desktop gave itself in its hello. Held on the TUNNEL so it
    /// cannot outlive the connection that vouched for it.
    name: String,

    /// Which connection this is. A desktop that reconnects replaces its tunnel,
    /// and the OLD connection's clean-up must not then remove the new one.
    generation: u64,
}

/// The parts of a tunnel a caller needs, cloned out so no lock is held while
/// doing anything with them.
struct Parts {
    frames: mpsc::Sender<TunnelFrame>,
    correlator: Arc<Mutex<Correlator>>,
    pushes: broadcast::Sender<ServerMessage>,
}

/// Every live tunnel, and the way to reach them.
///
/// Tunnels are keyed by **fingerprint**: the identity a desktop proves, by
/// signature, before it is attached (see `tunnel_socket::admit`). A paired
/// browser remembers the fingerprint and addresses the desktop by it.
pub struct Hub {
    tunnels: Mutex<HashMap<String, Tunnel>>,
    timeout: Duration,
    generations: AtomicU64,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new(DEFAULT_TIMEOUT)
    }
}

impl Hub {
    /// Build a hub whose relayed requests give up after `timeout`.
    ///
    /// The timeout is a parameter so a test can prove the give-up path in
    /// milliseconds instead of seconds.
    pub fn new(timeout: Duration) -> Self {
        Self {
            tunnels: Mutex::new(HashMap::new()),
            timeout,
            generations: AtomicU64::new(0),
        }
    }

    /// Register a tunnel and hand back the receiver that drains its frames.
    ///
    /// Keyed by the desktop's fingerprint, which is the only identity it has
    /// proved at this point — it may not be linked to an account yet.
    ///
    /// A desktop that reconnects replaces its previous tunnel. Any request still
    /// waiting on the old one is released immediately rather than left to time
    /// out against a socket nobody is reading.
    pub fn attach(&self, fingerprint: &str) -> mpsc::Receiver<TunnelFrame> {
        self.attach_generation(fingerprint).1
    }

    /// [`Self::attach`], also returning which connection this is, so that its
    /// clean-up can use [`Self::detach_generation`].
    pub fn attach_generation(&self, fingerprint: &str) -> (u64, mpsc::Receiver<TunnelFrame>) {
        let (frames, receiver) = mpsc::channel(TUNNEL_BUFFER);
        let (pushes, _) = broadcast::channel(PUSH_BUFFER);
        let generation = self.generations.fetch_add(1, Ordering::Relaxed) + 1;

        let tunnel = Tunnel {
            frames,
            correlator: Arc::new(Mutex::new(Correlator::new())),
            pushes,
            name: String::new(),
            generation,
        };

        let mut tunnels = self.lock();
        if let Some(previous) = tunnels.insert(fingerprint.to_string(), tunnel) {
            Self::release_waiters(&previous);
        }

        (generation, receiver)
    }

    /// Forget a tunnel. Anything still waiting on it is released.
    pub fn detach(&self, fingerprint: &str) {
        let removed = self.lock().remove(fingerprint);
        if let Some(tunnel) = removed {
            Self::release_waiters(&tunnel);
        }
    }

    /// Forget a tunnel, but only if it is still the connection `generation`.
    ///
    /// What a connection's own clean-up must use. When a desktop reconnects, the
    /// old connection notices it has been replaced and ends — and removing by
    /// fingerprint alone would then take the NEW, healthy tunnel down with it.
    /// Returns whether anything was removed.
    pub fn detach_generation(&self, fingerprint: &str, generation: u64) -> bool {
        let removed = {
            let mut tunnels = self.lock();
            match tunnels.get(fingerprint) {
                Some(tunnel) if tunnel.generation == generation => tunnels.remove(fingerprint),
                _ => None,
            }
        };
        match removed {
            Some(tunnel) => {
                Self::release_waiters(&tunnel);
                true
            }
            None => false,
        }
    }

    /// Record the label a connected desktop gave itself in its hello.
    ///
    /// Separate from [`Self::attach`] so that a caller which does not care — every
    /// test that only needs a live tunnel — is not made to invent one. Does
    /// nothing when the desktop is not attached: the name belongs to the
    /// connection, and there is no connection to name.
    pub fn observe_name(&self, fingerprint: &str, name: &str) {
        // Sanitised: this value is shown to a person, and it is remote input.
        let Some(name) = clean_name(name) else {
            return;
        };

        if let Some(tunnel) = self.lock().get_mut(fingerprint) {
            tunnel.name = name;
        }
    }

    /// The label a connected desktop gave itself, or `None` when it is not
    /// connected.
    ///
    /// Only a live tunnel knows this, and that is enough: the one caller that
    /// needs it — the website completing a link on the desktop's behalf — has
    /// already refused the request when the tunnel is down.
    pub fn name_of(&self, fingerprint: &str) -> Option<String> {
        self.lock()
            .get(fingerprint)
            .map(|tunnel| tunnel.name.clone())
    }

    /// Send a frame down a tunnel that nobody is waiting on.
    ///
    /// For frames the desktop did not ask for, such as a keepalive. Waits if the
    /// tunnel is congested rather than dropping the frame.
    pub async fn send(&self, fingerprint: &str, frame: TunnelFrame) -> Result<(), RelayError> {
        let Some(parts) = self.parts(fingerprint) else {
            return Err(RelayError::Offline);
        };

        parts
            .frames
            .send(frame)
            .await
            .map_err(|_| RelayError::Closed)
    }

    /// Relay a request to a desktop and wait for its answer.
    ///
    /// `token` is the calling browser's own credential, passed through for the
    /// desktop to check. The cloud adds nothing of its own.
    pub async fn request(
        &self,
        fingerprint: &str,
        method: &str,
        path: &str,
        body: String,
        token: &str,
    ) -> Result<TunnelResponse, RelayError> {
        let Some(parts) = self.parts(fingerprint) else {
            return Err(RelayError::Offline);
        };

        let (id, receiver) = {
            let mut correlator = parts.correlator.lock().expect("not poisoned");
            correlator.issue().map_err(|error| match error {
                TunnelError::TooManyInFlight { limit } => RelayError::TooManyInFlight { limit },
                // Encoding is the only other thing `issue` can fail on, and it
                // cannot happen here; treat anything else as a closed tunnel
                // rather than inventing a third case.
                _ => RelayError::Closed,
            })?
        };

        let frame = TunnelFrame {
            kind: Some(tunnel_frame::Kind::Request(TunnelRequest {
                id,
                method: method.to_string(),
                path: path.to_string(),
                body,
                token: token.to_string(),
            })),
        };

        if parts.frames.send(frame).await.is_err() {
            return Err(RelayError::Closed);
        }

        match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(response)) => Ok(response),

            // The sender was dropped, so the tunnel went away mid-flight.
            Ok(Err(_)) => Err(RelayError::Closed),

            Err(_) => {
                // Free the slot. Without this a desktop that never answers
                // would keep consuming the in-flight budget until every further
                // request was refused, which is a leak that looks like a
                // different bug entirely.
                parts.correlator.lock().expect("not poisoned").abandon(id);
                Err(RelayError::Timeout)
            }
        }
    }

    /// Hand a desktop's answer to whoever is waiting for it.
    pub fn resolve(&self, fingerprint: &str, response: TunnelResponse) -> Resolved {
        let Some(parts) = self.parts(fingerprint) else {
            return Resolved::Unknown;
        };

        // Bound to a local rather than returned directly: as a tail expression
        // the guard would still be alive while `parts` is dropped.
        let resolved = parts
            .correlator
            .lock()
            .expect("not poisoned")
            .resolve(response);

        resolved
    }

    /// Broadcast an unsolicited frame from a desktop to everyone watching it.
    ///
    /// Having no listeners is not a failure: it just means nobody has the portal
    /// open for this desktop right now.
    pub fn push(&self, fingerprint: &str, message: ServerMessage) {
        if let Some(parts) = self.parts(fingerprint) {
            let _ = parts.pushes.send(message);
        }
    }

    /// Watch a desktop's unsolicited frames. `None` if it is not connected.
    pub fn subscribe(&self, fingerprint: &str) -> Option<broadcast::Receiver<ServerMessage>> {
        self.parts(fingerprint)
            .map(|parts| parts.pushes.subscribe())
    }

    /// Whether a desktop's tunnel is currently attached.
    pub fn is_online(&self, fingerprint: &str) -> bool {
        self.lock().contains_key(fingerprint)
    }

    /// How many tunnels are attached.
    pub fn online_count(&self) -> usize {
        self.lock().len()
    }

    fn parts(&self, fingerprint: &str) -> Option<Parts> {
        self.lock().get(fingerprint).map(|tunnel| Parts {
            frames: tunnel.frames.clone(),
            correlator: Arc::clone(&tunnel.correlator),
            pushes: tunnel.pushes.clone(),
        })
    }

    /// Drop every sender still waiting on a tunnel, so each awaiting receiver
    /// fails at once instead of hanging.
    fn release_waiters(tunnel: &Tunnel) {
        tunnel.correlator.lock().expect("not poisoned").fail_all();
    }

    /// The tunnel map is only ever locked for a lookup or an insert, never
    /// across an `await`, so a plain `Mutex` is enough and keeps every caller
    /// synchronous.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Tunnel>> {
        self.tunnels.lock().expect("the tunnel map is not poisoned")
    }
}

/// The longest label we will show for a desktop.
const MAX_NAME: usize = 64;

/// A label fit to show, or `None` when there is nothing usable in it.
///
/// The label arrives from the machine itself, so it is remote input that ends up
/// in a list a person reads. Control characters are dropped rather than
/// escaped: this is shown as one line, and a newline in it would be a lie about
/// where one entry ends and the next begins.
pub(crate) fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let cleaned = cleaned.trim();

    if cleaned.is_empty() {
        return None;
    }

    Some(cleaned.chars().take(MAX_NAME).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_protocol::{server_message, ErrorEvent, TunnelPing};

    #[test]
    fn an_old_connection_cannot_detach_the_one_that_replaced_it() {
        // The race this pins: a desktop reconnects, its old socket notices it was
        // replaced and ends — and its clean-up must not take the new tunnel too.
        let hub = Hub::new(Duration::from_millis(20));
        let (old, _old_frames) = hub.attach_generation(DESKTOP);
        let (new, _new_frames) = hub.attach_generation(DESKTOP);
        assert_ne!(old, new);

        assert!(
            !hub.detach_generation(DESKTOP, old),
            "the old one is gone already"
        );
        assert!(hub.is_online(DESKTOP), "and the new one is untouched");

        assert!(hub.detach_generation(DESKTOP, new));
        assert!(!hub.is_online(DESKTOP));
    }

    #[test]
    fn a_tunnels_name_is_known_only_while_it_is_connected() {
        let hub = Hub::new(Duration::from_millis(20));

        assert_eq!(hub.name_of(DESKTOP), None, "nothing is connected");

        let _frames = hub.attach(DESKTOP);
        assert_eq!(
            hub.name_of(DESKTOP).as_deref(),
            Some(""),
            "connected, but it has not said what it is called yet"
        );

        hub.observe_name(DESKTOP, "  Studio PC  ");
        assert_eq!(hub.name_of(DESKTOP).as_deref(), Some("Studio PC"));

        // Blank is not a name, and must not erase one.
        hub.observe_name(DESKTOP, "   ");
        assert_eq!(hub.name_of(DESKTOP).as_deref(), Some("Studio PC"));

        // A fresh connection starts over: the name belongs to the connection.
        let _again = hub.attach(DESKTOP);
        assert_eq!(hub.name_of(DESKTOP).as_deref(), Some(""));

        hub.detach(DESKTOP);
        assert_eq!(
            hub.name_of(DESKTOP),
            None,
            "and it goes away with the tunnel"
        );
    }

    const DESKTOP: &str = "desktop-1";

    fn hub() -> Arc<Hub> {
        Arc::new(Hub::default())
    }

    async fn next_request(frames: &mut mpsc::Receiver<TunnelFrame>) -> TunnelRequest {
        let frame = frames.recv().await.expect("a frame arrived");
        match frame.kind {
            Some(tunnel_frame::Kind::Request(request)) => request,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    fn answer(id: u64, body: &str) -> TunnelResponse {
        TunnelResponse {
            id,
            status: 200,
            body: body.to_string(),
        }
    }

    #[tokio::test]
    async fn a_request_reaches_the_desktop_and_its_answer_comes_back() {
        let hub = hub();
        let mut frames = hub.attach(DESKTOP);

        let asking = {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move {
                hub.request(DESKTOP, "GET", "/api/status", String::new(), "")
                    .await
            })
        };

        let request = next_request(&mut frames).await;
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/status");
        assert_eq!(request.body, "");
        assert_ne!(request.id, 0, "id zero is indistinguishable from absent");

        hub.resolve(DESKTOP, answer(request.id, "{\"rev\":1}"));

        let response = asking.await.expect("the task ran").expect("relayed");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "{\"rev\":1}");
    }

    #[tokio::test]
    async fn answers_are_matched_to_the_request_that_asked() {
        // The bug this prevents: two requests in flight, answers arriving out of
        // order, and one caller shown the other's configuration.
        let hub = hub();
        let mut frames = hub.attach(DESKTOP);

        let first = {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move {
                hub.request(DESKTOP, "GET", "/api/first", String::new(), "")
                    .await
            })
        };
        let first_request = next_request(&mut frames).await;

        let second = {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move {
                hub.request(DESKTOP, "GET", "/api/second", String::new(), "")
                    .await
            })
        };
        let second_request = next_request(&mut frames).await;

        assert_ne!(first_request.id, second_request.id);

        // Answer in reverse order, which is the case that breaks a design that
        // assumes one request at a time.
        hub.resolve(DESKTOP, answer(second_request.id, "second"));
        hub.resolve(DESKTOP, answer(first_request.id, "first"));

        assert_eq!(
            first.await.expect("ran").expect("relayed").body,
            "first",
            "the first caller must not be given the second answer"
        );
        assert_eq!(second.await.expect("ran").expect("relayed").body, "second");
    }

    #[tokio::test]
    async fn a_request_to_an_offline_desktop_is_refused() {
        let hub = hub();
        assert_eq!(
            hub.request("nobody", "GET", "/api/status", String::new(), "")
                .await,
            Err(RelayError::Offline)
        );
    }

    #[tokio::test]
    async fn detaching_releases_a_request_that_was_never_answered() {
        // Without this the portal waits for the full timeout against a socket
        // that no longer exists.
        let hub = hub();
        let mut frames = hub.attach(DESKTOP);

        let asking = {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move {
                hub.request(DESKTOP, "GET", "/api/status", String::new(), "")
                    .await
            })
        };

        let _request = next_request(&mut frames).await;
        hub.detach(DESKTOP);

        assert_eq!(asking.await.expect("ran"), Err(RelayError::Closed));
    }

    #[tokio::test]
    async fn a_reconnect_replaces_the_old_tunnel_and_frees_its_waiters() {
        // A desktop that drops and reconnects mid-request must not leave the
        // portal waiting on the dead socket.
        let hub = hub();
        let mut first = hub.attach(DESKTOP);

        let asking = {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move {
                hub.request(DESKTOP, "GET", "/api/status", String::new(), "")
                    .await
            })
        };

        let _request = next_request(&mut first).await;
        let _second = hub.attach(DESKTOP);

        assert_eq!(asking.await.expect("ran"), Err(RelayError::Closed));
        assert!(
            first.recv().await.is_none(),
            "the replaced tunnel's writer must be told the socket is over"
        );
        assert!(hub.is_online(DESKTOP), "the new tunnel is still attached");
        assert_eq!(hub.online_count(), 1, "and only one is attached");
    }

    #[tokio::test]
    async fn a_desktop_that_never_answers_times_out() {
        // Short timeout on purpose: the give-up path must be provable without
        // making the suite wait 30 seconds.
        let hub = Arc::new(Hub::new(Duration::from_millis(20)));
        let mut frames = hub.attach(DESKTOP);

        let asking = {
            let hub = Arc::clone(&hub);
            tokio::spawn(async move {
                hub.request(DESKTOP, "GET", "/api/status", String::new(), "")
                    .await
            })
        };

        let _request = next_request(&mut frames).await;

        assert_eq!(asking.await.expect("ran"), Err(RelayError::Timeout));
        // The tunnel is still fine: one slow request is not a dead desktop.
        assert!(hub.is_online(DESKTOP));
    }

    #[tokio::test]
    async fn an_answer_for_something_nobody_asked_is_reported_as_unknown() {
        let hub = hub();
        let _frames = hub.attach(DESKTOP);

        assert_eq!(
            hub.resolve(DESKTOP, answer(4242, "{}")),
            Resolved::Unknown,
            "an invented id is a bug on the desktop's side, not a race"
        );
        assert_eq!(
            hub.resolve("nobody", answer(1, "{}")),
            Resolved::Unknown,
            "and so is an answer from a tunnel we do not have"
        );
    }

    #[tokio::test]
    async fn a_push_reaches_everyone_watching_that_desktop() {
        // This is how the portal sees a config change without polling.
        let hub = hub();
        let _frames = hub.attach(DESKTOP);

        let mut first = hub.subscribe(DESKTOP).expect("subscribed");
        let mut second = hub.subscribe(DESKTOP).expect("subscribed");

        hub.push(
            DESKTOP,
            ServerMessage {
                kind: Some(server_message::Kind::Error(ErrorEvent {
                    code: "changed".to_string(),
                    message: "the config moved".to_string(),
                })),
            },
        );

        for subscriber in [&mut first, &mut second] {
            let received = subscriber.recv().await.expect("a push arrived");
            assert!(matches!(
                received.kind,
                Some(server_message::Kind::Error(_))
            ));
        }
    }

    #[tokio::test]
    async fn pushing_to_an_unwatched_or_offline_desktop_is_harmless() {
        // Nobody having the portal open is the normal case, not an error.
        let hub = hub();
        let _frames = hub.attach(DESKTOP);

        hub.push(
            DESKTOP,
            ServerMessage {
                kind: Some(server_message::Kind::Error(ErrorEvent {
                    code: "changed".to_string(),
                    message: "nobody is listening".to_string(),
                })),
            },
        );
        hub.push(DESKTOP, ServerMessage { kind: None });
        hub.push("nobody", ServerMessage { kind: None });

        assert!(hub.subscribe("nobody").is_none());
    }

    #[tokio::test]
    async fn an_unattached_desktop_is_not_online() {
        let hub = hub();
        assert!(!hub.is_online(DESKTOP));
        let _frames = hub.attach(DESKTOP);
        assert!(hub.is_online(DESKTOP));
        hub.detach(DESKTOP);
        assert!(!hub.is_online(DESKTOP));
        assert_eq!(hub.online_count(), 0);
    }

    #[tokio::test]
    async fn a_frame_can_be_pushed_down_a_tunnel_nobody_asked_on() {
        // How a link result reaches a desktop: it did not request it, the
        // ceremony finished somewhere else.
        let hub = hub();
        let mut frames = hub.attach(DESKTOP);

        hub.send(
            DESKTOP,
            TunnelFrame {
                kind: Some(tunnel_frame::Kind::Ping(TunnelPing {})),
            },
        )
        .await
        .expect("sent");

        let received = frames.recv().await.expect("a frame arrived");
        assert!(matches!(received.kind, Some(tunnel_frame::Kind::Ping(_))));
    }

    #[tokio::test]
    async fn sending_to_a_desktop_that_is_not_connected_is_refused() {
        let hub = hub();
        let outcome = hub
            .send(
                "nobody",
                TunnelFrame {
                    kind: Some(tunnel_frame::Kind::Ping(TunnelPing {})),
                },
            )
            .await;

        assert_eq!(outcome, Err(RelayError::Offline));
    }
}

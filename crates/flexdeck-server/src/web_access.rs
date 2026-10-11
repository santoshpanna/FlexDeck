//! The desktop's control over its tunnel to the portal.
//!
//! "Enable web access" is a decision the *user* makes, so it is persisted rather
//! than held in memory: an agent that came back from a restart with the tunnel
//! silently off would look like it had stopped working.
//!
//! There is no credential to keep: the cloud has no accounts, and the desktop
//! proves who it is on every connect by signing the cloud's challenge with its
//! own certificate key (see [`crate::tunnel`]).
//!
//! # Why a controller and not just a spawned task
//!
//! The toggle can be flipped more than once, and a second press must not start a
//! second tunnel. Owning the task handle is what makes "already running" and
//! "actually stopped" answerable rather than assumed.

//! The toggle is reachable over HTTP rather than only from Rust, because the
//! thing that flips it is a user interface that does not exist yet. A route is
//! the contract such an interface can be written against, and it is also what
//! makes the feature testable end to end today.

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::agent::{Agent, AgentError};
use crate::http::{require_configurator, ApiError};
use crate::settings::{LINK_TOKEN_KEY, WEB_ACCESS_KEY, WEB_ACCESS_OFF, WEB_ACCESS_ON};
use crate::tunnel::{run, TunnelConfig, TunnelEvent};

/// How many tunnel events a listener may fall behind before it is dropped.
const EVENTS_BUFFER: usize = 64;

/// What the tunnel is actually doing, as opposed to what the user asked for.
///
/// [`WebAccess::is_running`] used to mean "the reconnect loop's task is alive",
/// which stays true while *every* attempt is refused — so a desktop the portal
/// was refusing still read as connected. These states keep the
/// loop's liveness and the socket's state apart, which is the distinction the
/// Status card promises.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Connection {
    /// Web access is off, or has never been switched on.
    #[default]
    Idle,
    /// Trying, or waiting between attempts. Nothing is connected right now.
    Connecting,
    /// The portal accepted this desktop and the socket is up.
    Connected,
    /// The portal refused, and will keep refusing until something changes.
    Refused { code: String, message: String },
}

/// What the tunnel is doing, and why the last attempt failed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TunnelState {
    /// The socket's state.
    connection: Connection,
    /// The last transport failure in its own words, cleared once one connects.
    ///
    /// Kept apart from [`Connection::Refused`] because they answer different
    /// questions: a refusal is the PORTAL saying no, while this is the socket
    /// never getting there at all. Without it, "could not be reached" reads the
    /// same whether the portal is switched off or the address is simply wrong.
    last_error: Option<String>,
}

/// A refusal the portal stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WebAccessError {
    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error("this installation has no portal address set")]
    NoPortal,
}

/// Starts, stops and remembers the desktop's tunnel.
pub struct WebAccess {
    agent: Arc<Agent>,
    router: Router,
    /// The address and label, shared with the running loop so a corrected address
    /// is used by the very next reconnect instead of at the next restart.
    config: Arc<Mutex<TunnelConfig>>,
    task: Mutex<Option<JoinHandle<()>>>,
    events: broadcast::Sender<TunnelEvent>,
    /// The socket's real state, updated from the tunnel's own events. Shared with
    /// the drain task, which is its only writer.
    state: Arc<Mutex<TunnelState>>,
}

impl WebAccess {
    pub fn new(agent: Arc<Agent>, router: Router, config: TunnelConfig) -> Self {
        let (events, _) = broadcast::channel(EVENTS_BUFFER);

        // A credential from when desktops were linked to accounts. Nothing reads
        // it any more, and a dead secret is better gone than left lying around.
        if agent
            .setting(LINK_TOKEN_KEY)
            .ok()
            .flatten()
            .is_some_and(|t| !t.is_empty())
        {
            let _ = agent.set_setting(LINK_TOKEN_KEY, "");
        }

        Self {
            agent,
            router,
            config: Arc::new(Mutex::new(config)),
            task: Mutex::new(None),
            events,
            state: Arc::new(Mutex::new(TunnelState::default())),
        }
    }

    /// The portal this installation is pointed at, if any.
    pub fn portal(&self) -> Option<String> {
        self.config
            .lock()
            .expect("the tunnel config lock is not poisoned")
            .url
            .clone()
    }

    /// Point this installation at a different portal, or at none.
    ///
    /// The CALLER persists the choice. This is the running half, so that a
    /// corrected address takes effect now rather than at the next restart — the
    /// same split, and for the same reason, as `enable` for the on/off choice.
    ///
    /// Nothing here DIALS: a wrong address is not refused, it is reported through
    /// `last_error`. Refusing it would make the window the arbiter of what is
    /// reachable, which it cannot be.
    pub fn set_portal(&self, url: Option<String>) -> Result<(), WebAccessError> {
        let was_enabled = self.is_enabled()?;

        let url = url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        {
            let mut config = self
                .config
                .lock()
                .expect("the tunnel config lock is not poisoned");
            config.url = url;
        }

        // Anything already running is dialling the OLD address, so it cannot
        // simply carry on. Turning it off persists "off", which is why it is
        // turned back on only where there is still somewhere to connect to.
        self.disable();

        if was_enabled && self.portal().is_some() {
            self.enable()?;
        }

        Ok(())
    }

    /// Watch what the tunnel is doing, for whatever shows it to the user.
    pub fn events(&self) -> broadcast::Receiver<TunnelEvent> {
        self.events.subscribe()
    }

    /// Whether the user has turned web access on, as persisted.
    pub fn is_enabled(&self) -> Result<bool, WebAccessError> {
        Ok(self.agent.setting(WEB_ACCESS_KEY)?.as_deref() == Some(WEB_ACCESS_ON))
    }

    /// The tunnel's state right now, cloned out so the lock is never held while a
    /// caller formats or serialises it.
    fn state(&self) -> TunnelState {
        self.state
            .lock()
            .expect("the connection lock is not poisoned")
            .clone()
    }

    /// Whether a tunnel is **connected** right now.
    ///
    /// Distinct from [`Self::is_enabled`] on purpose: the preference is on the
    /// user's side, this is the socket's. A user who has enabled web access but
    /// whose portal is unreachable — or is refusing this desktop — is enabled and
    /// NOT running, and saying so is the honest answer rather than a comforting
    /// one. The reconnect loop being alive is deliberately not the question: it is
    /// alive while it is being refused, which is exactly the lie this avoids.
    pub fn is_running(&self) -> bool {
        matches!(self.state().connection, Connection::Connected)
    }

    /// Why the portal refused this desktop, if it did.
    ///
    /// A different problem from an unreachable portal: the network is fine and
    /// the portal said no — usually a version mismatch.
    pub fn refusal(&self) -> Option<Refusal> {
        match self.state().connection {
            Connection::Refused { code, message } => Some(Refusal { code, message }),
            _ => None,
        }
    }

    /// Why the last attempt failed, in the transport's own words.
    ///
    /// This is the difference between "the portal could not be reached" (which
    /// describes a switched-off portal, a wrong address and a wrong URL SCHEME
    /// identically) and a line that names the actual problem.
    pub fn last_error(&self) -> Option<String> {
        self.state().last_error
    }

    /// The same handle the drain task writes to, so a test can watch a real
    /// transition instead of asserting on a fabricated status.
    #[cfg(test)]
    fn state_handle(&self) -> Arc<Mutex<TunnelState>> {
        Arc::clone(&self.state)
    }

    /// Turn web access on and start the tunnel.
    ///
    /// Safe to call repeatedly: enabling something already enabled is a no-op
    /// rather than a second tunnel to the same portal.
    ///
    /// Must be called from within a Tokio runtime, because it spawns.
    pub fn enable(&self) -> Result<(), WebAccessError> {
        // Refused rather than started: a tunnel with no address would sit there
        // reporting a connection failure forever, which is a worse lie than
        // saying plainly that there is nowhere to connect to.
        if self.portal().is_none() {
            return Err(WebAccessError::NoPortal);
        }

        self.agent.set_setting(WEB_ACCESS_KEY, WEB_ACCESS_ON)?;

        let mut task = self.task.lock().expect("the task lock is not poisoned");
        if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return Ok(());
        }

        let (events, incoming) = mpsc::unbounded_channel();

        // A fresh attempt starts from "connecting", never from the previous
        // attempt's verdict: a stale refusal must not outlive the reconnect that
        // is going to resolve it.
        self.state
            .lock()
            .expect("the connection lock is not poisoned")
            .connection = Connection::Connecting;

        // The drain turns the loop's events into the state the window shows.
        tokio::spawn(drain(
            incoming,
            self.events.clone(),
            Arc::clone(&self.state),
        ));

        *task = Some(tokio::spawn(run(
            Arc::clone(&self.config),
            Arc::clone(&self.agent),
            self.router.clone(),
            events,
        )));

        Ok(())
    }

    /// Turn web access off and stop the tunnel.
    ///
    /// Aborting is deliberate and safe here: the loop holds nothing that has to be
    /// unwound, and a socket mid-handshake is precisely what should stop.
    pub fn disable(&self) {
        let _ = self.agent.set_setting(WEB_ACCESS_KEY, WEB_ACCESS_OFF);

        if let Some(handle) = self
            .task
            .lock()
            .expect("the task lock is not poisoned")
            .take()
        {
            handle.abort();
        }

        // Switching it off is a decision, not a failure: the tunnel is idle, not
        // merely disconnected.
        self.state
            .lock()
            .expect("the connection lock is not poisoned")
            .connection = Connection::Idle;
    }

    /// The running task's id, so a test can prove there is only one.
    #[cfg(test)]
    fn task_id(&self) -> Option<tokio::task::Id> {
        self.task
            .lock()
            .expect("the task lock is not poisoned")
            .as_ref()
            .map(|handle| handle.id())
    }
}

/// The routes that read and set web access.
///
/// Merged into the agent's own router by whoever builds it. The state is applied
/// here so the result is a `Router<()>`, which is what lets it merge with a
/// router whose state is the agent instead.
///
/// Configurator-only. These routes sit on the LAN listener, and the write opens
/// this desktop to the internet — not something anyone on the network should be
/// able to do. The desktop's window does not come through here at all; it holds
/// the handle and calls it directly.
pub fn router(access: Arc<WebAccess>) -> Router {
    let agent = Arc::clone(&access.agent);

    Router::new()
        .route("/api/web-access", get(read).post(write))
        .route_layer(middleware::from_fn_with_state(agent, require_configurator))
        .with_state(access)
}

/// What the desktop's own window needs to show the user.
///
/// `None` is an installation with no web-access state at all, and it answers with
/// the SAME SHAPE as a configured one: the window reads one description of a
/// tunnel, so it cannot be right for one installation and wrong for another.
///
/// This is the ONLY place that description is built — the agent's route and the
/// window's IPC command both call it, so a field added here appears in both.
pub fn view(access: Option<&WebAccess>) -> Result<Value, ApiError> {
    let Some(access) = access else {
        return Ok(json!({
            "enabled": false,
            "running": false,
            "refused": Value::Null,
            "lastError": Value::Null,
            "fingerprint": Value::Null,
            "portal": Value::Null,
        }));
    };

    let refused = access.refusal().map(|refusal| {
        json!({
            "code": refusal.code,
            "message": refusal.message,
        })
    });
    let last_error = access.last_error();

    Ok(json!({
        // What the user chose, and what a restart will do.
        "enabled": access
            .is_enabled()
            .map_err(|error| ApiError::internal(error.to_string()))?,
        // What the socket is doing. Different from the above: enabled and not
        // running is a real state — the portal is simply unreachable — and
        // reporting one as the other would be a comforting lie.
        "running": access.is_running(),
        // Why the portal refused, if it did. `null` means it did not — an
        // unanswered question and a stated refusal are different states, and the
        // window must not conflate them.
        "refused": refused,
        // Why the last attempt failed, when the failure was the socket rather than
        // the portal. `null` once anything connects.
        "lastError": last_error,
        // The identity the cloud routes by, and that a paired browser remembers.
        "fingerprint": access.agent.identity().fingerprint(),
        // Where it would connect. Read from the live config, not from a captured
        // copy, so a corrected address shows up as soon as it is set.
        "portal": access.portal(),
    }))
}

async fn read(State(access): State<Arc<WebAccess>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(view(Some(&access))?))
}

#[derive(Debug, Deserialize)]
struct Toggle {
    enabled: bool,
}

async fn write(
    State(access): State<Arc<WebAccess>>,
    Json(toggle): Json<Toggle>,
) -> Result<Json<Value>, ApiError> {
    if toggle.enabled {
        access
            .enable()
            .map_err(|error| ApiError::internal(error.to_string()))?;
    } else {
        access.disable();
    }

    Ok(Json(view(Some(&access))?))
}

/// Turn what the tunnel reports into the state the window shows, and pass it on
/// to whoever is watching.
async fn drain(
    mut incoming: mpsc::UnboundedReceiver<TunnelEvent>,
    relay: broadcast::Sender<TunnelEvent>,
    state: Arc<Mutex<TunnelState>>,
) {
    while let Some(event) = incoming.recv().await {
        {
            let mut tunnel = state.lock().expect("the connection lock is not poisoned");

            match &event {
                TunnelEvent::Accepted => {
                    tunnel.connection = Connection::Connected;
                    // The last failure is history once something gets through.
                    // Keeping it would show a stale reason beside a working tunnel.
                    tunnel.last_error = None;
                }

                TunnelEvent::Rejected { code, message } => {
                    tunnel.connection = Connection::Refused {
                        code: code.clone(),
                        message: message.clone(),
                    };
                }

                TunnelEvent::Disconnected { reason } => {
                    // A refusal is immediately followed by a Disconnected, so only
                    // a LIVE connection is downgraded — otherwise the loop's own
                    // verdict would be wiped by the drop that reports it.
                    if matches!(tunnel.connection, Connection::Connected) {
                        tunnel.connection = Connection::Connecting;
                    }

                    // Whatever ended this attempt, it is the most recent fact
                    // about the tunnel and the only explanation the user can get.
                    tunnel.last_error = Some(reason.clone());
                }
            }
        }

        // Nobody listening is the normal case, not an error.
        let _ = relay.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use flexdeck_pairing::Identity;
    use flexdeck_store::ConfigStore;
    use tower::ServiceExt;

    /// A URL nothing is listening on: the loop fails to connect and backs off,
    /// which is fine — these tests are about the toggle, not about connecting.
    const NOWHERE: &str = "ws://127.0.0.1:1/tunnel";

    fn agent() -> Arc<Agent> {
        let store = ConfigStore::open_in_memory().expect("store opens");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        Arc::new(Agent::new(
            store,
            identity,
            "192.168.1.10".to_string(),
            7878,
        ))
    }

    fn access(agent: &Arc<Agent>) -> WebAccess {
        WebAccess::new(
            Arc::clone(agent),
            http::router(Arc::clone(agent)),
            TunnelConfig {
                url: Some(NOWHERE.to_string()),
                name: "Studio PC".to_string(),
            },
        )
    }

    /// An installation that has never been told where its portal is.
    fn no_portal(agent: &Arc<Agent>) -> WebAccess {
        WebAccess::new(
            Arc::clone(agent),
            http::router(Arc::clone(agent)),
            TunnelConfig {
                url: None,
                name: "Studio PC".to_string(),
            },
        )
    }

    #[tokio::test]
    async fn enabling_is_remembered_and_can_be_turned_off_again() {
        let agent = agent();
        let web = access(&agent);

        assert!(!web.is_enabled().expect("readable"));
        assert!(!web.is_running());

        web.enable().expect("enables");

        assert!(
            web.is_enabled().expect("readable"),
            "the choice must survive a restart, or the tunnel looks broken after one"
        );
        assert!(
            web.task_id().is_some(),
            "switching it on starts a tunnel attempt"
        );
        assert!(
            !web.is_running(),
            "nothing is listening at {NOWHERE}, so nothing is connected — reporting \
             this as running was the comforting lie"
        );

        web.disable();

        assert!(!web.is_enabled().expect("readable"));
        assert!(!web.is_running());
    }

    #[tokio::test]
    async fn enabling_twice_does_not_start_a_second_tunnel() {
        let agent = agent();
        let web = access(&agent);

        web.enable().expect("enables");
        let first = web.task_id();

        web.enable().expect("enables again");
        let second = web.task_id();

        assert_eq!(first, second, "one tunnel, not two");
        assert!(first.is_some(), "and there is one");
    }

    #[tokio::test]
    async fn disabling_before_enabling_is_harmless() {
        let agent = agent();
        let web = access(&agent);

        web.disable();

        assert!(!web.is_enabled().expect("readable"));
        assert!(!web.is_running());
    }

    #[tokio::test]
    async fn a_credential_from_the_account_era_is_cleared() {
        // Desktops used to be linked to accounts with a stored token. Nothing
        // reads it any more, and a dead secret is better gone.
        let agent = agent();
        agent
            .set_setting(LINK_TOKEN_KEY, "an-old-link-token")
            .expect("stored");

        let _web = access(&agent);

        assert_eq!(
            agent.setting(LINK_TOKEN_KEY).expect("readable"),
            Some(String::new())
        );
    }

    #[tokio::test]
    async fn the_window_is_connected_only_while_the_socket_is_up() {
        // The bug this pins: `is_running` used to mean "the reconnect task is
        // alive", which stays true while every attempt is refused — so a desktop
        // the portal was refusing still read as connected.
        let agent = agent();
        let web = access(&agent);
        let (events, incoming) = mpsc::unbounded_channel();
        let (relay, mut watching) = broadcast::channel(EVENTS_BUFFER);

        let draining = tokio::spawn(drain(incoming, relay, web.state_handle()));

        assert!(!web.is_running(), "nothing has been attempted yet");

        // A failure FIRST, so the later assertion that getting through clears it
        // is not vacuous: the state starts empty, and asserting "still empty"
        // would pass even with the clearing deleted.
        events
            .send(TunnelEvent::Disconnected {
                reason: "the portal closed the tunnel".to_string(),
            })
            .expect("sent");
        watching.recv().await.expect("relayed");
        assert!(web.last_error().is_some(), "the failure was recorded");

        events.send(TunnelEvent::Accepted).expect("sent");
        watching.recv().await.expect("relayed");

        assert!(web.is_running(), "accepted means connected");
        assert_eq!(web.refusal(), None);
        assert_eq!(
            web.last_error(),
            None,
            "getting through clears the previous failure"
        );

        let reason = "could not connect to wss://p.example/tunnel: invalid TLS handshake";
        events
            .send(TunnelEvent::Disconnected {
                reason: reason.to_string(),
            })
            .expect("sent");
        watching.recv().await.expect("relayed");

        assert!(!web.is_running(), "a drop is not a live connection");
        assert_eq!(
            web.last_error().as_deref(),
            Some(reason),
            "the transport's own words are kept: 'not connected' alone cannot tell a \
             wrong URL scheme from a portal that is switched off"
        );

        drop(events);
        let _ = draining.await;
    }

    #[tokio::test]
    async fn a_refusal_survives_the_drop_that_reports_it() {
        // A refusal is a different problem from an unreachable portal: the
        // network is fine and the portal said no.
        let agent = agent();
        let web = access(&agent);
        let (events, incoming) = mpsc::unbounded_channel();
        let (relay, mut watching) = broadcast::channel(EVENTS_BUFFER);
        let draining = tokio::spawn(drain(incoming, relay, web.state_handle()));

        events
            .send(TunnelEvent::Rejected {
                code: "protocol_version".to_string(),
                message: "this server speaks tunnel protocol 3, the desktop sent 2".to_string(),
            })
            .expect("sent");
        watching.recv().await.expect("relayed");

        // `run` ALWAYS follows a refusal with a Disconnected, so this is the order
        // production actually produces. Without the guard in `drain` the refusal
        // would be wiped by the drop that reports it and the window would fall
        // back to "could not be reached".
        events
            .send(TunnelEvent::Disconnected {
                reason: "protocol_version: this server speaks tunnel protocol 3".to_string(),
            })
            .expect("sent");
        watching.recv().await.expect("relayed");

        assert!(!web.is_running());
        let refusal = web
            .refusal()
            .expect("the refusal survives the drop that reports it");
        assert_eq!(refusal.code, "protocol_version");

        drop(events);
        let _ = draining.await;
    }

    #[tokio::test]
    async fn enabling_without_a_portal_address_is_refused_with_a_reason() {
        // Refused rather than started: a tunnel with nowhere to dial reports a
        // connection failure forever, which is a worse answer than saying plainly
        // that there is nowhere to connect to.
        let agent = agent();
        let web = no_portal(&agent);

        assert!(matches!(web.enable(), Err(WebAccessError::NoPortal)));
        assert!(
            !web.is_enabled().expect("readable"),
            "a refused enable must not persist \"on\""
        );
        assert!(!web.is_running());
    }

    #[tokio::test]
    async fn setting_the_portal_address_starts_and_stops_accordingly() {
        // The point of the window's address field: a wrong address becomes
        // correctable without a restart and without a command line.
        let agent = agent();
        let web = no_portal(&agent);

        web.set_portal(Some(NOWHERE.to_string())).expect("sets");
        assert_eq!(web.portal().as_deref(), Some(NOWHERE));

        web.enable().expect("enables");
        assert!(web.is_enabled().expect("readable"));
        let first = web.task_id();

        // Changing it while it is on keeps it on, dialling the new address.
        web.set_portal(Some("ws://127.0.0.1:2/tunnel".to_string()))
            .expect("retargets");

        assert!(web.is_enabled().expect("readable"), "still switched on");
        assert_eq!(web.portal().as_deref(), Some("ws://127.0.0.1:2/tunnel"));
        assert!(
            web.task_id().is_some_and(|id| Some(id) != first),
            "the tunnel was RESTARTED, because the running one was dialling the \
             address that was just replaced"
        );
        assert!(
            !web.is_running(),
            "and `running` still means CONNECTED: nothing is listening there, so \
             claiming otherwise would be the comforting lie"
        );

        // Clearing it turns web access OFF: there is nothing left to connect to,
        // and leaving it "on" would be a state that cannot be true.
        web.set_portal(None).expect("clears");
        assert_eq!(web.portal(), None);
        assert!(!web.is_enabled().expect("readable"));
        assert!(web.task_id().is_none(), "and the tunnel was stopped");
    }

    #[tokio::test]
    async fn a_blank_address_reads_as_no_address_at_all() {
        let agent = agent();
        let web = access(&agent);

        web.set_portal(Some("   ".to_string())).expect("clears");

        assert_eq!(web.portal(), None, "whitespace is not an address");
    }

    // -- over the HTTP routes ------------------------------------------------

    /// Send with the admin token, which is what every route here requires.
    async fn call(access: &Arc<WebAccess>, mut request: Request<Body>) -> (StatusCode, Value) {
        let token = access.agent.admin_token().expect("an admin token");
        request.headers_mut().insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("a header value"),
        );

        call_as(access, request).await
    }

    async fn call_as(access: &Arc<WebAccess>, request: Request<Body>) -> (StatusCode, Value) {
        let response = router(Arc::clone(access))
            .oneshot(request)
            .await
            .expect("the router answered");

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("a body");

        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn read(access: &Arc<WebAccess>) -> (StatusCode, Value) {
        let request = Request::builder()
            .uri("/api/web-access")
            .body(Body::empty())
            .expect("a request");

        call(access, request).await
    }

    async fn set(access: &Arc<WebAccess>, enabled: bool) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri("/api/web-access")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "enabled": enabled }).to_string()))
            .expect("a request");

        call(access, request).await
    }

    #[tokio::test]
    async fn the_routes_refuse_anyone_without_the_admin_token() {
        // The read carries the live link code and the write starts the tunnel.
        // Open on the LAN, that is enough for a stranger to link this desktop to
        // their own account.
        let agent = agent();
        let web = Arc::new(access(&agent));

        let anonymous = Request::builder()
            .uri("/api/web-access")
            .body(Body::empty())
            .expect("a request");
        let (status, body) = call_as(&web, anonymous).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            body.get("enabled").is_none(),
            "a refusal leaks nothing: {body}"
        );

        let switch_on = Request::builder()
            .method("POST")
            .uri("/api/web-access")
            .header("content-type", "application/json")
            .header("authorization", "Bearer not-the-admin-token")
            .body(Body::from(json!({ "enabled": true }).to_string()))
            .expect("a request");
        let (status, _) = call_as(&web, switch_on).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            !web.is_enabled().expect("readable"),
            "a refused write changed nothing"
        );
    }

    #[tokio::test]
    async fn the_status_route_reports_what_was_chosen_and_what_is_running() {
        let agent = agent();
        let web = Arc::new(access(&agent));

        let (status, body) = read(&web).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], false);
        assert_eq!(body["running"], false);
        assert_eq!(
            body["refused"],
            Value::Null,
            "no refusal is `null`, not an absent key: the window tests it directly"
        );

        // The fingerprint is what the user reads out to the website, so it has to
        // be here — the website has no other way to name a desktop to link.
        assert_eq!(
            body["fingerprint"],
            Value::String(agent.identity().fingerprint())
        );
    }

    #[tokio::test]
    async fn the_toggle_route_turns_the_tunnel_on_and_off() {
        let agent = agent();
        let web = Arc::new(access(&agent));

        let (status, body) = set(&web, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["enabled"], true,
            "the reply must reflect the new state"
        );
        assert_eq!(
            body["running"], false,
            "nothing is listening at {NOWHERE}, and the status must not claim \
             otherwise just because the reconnect loop is alive"
        );

        let (_, body) = set(&web, false).await;
        assert_eq!(body["enabled"], false);
        assert_eq!(body["running"], false);

        // And the choice stuck, rather than only being echoed back.
        let (_, body) = read(&web).await;
        assert_eq!(body["enabled"], false);
    }
}

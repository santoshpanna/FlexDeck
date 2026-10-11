//! The desktop's outbound tunnel to the cloud portal.
//!
//! # The desktop decides, always
//!
//! A forwarded request is re-authorised by the agent, using the agent's own
//! router and its own middleware. Nothing in this module grants authority: it
//! takes the request the cloud sent, marks it as having come through the relay,
//! attaches the **calling browser's own credential** (and nothing else), and
//! lets the same code that serves the LAN decide the outcome.
//!
//! That is why [`forward`] exists as a separate, tested function rather than
//! being inlined into the socket loop. It is the single place where a remote
//! party's input turns into a local API call, so it is the place worth pinning
//! down — and a bug that lets the cloud reach a route the portal could not is
//! exactly the kind of thing a socket-level test would not notice.
//!
//! # Why the cloud holds no authority of its own
//!
//! There are no accounts and no admin credential on this path. A browser that
//! paired with a code shown on this desktop holds a token this desktop issued;
//! the cloud passes it through in `TunnelRequest.token`, and the request is
//! marked [`Channel::Relay`], where only such a token is honoured (see
//! [`crate::access`]). The agent's admin token never touches the tunnel, so a
//! compromised portal can do no more than replay what a paired browser sent it.
//!
//! # Proving who we are
//!
//! The cloud routes by fingerprint, and a fingerprint is not a secret. So the
//! cloud opens with a random challenge, and [`hello`] answers it with a
//! signature by the certificate's private key. Without the key, claiming
//! another desktop's fingerprint gets a refusal, not its traffic.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use flexdeck_protocol::{
    tunnel_frame, PairOffer, TunnelFrame, TunnelHello, TunnelPush, TunnelRequest, TunnelResponse,
};
use flexdeck_tunnel::{decode, encode, TunnelError, PROTOCOL_VERSION};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

use crate::access::Channel;
use crate::agent::{now_secs, Agent, AgentError, BrowserOffer};

/// The socket half of the tunnel.
type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type ClientSink = SplitSink<ClientSocket, Message>;
type ClientStream = SplitStream<ClientSocket>;

/// The longest we will wait between reconnection attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// How often to send a keepalive.
///
/// An idle tunnel is dropped by routers in minutes, and a tunnel that looks open
/// but is not is worse than one that is known to be gone.
const KEEPALIVE: Duration = Duration::from_secs(30);

/// The largest forwarded body we will accept from the cloud.
///
/// A configuration document is a few kilobytes. The cap is here so that a
/// hostile or buggy portal cannot make the desktop allocate without bound, and
/// so the failure is a stated refusal rather than an out-of-memory abort.
pub const MAX_BODY: usize = 1024 * 1024;

/// The route prefix the tunnel may reach.
///
/// Deliberately not "any path". The agent also serves a WebSocket upgrade at
/// `/ws`, which cannot be carried by a request/response frame — forwarding it
/// would produce a response with an upgrade header and no way to continue the
/// conversation. Restricting the tunnel to the JSON API makes that structurally
/// impossible instead of a case someone has to remember.
const API_PREFIX: &str = "/api/";

/// Why a forwarded request could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    /// The request carried no id, so a response could not be correlated with it.
    ///
    /// This is always a bug on the cloud side rather than something a client
    /// did. Note that id `0` is indistinguishable from "absent" on the wire,
    /// because the proto3 JSON mapping omits default values — so ids start at 1
    /// and 0 is refused here rather than silently answered as if it were real.
    #[error("a forwarded request arrived without an id, so it cannot be answered")]
    Unidentified,
}

/// Everything a tunnel needs to know to start.
///
/// The address is read at each attempt rather than captured, because it can be
/// corrected underneath a running loop. See [`session`].
#[derive(Debug, Clone)]
pub struct TunnelConfig {
    /// The cloud's tunnel endpoint, e.g. `wss://portal.example.com/tunnel`, or
    /// `None` when this installation has no portal configured at all.
    pub url: Option<String>,

    /// Human label shown on the website. Cosmetic; never used for authorisation.
    pub name: String,
}

/// What happened on the tunnel, for the desktop's own window to show.
#[derive(Debug, Clone)]
pub enum TunnelEvent {
    /// The cloud accepted the connection and its proof of identity.
    Accepted,

    /// The cloud refused, and will keep refusing until something changes.
    Rejected { code: String, message: String },

    /// The connection dropped and will be retried.
    Disconnected { reason: String },
}

/// Answer the cloud's challenge: who we are, and proof that we hold the key.
///
/// The signature covers the nonce *and* the fingerprint (see
/// [`flexdeck_pairing::tunnel_proof_message`]), so it answers this challenge,
/// for this identity, and nothing else.
pub fn hello(agent: &Agent, name: &str, nonce: &str) -> Result<TunnelFrame, SessionError> {
    let identity = agent.identity();
    let fingerprint = identity.fingerprint();
    let signature = identity
        .sign(&flexdeck_pairing::tunnel_proof_message(nonce, &fingerprint))
        .map_err(|error| SessionError::Identity(error.to_string()))?;

    Ok(TunnelFrame {
        kind: Some(tunnel_frame::Kind::Hello(TunnelHello {
            protocol_version: PROTOCOL_VERSION,
            fingerprint,
            name: name.to_string(),
            // The revision the portal should measure deltas from. Without it the
            // portal would have to assume this desktop is current, which is the
            // one thing it cannot know.
            rev: agent.rev().ok().flatten().unwrap_or_default(),
            certificate: identity.cert_der().to_vec(),
            signature,
        })),
    })
}

/// The frame that tells the cloud which browser code to route here, if any.
/// An expired offer is sent as a withdrawal.
pub fn pair_offer(offer: Option<&BrowserOffer>, now: i64) -> TunnelFrame {
    let live = offer.filter(|offer| offer.expires_at > now);
    TunnelFrame {
        kind: Some(tunnel_frame::Kind::PairOffer(PairOffer {
            code_hash: live.map(|o| o.code_hash.clone()).unwrap_or_default(),
            expires_at: live.map(|o| o.expires_at).unwrap_or_default(),
        })),
    }
}

/// Run one forwarded request through the agent's own router.
///
/// Returns the response to send back, or [`ForwardError::Unidentified`] when
/// there is no id to answer against.
///
/// A refusal for a bad path or method comes back as `Ok` with a status, not as
/// an error, because the cloud can act on a `404` — it cannot act on a dropped
/// connection.
pub async fn forward(
    router: &Router,
    request: &TunnelRequest,
) -> Result<TunnelResponse, ForwardError> {
    if request.id == 0 {
        return Err(ForwardError::Unidentified);
    }

    let id = request.id;
    let refuse = |status: StatusCode, message: &str| {
        Ok(TunnelResponse {
            id,
            status: status.as_u16() as u32,
            body: error_body(message),
        })
    };

    if !request.path.starts_with(API_PREFIX) {
        return refuse(
            StatusCode::NOT_FOUND,
            "the tunnel only carries requests under /api/",
        );
    }

    let Ok(method) = Method::from_bytes(request.method.as_bytes()) else {
        return refuse(StatusCode::BAD_REQUEST, "unknown HTTP method");
    };

    if request.body.len() > MAX_BODY {
        return refuse(StatusCode::PAYLOAD_TOO_LARGE, "request body is too large");
    }

    // Marked as relayed with a typed extension, which nothing on the wire can
    // set or forge. It is what makes the middleware honour only a remote
    // browser's credential here — never the admin token, never a tablet's.
    let mut builder = Request::builder()
        .method(method)
        .uri(&request.path)
        .extension(Channel::Relay);

    // The caller's own credential, passed through untouched. The cloud adds
    // nothing of its own: with no token, only the code-redeem route answers.
    if !request.token.is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {}", request.token));
    }

    let body = if request.body.is_empty() {
        Body::empty()
    } else {
        // `Json<T>` extractors reject a request without this header, so omitting
        // it would turn every write into a 415 that looks like a contract bug.
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(request.body.clone())
    };

    let Ok(forwarded) = builder.body(body) else {
        return refuse(StatusCode::BAD_REQUEST, "malformed request");
    };

    let response = match router.clone().oneshot(forwarded).await {
        Ok(response) => response,
        Err(never) => match never {},
    };

    let status = response.status().as_u16() as u32;
    let body = match axum::body::to_bytes(response.into_body(), MAX_BODY).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => {
            return Ok(TunnelResponse {
                id,
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16() as u32,
                body: error_body(&format!("the response could not be read: {error}")),
            });
        }
    };

    Ok(TunnelResponse { id, status, body })
}

/// A JSON error body, matching the shape the REST API already uses.
fn error_body(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

// -- the socket --------------------------------------------------------------

/// Why one connection attempt ended.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("could not reach the portal at {url}: {source}")]
    Connect {
        url: String,
        #[source]
        source: Box<tokio_tungstenite::tungstenite::Error>,
    },

    #[error("the tunnel stopped carrying frames: {0}")]
    Transport(#[source] Box<tokio_tungstenite::tungstenite::Error>),

    #[error("a tunnel frame did not survive the wire: {0}")]
    Frame(#[from] TunnelError),

    #[error("the portal refused this desktop ({code}): {message}")]
    Refused { code: String, message: String },

    #[error(transparent)]
    Store(#[from] AgentError),

    #[error("could not prove this desktop's identity: {0}")]
    Identity(String),

    #[error("the portal closed the tunnel")]
    Closed,

    #[error("this installation has no portal address set")]
    NoPortal,
}

/// Open one tunnel: greet the portal, then carry frames until either end stops.
///
/// Returns when the connection is over, for any reason. Reconnecting is
/// [`run`]'s job, because a single attempt cannot know whether retrying is
/// sensible.
pub async fn session(
    config: &Mutex<TunnelConfig>,
    agent: Arc<Agent>,
    router: Router,
    events: &UnboundedSender<TunnelEvent>,
) -> Result<(), SessionError> {
    // Read under the lock, then let it go: nothing below this line may hold it,
    // and the address is deliberately re-read on every attempt so that a
    // corrected one is used by the very next reconnect.
    let (url, name) = {
        let config = config
            .lock()
            .expect("the tunnel config lock is not poisoned");
        (config.url.clone(), config.name.clone())
    };

    let Some(url) = url else {
        return Err(SessionError::NoPortal);
    };

    let (socket, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|source| SessionError::Connect {
            url: url.clone(),
            source: Box::new(source),
        })?;

    let (mut sink, mut stream) = socket.split();

    // The cloud speaks first, with a challenge to sign. Anything else first is
    // either a refusal or a portal that does not speak this protocol.
    let nonce = match next_frame(&mut stream).await?.kind {
        Some(tunnel_frame::Kind::Challenge(challenge)) if !challenge.nonce.is_empty() => {
            challenge.nonce
        }
        Some(tunnel_frame::Kind::Rejected(rejected)) => {
            return Err(refused(events, rejected.code, rejected.message));
        }
        other => {
            return Err(SessionError::Refused {
                code: "unexpected".to_string(),
                message: format!("expected a challenge first, got {other:?}"),
            });
        }
    };

    send(&mut sink, &hello(&agent, &name, &nonce)?).await?;

    match next_frame(&mut stream).await?.kind {
        Some(tunnel_frame::Kind::Welcome(_)) => {
            let _ = events.send(TunnelEvent::Accepted);
        }

        Some(tunnel_frame::Kind::Rejected(rejected)) => {
            return Err(refused(events, rejected.code, rejected.message));
        }

        other => {
            return Err(SessionError::Refused {
                code: "unexpected".to_string(),
                message: format!("expected a welcome, got {other:?}"),
            });
        }
    }

    pump(sink, stream, agent, router).await
}

/// Report a refusal to the window, and turn it into the session's outcome.
fn refused(events: &UnboundedSender<TunnelEvent>, code: String, message: String) -> SessionError {
    let _ = events.send(TunnelEvent::Rejected {
        code: code.clone(),
        message: message.clone(),
    });
    SessionError::Refused { code, message }
}

/// Spread a wait over ±25%, so a fleet of desktops that all lost the cloud at
/// once — a deploy, say — does not reconnect in one synchronised wave.
fn jittered(wait: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or_default();
    wait * (75 + nanos % 51) / 100
}

/// Keep a tunnel open, reconnecting for as long as the process lives.
///
/// Never gives up: a desktop that was asleep, or a portal that is mid-deploy,
/// must not require the user to restart anything.
pub async fn run(
    config: Arc<Mutex<TunnelConfig>>,
    agent: Arc<Agent>,
    router: Router,
    events: UnboundedSender<TunnelEvent>,
) {
    let mut backoff = Duration::from_secs(1);

    loop {
        // Somewhere to go, still? The address can be cleared while the loop is
        // waiting between attempts, and a task with no portal should END rather
        // than sit here dialling nothing for the life of the process.
        let has_portal = config
            .lock()
            .expect("the tunnel config lock is not poisoned")
            .url
            .is_some();

        if !has_portal {
            return;
        }

        let outcome = session(&config, Arc::clone(&agent), router.clone(), &events).await;

        let wait = match outcome {
            Ok(()) => {
                let _ = events.send(TunnelEvent::Disconnected {
                    reason: "the portal closed the tunnel".to_string(),
                });

                // A clean close is usually a deploy or a restart, so try again
                // promptly rather than serving out a long backoff.
                backoff = Duration::from_secs(1);
                Duration::from_secs(1)
            }

            Err(SessionError::Refused { code, message }) => {
                let _ = events.send(TunnelEvent::Disconnected {
                    reason: format!("{code}: {message}"),
                });

                // Nothing about this fixes itself by retrying quickly — the
                // protocol version is wrong, or the proof was refused. Wait properly.
                MAX_BACKOFF
            }

            Err(error) => {
                let _ = events.send(TunnelEvent::Disconnected {
                    reason: error.to_string(),
                });

                let wait = backoff;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                wait
            }
        };

        tokio::time::sleep(jittered(wait)).await;
    }
}

/// Carry frames both ways until either end stops.
async fn pump(
    sink: ClientSink,
    mut stream: ClientStream,
    agent: Arc<Agent>,
    router: Router,
) -> Result<(), SessionError> {
    // Frames go out through a task of their own rather than straight from this
    // loop. Answering a forwarded request means awaiting the agent's own router,
    // and doing that here would stall every push behind it — including the one
    // that tells the portal the change it is waiting for just happened.
    let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<TunnelFrame>();

    let writer = tokio::spawn(async move {
        let mut sink = sink;

        while let Some(frame) = outgoing_rx.recv().await {
            let Ok(text) = encode(&frame) else { continue };

            if sink.send(Message::text(text)).await.is_err() {
                break;
            }
        }
    });

    let mut changes = agent.subscribe();

    // The browser pairing code the cloud should route here. Offered once now —
    // a code opened while the tunnel was down still needs routing — and again
    // whenever it changes.
    let mut offers = agent.watch_browser_offer();
    let first = pair_offer(offers.borrow_and_update().as_ref(), now_secs());
    let _ = outgoing.send(first);
    let mut offers_open = true;
    // The revision the portal has been told about. `catch_up` measures from
    // here, so a portal that missed several changes still gets one correct
    // answer instead of a patch it cannot apply.
    let mut told = agent.rev().ok().flatten();

    let mut keepalive = tokio::time::interval(KEEPALIVE);

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(message) = incoming else { break };
                let Ok(message) = message else { break };
                let Ok(text) = message.to_text() else { continue };

                let Ok(frame) = decode(text) else {
                    // Both ends are our own software, so an unreadable frame is a
                    // version mismatch or a bug. Continuing would turn it into a
                    // request that never completes.
                    break;
                };

                match frame.kind {
                    Some(tunnel_frame::Kind::Request(request)) => {
                        let router = router.clone();
                        let outgoing = outgoing.clone();

                        // Off the loop, so a slow request cannot delay pushes.
                        tokio::spawn(async move {
                            // An uncorrelatable request gets no answer: there is
                            // no id to answer against, and inventing one would be
                            // worse than silence.
                            if let Ok(response) = forward(&router, &request).await {
                                let _ = outgoing.send(TunnelFrame {
                                    kind: Some(tunnel_frame::Kind::Response(response)),
                                });
                            }
                        });
                    }

                    Some(tunnel_frame::Kind::Ping(_)) => {
                        let _ = outgoing.send(TunnelFrame {
                            kind: Some(tunnel_frame::Kind::Pong(Default::default())),
                        });
                    }

                    // A greeting has already happened, and relay frames travel the
                    // other way. Anything else here is a message this build does
                    // not know or a bug; ignoring it beats ending a healthy tunnel
                    // over it, which is the same tolerance the LAN socket shows.
                    _ => {}
                }
            }

            changed = changes.recv() => {
                match changed {
                    Ok(_rev) => {
                        if let Some(frame) = push_since(&agent, &mut told) {
                            let _ = outgoing.send(frame);
                        }
                    }

                    // We fell behind the broadcast. Resynchronise from wherever
                    // the portal was last told, rather than skipping what we
                    // missed — the whole point is that it can catch up.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if let Some(frame) = push_since(&agent, &mut told) {
                            let _ = outgoing.send(frame);
                        }
                    }

                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            changed = offers.changed(), if offers_open => {
                if changed.is_err() {
                    // The agent is going away; nothing more will be offered.
                    offers_open = false;
                    continue;
                }
                let frame = pair_offer(offers.borrow_and_update().as_ref(), now_secs());
                let _ = outgoing.send(frame);
            }

            _ = keepalive.tick() => {
                let _ = outgoing.send(TunnelFrame {
                    kind: Some(tunnel_frame::Kind::Ping(Default::default())),
                });
            }
        }
    }

    // Dropping our sender ends the writer, which then closes the socket. Waiting
    // for it means the frames we queued are actually gone before we reconnect.
    drop(outgoing);
    let _ = writer.await;

    Ok(())
}

/// What to tell the portal about a configuration change, and remember that we did.
fn push_since(agent: &Agent, told: &mut Option<u64>) -> Option<TunnelFrame> {
    let outcome = agent.catch_up(*told).ok()?;
    let (message, revision) = crate::ws::catch_up_message(outcome).ok()?;

    if let Some(revision) = revision {
        *told = Some(revision);
    }

    message.map(|message| TunnelFrame {
        kind: Some(tunnel_frame::Kind::Push(TunnelPush {
            message: Some(message),
        })),
    })
}

async fn send(sink: &mut ClientSink, frame: &TunnelFrame) -> Result<(), SessionError> {
    let text = encode(frame)?;

    sink.send(Message::text(text))
        .await
        .map_err(|error| SessionError::Transport(Box::new(error)))
}

async fn next_frame(stream: &mut ClientStream) -> Result<TunnelFrame, SessionError> {
    loop {
        let message = stream
            .next()
            .await
            .ok_or(SessionError::Closed)?
            .map_err(|error| SessionError::Transport(Box::new(error)))?;

        // The contract is JSON text end to end, so a binary frame is not part of
        // it. Skipping rather than failing keeps one stray frame from ending an
        // otherwise healthy tunnel.
        let Ok(text) = message.to_text() else {
            continue;
        };

        return Ok(decode(text)?);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::DEFAULT_CONFIG;
    use crate::http;
    use flexdeck_pairing::Identity;
    use flexdeck_protocol::{TunnelChallenge, TunnelWelcome};
    use flexdeck_store::{ConfigStore, DeviceKind};
    use serde_json::Value;
    use std::sync::Arc;

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

    fn config() -> TunnelConfig {
        TunnelConfig {
            url: Some("wss://portal.example.com/tunnel".to_string()),
            name: "Studio PC".to_string(),
        }
    }

    /// A browser paired through the website, as the relay would carry it.
    fn remote_browser(agent: &Agent) -> String {
        agent
            .issue_credential("Laptop", DeviceKind::BrowserRemote)
            .expect("issues")
            .0
    }

    fn request(id: u64, method: &str, path: &str, body: &str, token: &str) -> TunnelRequest {
        TunnelRequest {
            id,
            method: method.to_string(),
            path: path.to_string(),
            body: body.to_string(),
            token: token.to_string(),
        }
    }

    /// Forward a request the way the socket loop will: through the agent's own
    /// router, carrying only what the cloud passed along.
    async fn forward_with(agent: &Arc<Agent>, request: TunnelRequest) -> TunnelResponse {
        let router = http::router(Arc::clone(agent));
        forward(&router, &request)
            .await
            .expect("the request carried an id")
    }

    fn error_message(response: &TunnelResponse) -> String {
        let body: Value = serde_json::from_str(&response.body).expect("a JSON body");
        body["error"]
            .as_str()
            .expect("an error message")
            .to_string()
    }

    #[tokio::test]
    async fn a_forwarded_read_reaches_the_agents_own_api() {
        let agent = agent();
        let token = remote_browser(&agent);
        let response = forward_with(&agent, request(1, "GET", "/api/status", "", &token)).await;

        assert_eq!(response.status, 200);
        assert_eq!(response.id, 1);

        let body: Value = serde_json::from_str(&response.body).expect("JSON");
        assert_eq!(
            body["fingerprint"],
            Value::String(agent.identity().fingerprint())
        );
    }

    #[tokio::test]
    async fn a_forwarded_write_from_a_paired_browser_really_changes_the_agent() {
        // Proves the relay carries a configurator's authority when — and only
        // when — a paired browser's own token comes with it. It also proves the
        // `content-type` header is attached: without it axum's `Json` extractor
        // answers 415 and the write would look like a contract bug.
        let agent = agent();
        let token = remote_browser(&agent);
        let response = forward_with(
            &agent,
            request(1, "PUT", "/api/config", DEFAULT_CONFIG, &token),
        )
        .await;

        assert_eq!(response.status, 200, "body was: {}", response.body);
        assert_eq!(agent.rev().expect("rev"), Some(1));
    }

    #[tokio::test]
    async fn the_cloud_adds_no_authority_of_its_own() {
        // The heart of the no-accounts design. With no token, or with the agent's
        // own admin token (which the cloud could only have by stealing it), a
        // relayed call is refused — the cloud holds nothing that acts here.
        let agent = agent();
        let admin = agent.admin_token().expect("an admin token");

        for token in ["", "guessed", admin.as_str()] {
            let response = forward_with(
                &agent,
                request(1, "PUT", "/api/config", DEFAULT_CONFIG, token),
            )
            .await;
            assert_eq!(response.status, 401, "token {token:?} was honoured");
        }
        assert_eq!(agent.rev().expect("rev"), None, "nothing was written");
    }

    #[tokio::test]
    async fn a_local_browsers_token_is_useless_through_the_relay() {
        let agent = agent();
        let (local, _) = agent
            .issue_credential("This PC", DeviceKind::BrowserLocal)
            .expect("issues");
        let response = forward_with(&agent, request(1, "GET", "/api/status", "", &local)).await;
        assert_eq!(response.status, 401);
    }

    #[tokio::test]
    async fn a_non_2xx_status_passes_through_untouched() {
        // A tunnel that turned every response into 200 would hide the agent's
        // own decisions from the portal.
        let agent = agent();
        let token = remote_browser(&agent);
        let response = forward_with(&agent, request(1, "GET", "/api/config", "", &token)).await;

        assert_eq!(response.status, 404, "no config is committed yet");
    }

    #[tokio::test]
    async fn a_path_outside_the_api_never_reaches_the_router() {
        // `/ws` is the case that matters: it is a WebSocket upgrade, and a
        // request/response frame has no way to continue that conversation. The
        // refusal must be ours, not a generic 404, so the message is asserted —
        // otherwise the test would pass just as well with the check deleted.
        let agent = agent();
        let response = forward_with(&agent, request(1, "GET", "/ws", "", "")).await;

        assert_eq!(response.status, 404);
        assert!(
            error_message(&response).contains("under /api/"),
            "the refusal must come from the tunnel's own path check"
        );
    }

    #[tokio::test]
    async fn a_method_the_api_does_not_allow_is_refused_by_the_router() {
        // `BANANA` is a *valid* HTTP method token, so it is not this module's
        // business to reject it. The request is forwarded and axum answers 405 —
        // which is the more useful answer anyway, and the reason the tunnel does
        // not keep its own list of allowed verbs.
        let agent = agent();
        let token = remote_browser(&agent);
        let response = forward_with(&agent, request(1, "BANANA", "/api/status", "", &token)).await;

        assert_eq!(response.status, 405);
    }

    #[tokio::test]
    async fn a_method_that_is_not_even_a_token_is_refused() {
        // A space is not a valid method character, so this cannot be built into
        // a request at all and has to be refused here.
        let agent = agent();
        let response = forward_with(&agent, request(1, "BAD METHOD", "/api/status", "", "")).await;

        assert_eq!(response.status, 400);
        assert!(error_message(&response).contains("method"));
    }

    #[tokio::test]
    async fn a_body_beyond_the_cap_is_refused_rather_than_allocated() {
        let agent = agent();
        let huge = "x".repeat(MAX_BODY + 1);
        let response = forward_with(&agent, request(1, "PUT", "/api/config", &huge, "")).await;

        assert_eq!(response.status, 413);
    }

    #[tokio::test]
    async fn a_request_without_an_id_is_refused_not_answered() {
        // Id 0 cannot be told apart from "absent" on the wire, because the
        // proto3 JSON mapping omits defaults. Answering it would send a response
        // the cloud could not correlate with anything.
        let agent = agent();
        let router = http::router(Arc::clone(&agent));

        let outcome = forward(&router, &request(0, "GET", "/api/status", "", "")).await;
        assert!(matches!(outcome, Err(ForwardError::Unidentified)));
    }

    #[tokio::test]
    async fn the_hello_proves_the_fingerprint_it_claims() {
        // The fingerprint is what the cloud routes by, so the hello must carry
        // the agent's own — and a signature over the challenge that only the
        // holder of its key could make.
        let agent = agent();
        let frame = hello(&agent, &config().name, "nonce-1").expect("a hello");

        let Some(tunnel_frame::Kind::Hello(greeting)) = frame.kind else {
            panic!("the first frame must be a hello");
        };

        assert_eq!(greeting.fingerprint, agent.identity().fingerprint());
        assert_eq!(greeting.protocol_version, PROTOCOL_VERSION);
        assert_eq!(greeting.name, "Studio PC");
        flexdeck_pairing::verify_proof(
            &greeting.certificate,
            &greeting.fingerprint,
            &flexdeck_pairing::tunnel_proof_message("nonce-1", &greeting.fingerprint),
            &greeting.signature,
        )
        .expect("the proof verifies");
    }

    #[tokio::test]
    async fn the_hello_reports_the_revision_the_portal_should_measure_from() {
        let agent = agent();
        agent
            .replace_config(DEFAULT_CONFIG)
            .expect("a config commits");

        let frame = hello(&agent, &config().name, "n").expect("a hello");
        let Some(tunnel_frame::Kind::Hello(greeting)) = frame.kind else {
            panic!("the first frame must be a hello");
        };

        assert_eq!(greeting.rev, 1);
    }

    #[test]
    fn an_offer_is_routed_until_it_expires_and_then_withdrawn() {
        let offer = BrowserOffer {
            code_hash: "h".to_string(),
            expires_at: 100,
        };

        let Some(tunnel_frame::Kind::PairOffer(live)) = pair_offer(Some(&offer), 99).kind else {
            panic!("a pair offer");
        };
        assert_eq!((live.code_hash.as_str(), live.expires_at), ("h", 100));

        for (offer, now) in [(Some(&offer), 100), (None, 0)] {
            let Some(tunnel_frame::Kind::PairOffer(gone)) = pair_offer(offer, now).kind else {
                panic!("a pair offer");
            };
            assert!(gone.code_hash.is_empty(), "nothing to route");
        }
    }

    #[test]
    fn jitter_stays_within_a_quarter_either_way() {
        let wait = Duration::from_secs(40);
        for _ in 0..50 {
            let jittered = jittered(wait);
            assert!(jittered >= Duration::from_secs(30) && jittered <= Duration::from_secs(50));
        }
    }

    #[tokio::test]
    async fn a_config_change_becomes_a_push_and_is_only_sent_once() {
        let agent = agent();

        let mut told = agent.rev().ok().flatten();
        assert!(
            push_since(&agent, &mut told).is_none(),
            "nothing has changed, so there is nothing to say"
        );

        agent
            .replace_config(DEFAULT_CONFIG)
            .expect("a config commits");

        let frame = push_since(&agent, &mut told).expect("a push");
        assert!(matches!(frame.kind, Some(tunnel_frame::Kind::Push(_))));
        assert_eq!(told, agent.rev().expect("rev"));

        assert!(
            push_since(&agent, &mut told).is_none(),
            "the portal is now current, so repeating it would be noise"
        );
    }

    // -- over a real socket --------------------------------------------------

    type CloudSocket = axum::extract::ws::WebSocket;
    type CloudSink = futures_util::stream::SplitSink<CloudSocket, axum::extract::ws::Message>;
    type CloudStream = futures_util::stream::SplitStream<CloudSocket>;

    /// A fake portal: accepts one tunnel and hands the socket to the test, so the
    /// test can play the cloud's part directly instead of nesting a second
    /// protocol implementation inside a spawn.
    async fn fake_portal() -> (String, mpsc::UnboundedReceiver<CloudSocket>) {
        let (sender, receiver) = mpsc::unbounded_channel();

        let app = axum::Router::new().route(
            "/tunnel",
            axum::routing::get(move |socket: axum::extract::ws::WebSocketUpgrade| {
                let sender = sender.clone();
                async move {
                    socket.on_upgrade(move |socket| async move {
                        let _ = sender.send(socket);
                    })
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binds");
        let address = listener.local_addr().expect("an address");

        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        (format!("ws://{address}/tunnel"), receiver)
    }

    async fn cloud_send(sink: &mut CloudSink, frame: &TunnelFrame) {
        let text = encode(frame).expect("encodes");
        sink.send(axum::extract::ws::Message::text(text))
            .await
            .expect("sends");
    }

    async fn cloud_next(stream: &mut CloudStream) -> TunnelFrame {
        let message = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("a frame within the timeout")
            .expect("a message")
            .expect("no transport error");

        decode(message.to_text().expect("text")).expect("a frame")
    }

    /// The next frame that is not a keepalive or a pairing-offer update.
    async fn cloud_next_of_interest(stream: &mut CloudStream) -> TunnelFrame {
        loop {
            let frame = cloud_next(stream).await;
            match frame.kind {
                Some(tunnel_frame::Kind::Ping(_)) | Some(tunnel_frame::Kind::PairOffer(_)) => {
                    continue
                }
                _ => return frame,
            }
        }
    }

    /// Play the cloud's side of the opening: challenge, verify, welcome.
    async fn open(sink: &mut CloudSink, stream: &mut CloudStream, fingerprint: &str) {
        cloud_send(
            sink,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Challenge(TunnelChallenge {
                    nonce: "fresh-nonce".to_string(),
                })),
            },
        )
        .await;

        let greeting = cloud_next(stream).await;
        let Some(tunnel_frame::Kind::Hello(hello)) = greeting.kind else {
            panic!("expected a hello, got {greeting:?}");
        };
        assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
        assert_eq!(hello.fingerprint, fingerprint);
        flexdeck_pairing::verify_proof(
            &hello.certificate,
            &hello.fingerprint,
            &flexdeck_pairing::tunnel_proof_message("fresh-nonce", &hello.fingerprint),
            &hello.signature,
        )
        .expect("the desktop proves the identity it claims");

        cloud_send(
            sink,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Welcome(TunnelWelcome {})),
            },
        )
        .await;
    }

    #[tokio::test]
    async fn a_desktop_answers_the_challenge_and_then_a_forwarded_request() {
        // The whole desktop side, over a real socket: answer the challenge, be
        // welcomed, take a forwarded call, and answer it with the agent's reply.
        let agent = agent();
        let fingerprint = agent.identity().fingerprint();
        let token = remote_browser(&agent);
        let router = http::router(Arc::clone(&agent));

        let (url, mut portals) = fake_portal().await;
        let (events, mut reported) = mpsc::unbounded_channel();

        let config = Mutex::new(TunnelConfig {
            url: Some(url),
            name: "Studio PC".to_string(),
        });

        let tunnel = tokio::spawn(async move { session(&config, agent, router, &events).await });

        let socket = tokio::time::timeout(Duration::from_secs(5), portals.recv())
            .await
            .expect("the desktop connected")
            .expect("a socket");
        let (mut sink, mut stream) = socket.split();

        open(&mut sink, &mut stream, &fingerprint).await;

        let accepted = reported.recv().await.expect("an event");
        assert!(matches!(accepted, TunnelEvent::Accepted));

        cloud_send(
            &mut sink,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Request(request(
                    7,
                    "GET",
                    "/api/status",
                    "",
                    &token,
                ))),
            },
        )
        .await;

        let reply = loop {
            if let Some(tunnel_frame::Kind::Response(response)) =
                cloud_next_of_interest(&mut stream).await.kind
            {
                break response;
            }
        };

        assert_eq!(reply.id, 7);
        assert_eq!(reply.status, 200);
        assert!(
            reply.body.contains(&fingerprint),
            "the reply must be the agent's own status, got: {}",
            reply.body
        );

        // Both halves are dropped, not just the write one: leaving the read half
        // alive keeps the socket open, so the desktop never sees the portal go
        // away and the session can only end by timing out.
        drop(sink);
        drop(stream);

        let ended = tokio::time::timeout(Duration::from_secs(5), tunnel).await;
        assert!(
            ended.is_ok(),
            "the session must end when the portal does, not linger"
        );
        assert!(ended.expect("ended").expect("ran").is_ok());
    }

    #[tokio::test]
    async fn an_open_browser_code_is_offered_to_the_cloud_by_its_hash() {
        // The website only knows the code the user typed; the cloud finds the
        // desktop by that code's hash, which the desktop offers up the tunnel.
        let agent = agent();
        let fingerprint = agent.identity().fingerprint();
        let router = http::router(Arc::clone(&agent));

        let (url, mut portals) = fake_portal().await;
        let (events, _reported) = mpsc::unbounded_channel();
        let config = Mutex::new(TunnelConfig {
            url: Some(url),
            name: "Studio PC".to_string(),
        });

        let desk = Arc::clone(&agent);
        let tunnel = tokio::spawn(async move { session(&config, desk, router, &events).await });

        let socket = tokio::time::timeout(Duration::from_secs(5), portals.recv())
            .await
            .expect("the desktop connected")
            .expect("a socket");
        let (mut sink, mut stream) = socket.split();
        open(&mut sink, &mut stream, &fingerprint).await;

        let offer = agent.begin_browser_pairing(crate::agent::now_secs());
        let expected = crate::agent::browser_code_hash(&offer.pin);

        let routed = loop {
            if let Some(tunnel_frame::Kind::PairOffer(pair)) = cloud_next(&mut stream).await.kind {
                if !pair.code_hash.is_empty() {
                    break pair;
                }
            }
        };
        assert_eq!(routed.code_hash, expected);
        assert_eq!(routed.expires_at, offer.expires_at);
        assert_ne!(
            routed.code_hash, offer.pin,
            "the code itself never travels up"
        );

        drop(sink);
        drop(stream);
        let _ = tokio::time::timeout(Duration::from_secs(5), tunnel).await;
    }

    #[tokio::test]
    async fn a_portal_that_does_not_challenge_is_not_greeted() {
        // Without a challenge there is nothing to sign, and a hello without a
        // proof is exactly what an impostor portal would want to collect.
        let agent = agent();
        let router = http::router(Arc::clone(&agent));

        let (url, mut portals) = fake_portal().await;
        let (events, _reported) = mpsc::unbounded_channel();
        let config = Mutex::new(TunnelConfig {
            url: Some(url),
            name: "Studio PC".to_string(),
        });

        let tunnel = tokio::spawn(async move { session(&config, agent, router, &events).await });

        let socket = tokio::time::timeout(Duration::from_secs(5), portals.recv())
            .await
            .expect("the desktop connected")
            .expect("a socket");
        let (mut sink, _stream) = socket.split();
        cloud_send(
            &mut sink,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Welcome(TunnelWelcome {})),
            },
        )
        .await;

        let outcome = tokio::time::timeout(Duration::from_secs(5), tunnel)
            .await
            .expect("ended")
            .expect("ran");
        assert!(matches!(outcome, Err(SessionError::Refused { .. })));
    }
}

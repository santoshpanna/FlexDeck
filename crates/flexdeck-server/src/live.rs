//! The configurator's live feed: what changed, the moment it changed.
//!
//! A tablet has had this from the start — its socket ([`crate::ws`]) is told
//! of every new look, page and configuration. The configurator gets the same
//! messages, in the same [`ServerMessage`] shape, two ways:
//!
//! * **On this computer**, a socket of its own at `/api/live` ([`upgrade`]),
//!   reached through the loopback listener, which already holds the page to
//!   the allowed origins. Its first frame is a `ClientHello` carrying the
//!   browser's token — never the URL, which ends up in logs.
//! * **Through the cloud**, down the tunnel. The cloud holds no authority of
//!   its own, so it cannot simply stream a desktop to whoever names its
//!   fingerprint. Instead it asks, with the browser's own token, at
//!   `POST /api/live/watch` ([`watch`]): this desktop decides, exactly as it
//!   decides every relayed request. A yes is a short lease plus the state to
//!   start from; the cloud renews it while the browser stays, and a renewal
//!   refused — the browser was revoked — ends the browser's socket there. The
//!   tunnel carries live frames only while a lease is held ([`RemoteWatch`]),
//!   so a desktop nobody is watching sends nothing upstream.
//!
//! Both are built from one [`Feed`], so the two cannot drift apart.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, State};
use axum::response::Response;
use axum::Json;
use flexdeck_protocol::{client_message, server_message, ClientMessage, DevicesEvent, ErrorEvent, PageEvent, ServerMessage};
use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::access::{Authority, Channel};
use crate::agent::Agent;
use crate::behavior::ViewerGuard;

/// How long one `watch` keeps the tunnel's live frames flowing. The cloud
/// renews well inside it ([`RENEW_EVERY`] there), so a missed renewal or two
/// is not a gap, and a browser that went away stops costing anything soon.
pub const LEASE: Duration = Duration::from_secs(30);

/// How often an open socket's token is checked again, so a browser revoked
/// while watching stops hearing anything.
const RECHECK: Duration = Duration::from_secs(15);

/// How long a socket has to say who it is.
const HELLO: Duration = Duration::from_secs(10);

/// A browser only ever sends its hello.
const MAX_CLIENT_MESSAGE_BYTES: usize = 16 * 1024;

/// Whether a configurator is watching through the cloud, and for how long.
///
/// Holds a [`ViewerGuard`] while it is, so data sources keep running for the
/// browser exactly as they do for a tablet on screen.
#[derive(Default)]
pub struct RemoteWatch {
    lease: Arc<Mutex<Option<(Instant, ViewerGuard)>>>,
}

impl std::fmt::Debug for RemoteWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteWatch").field("watched", &self.is_watched()).finish()
    }
}

impl RemoteWatch {
    /// Start, or extend, the lease.
    pub fn renew(&self, agent: &Agent) {
        self.renew_for(agent, LEASE);
    }

    fn renew_for(&self, agent: &Agent, lease: Duration) {
        {
            let mut held = self.lease.lock().unwrap_or_else(PoisonError::into_inner);
            let guard = match held.take() {
                Some((_, guard)) => guard,
                None => agent.behavior().viewer(),
            };
            *held = Some((Instant::now() + lease, guard));
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let held = Arc::clone(&self.lease);
            handle.spawn(async move {
                tokio::time::sleep(lease).await;
                Self::expire(&held);
            });
        }
    }

    /// Whether the lease still holds. One that ran out is let go here.
    pub fn is_watched(&self) -> bool {
        Self::expire(&self.lease)
    }

    fn expire(lease: &Mutex<Option<(Instant, ViewerGuard)>>) -> bool {
        let mut lease = lease.lock().unwrap_or_else(PoisonError::into_inner);
        if lease.as_ref().is_some_and(|(until, _)| Instant::now() < *until) {
            return true;
        }
        *lease = None;
        false
    }
}

/// Every change a configurator hears about, in arrival order.
///
/// Subscribed on creation, *before* the snapshot is read, so a change landing
/// between the two arrives twice rather than not at all — the same rule the
/// tablet socket follows.
pub struct Feed {
    /// The configuration's moves, and the revision this feed last reported.
    /// Absent for the tunnel, which reports the configuration on its own.
    config: Option<(broadcast::Receiver<u64>, Option<u64>)>,
    states: broadcast::Receiver<flexdeck_protocol::StateEvent>,
    pages: broadcast::Receiver<PageEvent>,
    devices: broadcast::Receiver<()>,
    presence: broadcast::Receiver<()>,
    sources: broadcast::Receiver<(String, crate::sources::RunStatus)>,
}

impl Feed {
    pub fn new(agent: &Agent, with_config: bool) -> Self {
        Self {
            config: with_config.then(|| (agent.subscribe(), None)),
            states: agent.behavior().subscribe_widget_states(),
            pages: agent.pages().subscribe(),
            devices: agent.subscribe_devices(),
            presence: agent.presence().changed(),
            sources: agent.behavior().sources().subscribe_status(),
        }
    }

    /// Where a newcomer starts: every key's look now, the page on screen, and
    /// — for a feed that reports it — the revision, as a patch-less marker
    /// the portal compares with the document it holds.
    pub fn snapshot(&mut self, agent: &Agent) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        if let Some((_, told)) = &mut self.config {
            *told = agent.rev().ok().flatten();
            if let Some(rev) = *told {
                messages.push(revision(rev));
            }
        }
        messages.extend(states(agent));
        if let Some(page_id) = agent.pages().current() {
            messages.push(ServerMessage { kind: Some(server_message::Kind::Page(PageEvent { page_id, step: 0 })) });
        }
        messages
    }

    /// The next change, as the messages that describe it. `None` once the
    /// agent is going away.
    pub async fn next(&mut self, agent: &Agent) -> Option<Vec<ServerMessage>> {
        let Self { config, states: looks, pages, devices: paired, presence, sources } = self;
        let config = async {
            match config {
                Some((changes, told)) => {
                    let moved = changes.recv().await;
                    Some((moved, told))
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            Some((moved, told)) = config => match moved {
                // Lagging loses nothing: the revision is read, not carried.
                Ok(_) | Err(RecvError::Lagged(_)) => {
                    let rev = agent.rev().ok().flatten();
                    if rev == *told {
                        return Some(Vec::new());
                    }
                    *told = rev;
                    Some(rev.map(revision).into_iter().collect())
                }
                Err(RecvError::Closed) => None,
            },
            state = looks.recv() => match state {
                Ok(state) => Some(vec![ServerMessage { kind: Some(server_message::Kind::State(state)) }]),
                // Missed some: every look again, rather than guess which.
                Err(RecvError::Lagged(_)) => Some(states(agent)),
                Err(RecvError::Closed) => None,
            },
            page = pages.recv() => match page {
                Ok(page) => Some(vec![ServerMessage { kind: Some(server_message::Kind::Page(page)) }]),
                Err(RecvError::Lagged(_)) => Some(Vec::new()),
                Err(RecvError::Closed) => None,
            },
            moved = paired.recv() => match moved {
                Ok(()) | Err(RecvError::Lagged(_)) => Some(vec![devices()]),
                Err(RecvError::Closed) => None,
            },
            moved = presence.recv() => match moved {
                Ok(()) | Err(RecvError::Lagged(_)) => Some(vec![devices()]),
                Err(RecvError::Closed) => None,
            },
            run = sources.recv() => match run {
                Ok((source, run)) => Some(vec![source_status(source, run)]),
                // Missed some: every source's status again.
                Err(RecvError::Lagged(_)) => Some(
                    agent.behavior().sources().status().into_iter().map(|(source, run)| source_status(source, run)).collect(),
                ),
                Err(RecvError::Closed) => None,
            },
        }
    }
}

/// "The configuration is at `rev`" — a patch with no operations. The portal
/// reads the document over HTTP; all it needs from here is that it moved.
fn revision(rev: u64) -> ServerMessage {
    ServerMessage {
        kind: Some(server_message::Kind::Patch(flexdeck_protocol::ConfigPatch { rev, patch: "[]".to_string() })),
    }
}

fn source_status(source: String, run: crate::sources::RunStatus) -> ServerMessage {
    ServerMessage {
        kind: Some(server_message::Kind::Source(flexdeck_protocol::SourceStatusEvent {
            source,
            ok: run.ok,
            error: run.error.unwrap_or_default(),
            updated: run.updated,
        })),
    }
}

fn devices() -> ServerMessage {
    ServerMessage { kind: Some(server_message::Kind::Devices(DevicesEvent {})) }
}

fn states(agent: &Agent) -> Vec<ServerMessage> {
    agent
        .behavior()
        .widget_state_snapshot()
        .into_iter()
        .map(|state| ServerMessage { kind: Some(server_message::Kind::State(state)) })
        .collect()
}

/// Only a configurator — a paired browser, or the admin token — watches.
fn admit(agent: &Agent, token: &str, channel: Channel) -> Result<Authority, String> {
    if token.trim().is_empty() {
        return Err("a token is required".to_string());
    }
    agent
        .authorise(token, channel)
        .map_err(|error| error.to_string())?
        .filter(Authority::can_configure)
        .ok_or_else(|| "unknown or revoked token".to_string())
}

/// `POST /api/live/watch`: the cloud asking, with a browser's own token, to
/// carry this desktop's live frames to it. Behind the same authentication as
/// every route, so a token this desktop does not honour never gets here.
pub async fn watch(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
) -> Result<Json<Value>, crate::http::ApiError> {
    if !authority.can_configure() {
        return Err(crate::http::ApiError::forbidden("only a configurator watches the deck"));
    }
    agent.remote_watch().renew(&agent);
    // The revision first, as on this computer, so the browser's first frame
    // always says where things stand — even on a deck with no live keys.
    let mut feed = Feed::new(&agent, true);
    let messages: Vec<Value> = feed
        .snapshot(&agent)
        .iter()
        .filter_map(|message| serde_json::to_value(message).ok())
        .collect();
    Ok(Json(json!({ "leaseSecs": LEASE.as_secs(), "messages": messages })))
}

/// Upgrade to the configurator's socket on this computer.
pub async fn upgrade(
    State(agent): State<Arc<Agent>>,
    channel: Option<Extension<Channel>>,
    socket: WebSocketUpgrade,
) -> Response {
    let channel = channel.map(|Extension(channel)| channel).unwrap_or_default();
    socket
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .max_frame_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(agent, channel, socket))
}

async fn send(socket: &mut WebSocket, message: &ServerMessage) -> bool {
    let Ok(text) = crate::sync::encode(message) else { return false };
    socket.send(Message::text(text)).await.is_ok()
}

async fn refuse(socket: &mut WebSocket, code: &str, reason: &str) {
    let message = ServerMessage {
        kind: Some(server_message::Kind::Error(ErrorEvent { code: code.to_string(), message: reason.to_string() })),
    };
    let _ = send(socket, &message).await;
    let _ = socket.close().await;
}

/// The token in the first frame, a `ClientHello`.
async fn hello(socket: &mut WebSocket) -> Result<String, String> {
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                let message: ClientMessage =
                    serde_json::from_str(text.as_str()).map_err(|_| "the first frame was not a hello".to_string())?;
                return match message.kind {
                    Some(client_message::Kind::Hello(hello)) => Ok(hello.token),
                    _ => Err("the first frame must be a hello".to_string()),
                };
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => return Err("the socket closed before saying hello".to_string()),
        }
    }
}

async fn connection(agent: Arc<Agent>, channel: Channel, mut socket: WebSocket) {
    let token = match tokio::time::timeout(HELLO, hello(&mut socket)).await {
        Ok(Ok(token)) => token,
        Ok(Err(reason)) => return refuse(&mut socket, "bad_hello", &reason).await,
        Err(_) => return refuse(&mut socket, "bad_hello", "no hello arrived in time").await,
    };
    if let Err(reason) = admit(&agent, &token, channel) {
        return refuse(&mut socket, "unauthorised", &reason).await;
    }

    // Sources run while someone is looking, a browser as much as a tablet.
    let _viewer = agent.behavior().viewer();
    let mut feed = Feed::new(&agent, true);
    for message in feed.snapshot(&agent) {
        if !send(&mut socket, &message).await {
            return;
        }
    }

    let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + RECHECK, RECHECK);
    loop {
        tokio::select! {
            messages = feed.next(&agent) => {
                let Some(messages) = messages else { break };
                for message in messages {
                    if !send(&mut socket, &message).await {
                        return;
                    }
                }
            }
            _ = recheck.tick() => {
                if admit(&agent, &token, channel).is_err() {
                    return refuse(&mut socket, "unauthorised", "this browser's access was revoked").await;
                }
            }
            frame = socket.recv() => match frame {
                // The browser says nothing after its hello; a ping is answered
                // by the library, and a close or a failure ends it.
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use flexdeck_pairing::{hash_token, Identity};
    use flexdeck_store::{ConfigStore, DeviceKind};
    use futures_util::StreamExt;
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message as Frame;
    use tower::ServiceExt;

    use super::*;

    type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    /// A scene key: lit while `demo.scene` is "CS2".
    fn agent() -> Arc<Agent> {
        let store = ConfigStore::open_in_memory().expect("store opens");
        store
            .commit(
                &json!({
                    "schema": 2,
                    "profiles": [{"id": "p", "pages": [{"id": "home", "widgets": [{
                        "id": "cs2", "type": "WIDGET_TYPE_BUTTON", "col": 0, "row": 0, "w": 1, "h": 1,
                        "states": {"looks": [{"id": "off"}, {"id": "on"}],
                                   "variable": {"variable": "demo.scene",
                                                "cases": [{"value": "CS2", "state": "on"}],
                                                "otherwise": "off"}}
                    }]}]}]
                })
                .to_string(),
            )
            .expect("commits");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        let agent = Arc::new(Agent::new(store, identity, "127.0.0.1".to_string(), 7878));
        agent
            .behavior()
            .variables()
            .declare(flexdeck_vars::VariableSpec::new("demo.scene", flexdeck_vars::Kind::Text, ""));
        agent.behavior().start(&tokio::runtime::Handle::current());
        agent
    }

    /// The real router, with requests marked as having come in on `channel`
    /// the way the loopback listener and the tunnel mark them.
    fn router(agent: &Arc<Agent>, channel: Option<Channel>) -> axum::Router {
        let router = crate::http::router(Arc::clone(agent));
        match channel {
            Some(channel) => router.layer(Extension(channel)),
            None => router,
        }
    }

    async fn serve(agent: &Arc<Agent>, channel: Option<Channel>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let address = listener.local_addr().expect("has an address");
        let router = router(agent, channel);
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serves") });
        format!("ws://{address}/api/live")
    }

    async fn open(url: &str, token: &str) -> Client {
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.expect("connects");
        let hello = json!({"hello": {"role": "portal", "token": token}});
        socket.send(Frame::text(hello.to_string())).await.expect("says hello");
        socket
    }

    async fn next(socket: &mut Client) -> ServerMessage {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .expect("a frame in time")
                .expect("the socket is open")
                .expect("not a transport error");
            if let Frame::Text(text) = frame {
                return serde_json::from_str(text.as_str()).expect("a contract-shaped message");
            }
        }
    }

    /// Read until `wanted` says yes, failing after a few seconds of silence.
    async fn until(socket: &mut Client, wanted: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        loop {
            let message = next(socket).await;
            if wanted(&message) {
                return message;
            }
        }
    }

    fn is_look(message: &ServerMessage, widget: &str, look: &str) -> bool {
        matches!(&message.kind, Some(server_message::Kind::State(state)) if state.widget_id == widget && state.state_id == look)
    }

    fn is_patch(message: &ServerMessage, rev: u64) -> bool {
        matches!(&message.kind, Some(server_message::Kind::Patch(patch)) if patch.rev == rev)
    }

    #[tokio::test]
    async fn a_browser_on_this_computer_sees_a_scene_go_live_as_it_happens() {
        let agent = agent();
        let (token, _) = agent.issue_credential("Laptop", DeviceKind::BrowserLocal).expect("issued");
        let mut socket = open(&serve(&agent, Some(Channel::Loopback)).await, &token).await;

        // First, where things stand: the revision, then every look.
        let first = next(&mut socket).await;
        assert!(is_patch(&first, 1), "{first:?}");
        until(&mut socket, |message| is_look(message, "cs2", "off")).await;

        agent
            .behavior()
            .variables()
            .refresh("demo.scene", flexdeck_vars::Value::Text("CS2".into()))
            .expect("refreshes");
        until(&mut socket, |message| is_look(message, "cs2", "on")).await;
    }

    #[tokio::test]
    async fn a_configuration_change_and_a_new_device_are_announced() {
        let agent = agent();
        let (token, _) = agent.issue_credential("Laptop", DeviceKind::BrowserLocal).expect("issued");
        let mut socket = open(&serve(&agent, Some(Channel::Loopback)).await, &token).await;
        until(&mut socket, |message| is_look(message, "cs2", "off")).await;

        agent.add_device("tab1", "Tablet", &hash_token("tablet-token")).expect("paired");
        until(&mut socket, |message| matches!(message.kind, Some(server_message::Kind::Devices(_)))).await;

        let (_, config) = agent.config().expect("reads").expect("has one");
        agent.replace_config(&config).expect("saves");
        until(&mut socket, |message| is_patch(message, 2)).await;
    }

    #[tokio::test]
    async fn only_a_configurator_on_its_own_channel_may_watch() {
        let agent = agent();
        agent.add_device("tab1", "Tablet", &hash_token("tablet-token")).expect("paired");
        let (local, _) = agent.issue_credential("Laptop", DeviceKind::BrowserLocal).expect("issued");
        let (remote, _) = agent.issue_credential("Phone", DeviceKind::BrowserRemote).expect("issued");

        let loopback = serve(&agent, Some(Channel::Loopback)).await;
        let lan = serve(&agent, None).await;
        let tries = [(&loopback, "tablet-token"), (&loopback, remote.as_str()), (&lan, local.as_str()), (&loopback, "guessed")];
        for (url, token) in tries {
            let mut socket = open(url, token).await;
            let message = next(&mut socket).await;
            let refused = matches!(&message.kind, Some(server_message::Kind::Error(error)) if error.code == "unauthorised");
            assert!(refused, "{token} on {url}: {message:?}");
        }
    }

    #[tokio::test]
    async fn a_revoked_browser_is_refused_at_the_next_check() {
        let agent = agent();
        let (token, device) = agent.issue_credential("Laptop", DeviceKind::BrowserLocal).expect("issued");
        assert!(admit(&agent, &token, Channel::Loopback).is_ok());
        agent.revoke_device(&device.id).expect("revokes");
        assert!(admit(&agent, &token, Channel::Loopback).is_err());
    }

    #[tokio::test]
    async fn a_source_run_reports_how_it_went_as_it_lands() {
        let agent = agent();
        let (token, _) = agent.issue_credential("Laptop", DeviceKind::BrowserLocal).expect("issued");
        let mut socket = open(&serve(&agent, Some(Channel::Loopback)).await, &token).await;
        until(&mut socket, |message| is_look(message, "cs2", "off")).await;

        let due = crate::sources::Due {
            source: serde_json::from_value(json!({"id": "echo", "kind": "command", "command": {"program": "nonexistent-program-flexdeck"}}))
                .expect("a source"),
            instance: None,
            args: Default::default(),
        };
        let _ = agent.behavior().sources().run(&due, 1_000, &Default::default());
        let heard = until(&mut socket, |message| matches!(message.kind, Some(server_message::Kind::Source(_)))).await;
        let Some(server_message::Kind::Source(run)) = heard.kind else { unreachable!() };
        assert_eq!((run.source.as_str(), run.ok), ("src.echo", false));
        assert!(run.error.contains("could not be started"), "{}", run.error);
    }

    async fn watch_as(agent: &Arc<Agent>, token: &str) -> (StatusCode, Value) {
        let request = Request::post("/api/live/watch")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("a request");
        let response = router(agent, Some(Channel::Relay)).oneshot(request).await.expect("answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("a body");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn a_lease_nobody_renews_stops_counting_as_a_viewer_on_its_own() {
        let agent = agent();
        let watch = RemoteWatch::default();
        watch.renew_for(&agent, Duration::from_millis(50));
        assert_eq!(agent.behavior().viewer_count(), 1);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(agent.behavior().viewer_count(), 0, "released without anyone asking whether it is watched");
    }

    #[tokio::test]
    async fn the_cloud_streams_only_while_this_desktop_says_a_browser_may_watch() {
        let agent = agent();
        agent.add_device("tab1", "Tablet", &hash_token("tablet-token")).expect("paired");
        let (remote, _) = agent.issue_credential("Phone", DeviceKind::BrowserRemote).expect("issued");

        let (status, _) = watch_as(&agent, "tablet-token").await;
        assert_ne!(status, StatusCode::OK, "a tablet is not a configurator");
        assert!(!agent.remote_watch().is_watched());

        let (status, body) = watch_as(&agent, &remote).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(agent.remote_watch().is_watched());
        let starts_with_the_look = body["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .any(|message| message["state"]["widgetId"] == "cs2");
        assert!(starts_with_the_look, "{body}");
    }
}

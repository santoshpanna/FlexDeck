//! The WebSocket transport.
//!
//! The socket carries one conversation: a client identifies itself, is told
//! whether it may proceed, receives the configuration, and is then kept current
//! as the agent's configuration changes.
//!
//! # Authentication happens before anything useful is sent
//!
//! The first frame must be a `ClientHello` carrying a device token. That token
//! is hashed and matched against a stored hash *before* the configuration is
//! sent. A socket that fails this never learns the user's layout, and never
//! receives a single further message.
//!
//! # Each connection remembers its own revision
//!
//! The agent announces "the configuration moved" rather than broadcasting a
//! patch, because a patch is only meaningful relative to a revision. Each
//! connection holds the revision it last received and asks
//! [`crate::sync::catch_up`] for exactly what it needs, which means a client
//! that missed several changes in a row still gets one correct answer instead of
//! a patch it cannot apply.
//!
//! # The client keeps the connection alive; the agent only checks it
//!
//! A connection does not always end with a close frame: a tablet that sleeps, or
//! loses Wi-Fi, leaves the connection open as far as this side can tell. So a
//! live socket is re-checked on a timer, and one that has fallen silent is
//! closed. That is also what makes a revocation take effect on a connection that
//! is already open — otherwise a revoked device could keep driving this machine
//! until it happened to reconnect, and would still be reported as connected.
//!
//! That check *sends nothing*. It only reads local state, because the client
//! already pings on its own timer, and an agent-originated ping is not free: on
//! a battery-powered tablet it forces a radio wake-up whether or not anything
//! has changed. A control surface left on a wall for an hour used to pay for
//! 720 of them. One round trip, driven by the side that has a battery, is enough
//! for both ends to know the other is still there.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use flexdeck_protocol::{
    client_message, server_message, ClientHello, ClientMessage, ErrorEvent, ServerMessage,
    StateEvent,
};
use flexdeck_store::Device;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

use crate::agent::Agent;
use crate::sync::{encode, patch_message, welcome_message, SyncError, SyncOutcome};

/// How often an open socket is re-checked.
///
/// Deliberately NOT a heartbeat. This only re-reads local state — has the token
/// been revoked, has the peer fallen silent — and sends nothing, so its cost
/// falls on the agent's own CPU (one token lookup) rather than on a battery in
/// another room.
const PROBE_SECS: u64 = 30;

/// How long a socket may go without a single frame before it is treated as gone.
///
/// **The client owns the heartbeat.** A tablet pings every 30s
/// (`pingInterval` in `AgentClient`), so any frame at all refreshes this; the
/// agent does not need to originate traffic to learn that the peer is alive.
///
/// Long enough to tolerate two missed pings on a lossy link, and still short
/// enough that a tablet which was switched off does not sit there looking
/// connected.
const LIVENESS_SECS: u64 = 75;

/// How often an open socket is re-checked.
///
/// Shortened under test so the mechanism can be exercised without a thirty-second
/// wait; what is under test is the mechanism, not the wall-clock value. Written
/// with `cfg!` rather than `#[cfg]` so the real constant is compiled in every
/// build.
fn probe_period() -> Duration {
    let seconds = if cfg!(test) { 0.05 } else { PROBE_SECS as f64 };

    Duration::from_secs_f64(seconds)
}

/// How long a socket may go without a frame before it is closed.
///
/// Derived rather than inlined for the same reason as `probe_period`: a test can
/// then prove the timeout actually fires without waiting 75 seconds for it. The
/// test value is deliberately generous — the other socket tests send their next
/// frame whenever they get around to it, and a tight window here would make this
/// change look like a regression in every one of them.
fn liveness_timeout() -> Duration {
    let seconds = if cfg!(test) {
        2.0
    } else {
        LIVENESS_SECS as f64
    };

    Duration::from_secs_f64(seconds)
}

/// How long an unauthenticated socket has to send its hello.
const HELLO_SECS: u64 = 10;

/// The largest message a client may send.
///
/// A client only ever sends a hello or a press, each well under a kilobyte. The
/// library default is 64 MiB, which an unauthenticated peer could make the agent
/// buffer before a single byte of it is checked.
const MAX_CLIENT_MESSAGE_BYTES: usize = 64 * 1024;

/// Shortened under test, for the same reason as [`liveness_timeout`].
fn hello_timeout() -> Duration {
    if cfg!(test) {
        Duration::from_millis(500)
    } else {
        Duration::from_secs(HELLO_SECS)
    }
}

/// Upgrade an HTTP request to a WebSocket.
pub async fn upgrade(State(agent): State<Arc<Agent>>, socket: WebSocketUpgrade) -> Response {
    socket
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .max_frame_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(agent, socket))
}

/// The transport's one security-relevant decision.
///
/// Kept separate from the socket loop so it can be tested directly. A test that
/// needed a live WebSocket to check "a bad token is refused" would be slower,
/// flakier, and no more convincing.
///
/// Returns the device the token belongs to, so the caller can register the
/// connection without looking the token up a second time.
pub(crate) fn admit(agent: &Agent, hello: &ClientHello) -> Result<Device, String> {
    if hello.token.trim().is_empty() {
        return Err("a device token is required".to_string());
    }

    // Compare against the stored *hash*, so a copy of the database is not a copy
    // of the credentials.
    //
    // Only a tablet's: this socket is the deck, and a browser credential is
    // honoured on its own channel and nowhere else (see `crate::access`).
    let device = agent
        .authenticate(&hello.token)
        .map_err(|error| error.to_string())?
        .filter(|device| device.kind == flexdeck_store::DeviceKind::Tablet)
        .ok_or_else(|| "unknown or revoked device token".to_string())?;

    // Verified, but nothing to send yet. Refusing here is friendlier than
    // welcoming the client into an empty conversation.
    if agent.rev().map_err(|error| error.to_string())?.is_none() {
        return Err(format!(
            "the agent has no configuration yet (device {})",
            device.id
        ));
    }

    Ok(device)
}

/// Turn a catch-up decision into the message to send, plus the revision the
/// client will hold afterwards.
///
/// A `None` message means there is nothing to send. A `None` revision means the
/// client is already where it should be.
pub(crate) fn catch_up_message(
    outcome: SyncOutcome,
) -> Result<(Option<ServerMessage>, Option<u64>), SyncError> {
    match outcome {
        SyncOutcome::UpToDate => Ok((None, None)),
        SyncOutcome::Patch { rev, patch } => Ok((Some(patch_message(rev, &patch)), Some(rev))),
        SyncOutcome::Snapshot { rev, config } => {
            // Profile selection is not implemented yet, so `profile_id` stays
            // empty rather than naming a profile nobody chose. The client is
            // expected to fall back to the configuration's own default.
            Ok((Some(welcome_message(rev, "", &config)?), Some(rev)))
        }
    }
}

/// Compute what a client at `from` needs, without touching a socket.
///
/// `None` means the decision itself failed and the connection should end.
fn prepare_catch_up(
    agent: &Agent,
    from: Option<u64>,
) -> Option<(Option<ServerMessage>, Option<u64>)> {
    let outcome = agent
        .catch_up(from)
        .map_err(|error| tracing::warn!(%error, ?from, "catch-up failed"))
        .ok()?;
    catch_up_message(outcome)
        .map_err(|error| tracing::warn!(%error, ?from, "catch-up message failed"))
        .ok()
}

/// Decode a client frame, ignoring anything unrecognisable.
///
/// A malformed frame from an already-authenticated client is not worth dropping
/// the connection over — the tablet may be an older build, and the socket still
/// has value for *receiving* configuration changes.
fn decode_frame(text: &str) -> Option<client_message::Kind> {
    serde_json::from_str::<ClientMessage>(text).ok()?.kind
}

async fn send_message(sender: &mut SplitSink<WebSocket, Message>, message: &ServerMessage) -> bool {
    let Ok(text) = encode(message) else {
        return false;
    };
    sender.send(Message::text(text)).await.is_ok()
}

/// Wait for the first frame, which must be a `ClientHello`.
/// Every bound widget's current state. `false` if the socket has gone.
async fn send_widget_states(agent: &Agent, sender: &mut SplitSink<WebSocket, Message>) -> bool {
    for state in agent.behavior().widget_state_snapshot() {
        let message = ServerMessage {
            kind: Some(server_message::Kind::State(state)),
        };
        if !send_message(sender, &message).await {
            return false;
        }
    }
    true
}

/// Every graphed variable's history. `false` if the socket has gone.
async fn send_series(agent: &Agent, sender: &mut SplitSink<WebSocket, Message>) -> bool {
    for event in agent.behavior().series_snapshot() {
        let message = ServerMessage {
            kind: Some(server_message::Kind::Series(event)),
        };
        if !send_message(sender, &message).await {
            return false;
        }
    }
    true
}

async fn await_hello(socket: &mut WebSocket) -> Result<ClientHello, String> {
    loop {
        let Some(frame) = socket.recv().await else {
            return Err("the client disconnected before saying hello".to_string());
        };

        match frame {
            Ok(Message::Text(text)) => {
                let message: ClientMessage = serde_json::from_str(text.as_str())
                    .map_err(|error| format!("the first frame was not valid JSON: {error}"))?;

                return match message.kind {
                    Some(client_message::Kind::Hello(hello)) => Ok(hello),
                    Some(
                        client_message::Kind::Press(_)
                        | client_message::Kind::Value(_)
                        | client_message::Kind::Page(_),
                    ) => {
                        Err("the first frame must be a hello, not a press".to_string())
                    }
                    None => Err("the first frame carried no message".to_string()),
                };
            }
            // Protocol-level frames are not application data.
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(Message::Binary(_)) => {
                return Err("this protocol is JSON; binary frames are not accepted".to_string())
            }
            Ok(Message::Close(_)) => {
                return Err("the client closed before saying hello".to_string())
            }
            Err(error) => return Err(format!("the socket failed: {error}")),
        }
    }
}

/// Send a contract-shaped error, then close.
///
/// Using the contract's own `ErrorEvent` means a client parses a refusal with
/// its generated types rather than a bespoke error format.
async fn refuse(socket: &mut WebSocket, code: &str, reason: &str) {
    let message = ServerMessage {
        kind: Some(server_message::Kind::Error(ErrorEvent {
            code: code.to_string(),
            message: reason.to_string(),
        })),
    };

    if let Ok(text) = encode(&message) {
        let _ = socket.send(Message::text(text)).await;
    }
    let _ = socket.close().await;
}

async fn connection(agent: Arc<Agent>, mut socket: WebSocket) {
    // Bounded, because the socket is open to anyone on the network until the
    // hello is checked: without a deadline, a peer that connects and then says
    // nothing (or only pings) holds a task and a connection forever.
    let hello = match tokio::time::timeout(hello_timeout(), await_hello(&mut socket)).await {
        Ok(Ok(hello)) => hello,
        Ok(Err(reason)) => return refuse(&mut socket, "bad_hello", &reason).await,
        Err(_) => {
            return refuse(&mut socket, "bad_hello", "no hello arrived in time").await;
        }
    };

    let device = match admit(&agent, &hello) {
        Ok(device) => device,
        Err(reason) => return refuse(&mut socket, "unauthorised", &reason).await,
    };

    // The socket's liveness, as a value. Dropping it when this task ends is what
    // takes the device offline — and because the task owns it rather than a call
    // releasing it, it also covers every early return below.
    let _presence = agent.presence().connect(&device.id);

    // Subscribe *before* reading the current revision. A change landing between
    // the two would otherwise be missed entirely; subscribing first means the
    // worst case is a redundant notification that finds nothing to do.
    let mut changes = agent.subscribe();
    // Live widget states. Subscribed before the snapshot below for the same
    // reason as the configuration: a change between the two is then a
    // duplicate, never a gap.
    let mut widget_states = agent.behavior().subscribe_widget_states();
    let mut series = agent.behavior().subscribe_series();
    let mut pages = agent.pages().subscribe();
    let _seat = agent.pages().seat();
    let _viewer = agent.behavior().viewer();
    let Some((initial, initial_rev)) = prepare_catch_up(&agent, None) else {
        return refuse(&mut socket, "no_config", "the agent has no configuration").await;
    };

    let (mut sender, mut receiver) = socket.split();

    if let Some(message) = initial {
        if !send_message(&mut sender, &message).await {
            return;
        }
    }

    if !send_widget_states(&agent, &mut sender).await {
        return;
    }
    if !send_series(&agent, &mut sender).await {
        return;
    }
    // The page everyone else is on, so a tablet joins there.
    if let Some(page_id) = agent.pages().current() {
        let message = ServerMessage {
            kind: Some(server_message::Kind::Page(flexdeck_protocol::PageEvent { page_id, step: 0 })),
        };
        if !send_message(&mut sender, &message).await {
            return;
        }
    }

    let mut client_rev = initial_rev;

    let mut probe = tokio::time::interval_at(Instant::now() + probe_period(), probe_period());
    let mut last_seen = Instant::now();
    // Widgets this tablet is holding down. If it goes away mid-hold — the
    // Wi-Fi drops, the app is killed — they are released for it below, so a
    // push-to-talk button never leaves a microphone open.
    let mut held: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    loop {
        tokio::select! {
            change = changes.recv() => {
                match change {
                    // Lagging is harmless: the client's revision is remembered
                    // here, not in the channel, so nothing is lost.
                    Ok(_) | Err(RecvError::Lagged(_)) => {
                        let Some((message, rev)) = prepare_catch_up(&agent, client_rev) else {
                            break;
                        };

                        let sent_config = message.is_some();
                        if let Some(message) = message {
                            if !send_message(&mut sender, &message).await {
                                break;
                            }
                        }
                        client_rev = rev.or(client_rev);
                        // A new configuration may bind different widgets,
                        // and graph different values.
                        if sent_config
                            && !(send_widget_states(&agent, &mut sender).await
                                && send_series(&agent, &mut sender).await)
                        {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            state = widget_states.recv() => {
                match state {
                    Ok(state) => {
                        let message = ServerMessage {
                            kind: Some(server_message::Kind::State(state)),
                        };
                        if !send_message(&mut sender, &message).await {
                            break;
                        }
                    }
                    // Missed some: send them all again rather than guess which.
                    Err(RecvError::Lagged(_)) => {
                        if !send_widget_states(&agent, &mut sender).await {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            point = series.recv() => {
                match point {
                    Ok(event) => {
                        let message = ServerMessage { kind: Some(server_message::Kind::Series(event)) };
                        if !send_message(&mut sender, &message).await {
                            break;
                        }
                    }
                    // Missed points: every history again, whole.
                    Err(RecvError::Lagged(_)) => {
                        if !send_series(&agent, &mut sender).await {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            page = pages.recv() => {
                match page {
                    Ok(event) => {
                        let message = ServerMessage { kind: Some(server_message::Kind::Page(event)) };
                        if !send_message(&mut sender, &message).await {
                            break;
                        }
                    }
                    // A page request that is late is not worth replaying.
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
            // A connection does not always end with a close frame, so a live one
            // is probed. Two things are noticed here that nothing else notices:
            // a peer that went away without saying so, which would otherwise be
            // reported as connected for as long as the OS held the socket, and a
            // token revoked while this socket was open.
            _ = probe.tick() => {
                if last_seen.elapsed() >= liveness_timeout() {
                    break;
                }

                // Re-checked on the timer rather than only at connect, so a
                // token revoked while this socket is open takes effect now
                // instead of at the next reconnect.
                if agent.authenticate(&hello.token).ok().flatten().is_none() {
                    break;
                }
            }
            frame = receiver.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        last_seen = Instant::now();

                        let decoded = decode_frame(text.as_str());
                        if let Some(client_message::Kind::Page(shown)) = &decoded {
                            agent.pages().report(&shown.page_id, shown.initial);
                            continue;
                        }
                        if let Some(client_message::Kind::Value(value)) = &decoded {
                            // Never awaited: the slider's worker runs it, and
                            // this socket goes straight back to listening.
                            agent.behavior().slide(&value.widget_id, value.value);
                            continue;
                        }
                        if let Some(client_message::Kind::Press(press)) = decoded {
                            // A press that maps to nothing is not worth closing
                            // over; the configuration may have changed under
                            // the client.
                            //
                            // The result is reported back rather than dropped.
                            // A control surface that swallows a press leaves
                            // the user unable to tell a working button from a
                            // dead one, and an unknown widget is exactly the
                            // case where that matters most.
                            let gesture = flexdeck_protocol::Gesture::try_from(press.gesture)
                                .ok()
                                .filter(|g| *g != flexdeck_protocol::Gesture::Unspecified)
                                .unwrap_or(flexdeck_protocol::Gesture::ShortPress);
                            match gesture {
                                flexdeck_protocol::Gesture::HoldStart => {
                                    held.insert(press.widget_id.clone());
                                }
                                flexdeck_protocol::Gesture::HoldEnd => {
                                    held.remove(&press.widget_id);
                                }
                                _ => {}
                            }
                            let outcome = {
                                // Flows may wait; never on an async worker.
                                let agent = Arc::clone(&agent);
                                let widget_id = press.widget_id.clone();
                                let zone = press.zone.clone();
                                tokio::task::spawn_blocking(move || agent.interact_in(&widget_id, gesture, &zone))
                                    .await
                            };
                            // The widget's look after the press, with how the
                            // press went: a failure says why, in words.
                            let mut state = agent
                                .behavior()
                                .widget_state(&press.widget_id)
                                .unwrap_or_else(|| StateEvent {
                                    widget_id: press.widget_id.clone(),
                                    ok: true,
                                    ..StateEvent::default()
                                });
                            match outcome {
                                Ok(Ok(interaction)) => {
                                    if let Some(failure) = interaction.failure {
                                        state.ok = false;
                                        state.error = failure;
                                    }
                                }
                                Ok(Err(error)) => {
                                    state.ok = false;
                                    state.error = error.to_string();
                                }
                                Err(error) => {
                                    state.ok = false;
                                    state.error = error.to_string();
                                }
                            }

                            let message = ServerMessage {
                                kind: Some(server_message::Kind::State(state)),
                            };

                            if !send_message(&mut sender, &message).await {
                                break;
                            }
                        }
                    }
                    // A close frame, or a socket that ended, ends the loop.
                    Some(Ok(Message::Close(_))) | None => break,
                    // Any other frame is proof the peer is still there. A pong to
                    // our probe is the usual one.
                    Some(Ok(_)) => last_seen = Instant::now(),
                    Some(Err(_)) => break,
                }
            }
        }
    }

    tracing::info!(device = %device.name, rev = ?client_rev, "tablet socket closed");

    // Whatever this tablet was still holding is let go, as if the finger had
    // lifted. Done after the loop, so every way out of it is covered.
    for widget_id in held {
        let agent = Arc::clone(&agent);
        let _ = tokio::task::spawn_blocking(move || {
            agent.interact(&widget_id, flexdeck_protocol::Gesture::HoldEnd)
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_pairing::{hash_token, Identity};
    use flexdeck_store::ConfigStore;

    const CONFIG: &str = r#"{"schema":2,"rev":1,"profiles":[]}"#;

    fn agent(committed: bool) -> Arc<Agent> {
        let store = ConfigStore::open_in_memory().expect("store opens");
        if committed {
            store.commit(CONFIG).expect("commits");
        }
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        Arc::new(Agent::new(store, identity, "127.0.0.1".to_string(), 7878))
    }

    fn hello(token: &str) -> ClientHello {
        ClientHello {
            role: "android".to_string(),
            device_id: "tablet".to_string(),
            token: token.to_string(),
            device: None,
        }
    }

    #[test]
    fn a_known_token_is_admitted() {
        let agent = agent(true);
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");

        let device = admit(&agent, &hello("good-token")).expect("admitted");
        assert_eq!(device.id, "tab1");
    }

    #[test]
    fn an_unknown_token_is_refused() {
        let agent = agent(true);
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");

        let refusal = admit(&agent, &hello("guessed-token")).expect_err("should refuse");
        assert!(refusal.contains("token"), "{refusal}");
    }

    #[test]
    fn an_empty_token_is_refused_before_any_lookup() {
        let refusal = admit(&agent(true), &hello("")).expect_err("should refuse");
        assert!(refusal.contains("required"), "{refusal}");
    }

    #[test]
    fn a_revoked_token_is_refused() {
        let agent = agent(true);
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        agent.revoke_device("tab1").expect("revokes");

        // Revocation must take effect on the very next connection attempt.
        assert!(admit(&agent, &hello("good-token")).is_err());
    }

    #[test]
    fn a_valid_token_is_still_refused_when_there_is_nothing_to_send() {
        let agent = agent(false);
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");

        let refusal = admit(&agent, &hello("good-token")).expect_err("should refuse");
        assert!(refusal.contains("no configuration"), "{refusal}");
    }

    #[test]
    fn a_current_client_needs_no_message() {
        let agent = agent(true);
        let rev = agent.rev().expect("rev").expect("exists");

        let (message, revision) =
            catch_up_message(agent.catch_up(Some(rev)).expect("decides")).expect("builds");

        assert!(message.is_none(), "nothing to send");
        assert!(revision.is_none(), "and nothing changes");
    }

    #[test]
    fn a_client_behind_gets_a_patch_message() {
        let agent = agent(true);
        agent
            .replace_config(r#"{"schema":2,"rev":2,"profiles":[]}"#)
            .expect("commits");

        let (message, revision) =
            catch_up_message(agent.catch_up(Some(1)).expect("decides")).expect("builds");

        let Some(server_message::Kind::Patch(patch)) = message.expect("a message").kind else {
            panic!("expected a patch");
        };
        assert_eq!(revision, Some(2));
        assert_eq!(patch.rev, 2);
        assert!(patch.patch.starts_with('['), "a JSON Patch is an array");
    }

    #[test]
    fn a_client_with_no_revision_gets_a_welcome() {
        let agent = agent(true);

        let (message, revision) =
            catch_up_message(agent.catch_up(None).expect("decides")).expect("builds");

        let Some(server_message::Kind::Welcome(welcome)) = message.expect("a message").kind else {
            panic!("expected a welcome");
        };
        assert_eq!(revision, Some(1));
        assert!(welcome.config.is_some(), "a welcome carries the document");
    }

    #[test]
    fn a_malformed_frame_is_ignored_rather_than_fatal() {
        assert!(decode_frame("not json").is_none());
        assert!(decode_frame("{}").is_none());
    }

    #[test]
    fn a_press_frame_decodes() {
        let decoded =
            decode_frame(r#"{"press":{"widgetId":"mute","gesture":"GESTURE_LONG_PRESS"}}"#);

        let Some(client_message::Kind::Press(press)) = decoded else {
            panic!("expected a press");
        };
        assert_eq!(press.widget_id, "mute");
        assert_eq!(press.gesture, flexdeck_protocol::Gesture::LongPress as i32);
    }

    #[test]
    fn a_page_frame_decodes() {
        // As `AgentClient.sendPage` writes it.
        let decoded = decode_frame(r#"{"page":{"pageId":"stream","initial":true}}"#);

        let Some(client_message::Kind::Page(shown)) = decoded else {
            panic!("expected a page");
        };
        assert_eq!(shown.page_id, "stream");
        assert!(shown.initial);
    }

    #[test]
    fn a_schema_one_press_frame_is_not_understood() {
        // The free-text `type` is gone; an old tablet's frame is ignored, not
        // misread as some gesture.
        assert!(decode_frame(r#"{"press":{"widgetId":"mute","type":"press"}}"#).is_none());
    }
}

/// End-to-end tests over a real socket.
///
/// The tests above check the *decisions* — is this token good, what should be
/// sent. These check that a real connection actually carries them out. The
/// distinction matters: every decision could be individually correct while the
/// loop driving them is wrong, and nothing else in the suite would notice.
#[cfg(test)]
mod socket {
    use std::time::Duration;

    use flexdeck_actions::{ActionError, CoreAction, Executor};
    use flexdeck_pairing::{hash_token, Identity};
    use flexdeck_store::ConfigStore;
    use serde_json::{json, Value};
    use tokio::sync::{broadcast, mpsc};
    use tokio_tungstenite::tungstenite::Message as Frame;

    use super::*;
    use crate::http;

    const CONFIG: &str = r#"{
      "schema": 2,
      "rev": 1,
      "profiles": [{
        "id": "tab1",
        "pages": [{
          "id": "home",
          "widgets": [{
            "id": "mute",
            "type": "WIDGET_TYPE_BUTTON",
            "col": 0, "row": 0, "w": 1, "h": 1,
            "flows": [{
              "trigger": {"gesture": "GESTURE_SHORT_PRESS"},
              "blocks": [{"block": "doAction", "plugin": "core", "action": "hotkey",
                          "args": {"keys": {"text": "ctrl+shift+m"}}}]
            }]
          }]
        }]
      }]
    }"#;

    type Client = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    /// Reports what ran, so a test can *wait* for a press to arrive rather than
    /// sleeping and hoping it did.
    struct SignallingExecutor {
        performed: mpsc::UnboundedSender<CoreAction>,
    }

    impl Executor for SignallingExecutor {
        fn perform(&mut self, action: &CoreAction) -> Result<(), ActionError> {
            let _ = self.performed.send(action.clone());
            Ok(())
        }
    }

    fn agent() -> Arc<Agent> {
        let store = ConfigStore::open_in_memory().expect("store opens");
        store.commit(CONFIG).expect("commits");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        Arc::new(Agent::new(store, identity, "127.0.0.1".to_string(), 7878))
    }

    /// Serve the *real* router on an ephemeral port, and return its socket URL.
    async fn serve(agent: Arc<Agent>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binds an ephemeral port");
        let address = listener.local_addr().expect("has an address");

        tokio::spawn(async move {
            axum::serve(listener, http::router(agent))
                .await
                .expect("the server runs");
        });

        format!("ws://{address}/ws")
    }

    async fn connect(url: &str) -> Client {
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("connects");
        socket
    }

    async fn say(socket: &mut Client, message: Value) {
        socket
            .send(Frame::text(message.to_string()))
            .await
            .expect("sends");
    }

    async fn next_text(socket: &mut Client) -> String {
        loop {
            let frame = socket
                .next()
                .await
                .expect("a frame arrives")
                .expect("the frame is not a transport error");

            match frame {
                Frame::Text(text) => return text.as_str().to_string(),
                // The agent no longer originates pings, but a client may, and
                // a control frame is never one of the messages under test. Skip
                // it rather than failing on it.
                Frame::Ping(_) | Frame::Pong(_) => continue,
                other => panic!("expected a text frame, got {other:?}"),
            }
        }
    }

    async fn next_message(socket: &mut Client) -> ServerMessage {
        serde_json::from_str(&next_text(socket).await).expect("a contract-shaped message")
    }

    fn hello(token: &str) -> Value {
        json!({"hello": {"role": "android", "deviceId": "tablet", "token": token}})
    }

    #[tokio::test]
    async fn a_paired_device_is_welcomed_with_the_configuration() {
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let url = serve(agent).await;

        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;

        let message = next_message(&mut socket).await;
        let Some(server_message::Kind::Welcome(welcome)) = message.kind else {
            panic!("expected a welcome, got {message:?}");
        };

        assert_eq!(welcome.rev, 1);
        let config = welcome.config.expect("a welcome carries the document");
        assert_eq!(config.profiles.len(), 1, "and the real one, not a stub");
    }

    #[tokio::test]
    async fn a_bound_widget_follows_its_variable_live() {
        // The record button turns on, and its label shows the time, because the
        // variables they are bound to changed — nobody pressed anything.
        let store = ConfigStore::open_in_memory().expect("store opens");
        store
            .commit(
                &json!({
                    "schema": 2,
                    "profiles": [{"id": "p", "pages": [{"id": "page", "widgets": [{
                        "id": "rec", "type": "WIDGET_TYPE_BUTTON", "col": 0, "row": 0, "w": 1, "h": 1,
                        "props": {"label": "Record", "labelTemplate": "REC {{demo.time}}"},
                        "states": {"looks": [{"id": "off"}, {"id": "on"}],
                                   "variable": {"variable": "demo.on",
                                                "cases": [{"value": "true", "state": "on"}],
                                                "otherwise": "off"}}
                    }]}]}]
                })
                .to_string(),
            )
            .expect("commits");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        let agent = Arc::new(Agent::new(store, identity, "127.0.0.1".to_string(), 7878));
        agent
            .add_device("tab1", "Tablet", &hash_token("good-token"))
            .expect("device added");
        let vars = Arc::clone(agent.behavior().variables());
        vars.declare(flexdeck_vars::VariableSpec::new(
            "demo.on",
            flexdeck_vars::Kind::Bool,
            "",
        ));
        vars.declare(flexdeck_vars::VariableSpec::new(
            "demo.time",
            flexdeck_vars::Kind::Text,
            "",
        ));
        agent.behavior().start(&tokio::runtime::Handle::current());
        let url = serve(Arc::clone(&agent)).await;

        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let welcome = next_message(&mut socket).await;
        assert!(matches!(
            welcome.kind,
            Some(server_message::Kind::Welcome(_))
        ));

        // On connect: the state as it is now, which is "nothing known yet".
        let first = next_message(&mut socket).await;
        let Some(server_message::Kind::State(state)) = first.kind else {
            panic!("expected the bound widget's state, got {first:?}");
        };
        assert_eq!(
            (state.widget_id.as_str(), state.state_id.as_str()),
            ("rec", "off")
        );
        assert_eq!(state.label, "REC –");

        vars.refresh("demo.time", flexdeck_vars::Value::Text("00:00:07".into()))
            .expect("refreshes");
        vars.refresh("demo.on", flexdeck_vars::Value::Bool(true))
            .expect("refreshes");

        // Each change pushes the widget's whole state; wait for the one where
        // both have landed.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let message = tokio::time::timeout_at(deadline, next_message(&mut socket))
                .await
                .expect("the live state arrives");
            if let Some(server_message::Kind::State(state)) = message.kind {
                if state.state_id == "on" && state.label == "REC 00:00:07" {
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn an_unknown_token_never_sees_the_configuration() {
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let url = serve(agent).await;

        let mut socket = connect(&url).await;
        say(&mut socket, hello("guessed-token")).await;

        let raw = next_text(&mut socket).await;
        let message: ServerMessage = serde_json::from_str(&raw).expect("a contract-shaped message");
        let Some(server_message::Kind::Error(error)) = message.kind else {
            panic!("expected a refusal, got {message:?}");
        };

        assert_eq!(error.code, "unauthorised");
        // The whole point of refusing before the welcome: none of the user's
        // layout crosses the wire.
        assert!(!raw.contains("profiles"), "leaked the configuration: {raw}");
        assert!(!raw.contains("mute"), "leaked the configuration: {raw}");
    }

    #[tokio::test]
    async fn a_socket_that_never_says_hello_is_closed() {
        // Anyone on the network can open this socket. Until the hello is checked
        // it costs the agent a task, so silence — or nothing but pings — must not
        // be able to keep it open.
        let url = serve(agent()).await;
        let mut socket = connect(&url).await;
        socket
            .send(Frame::Ping(Vec::new().into()))
            .await
            .expect("sends");

        let message = tokio::time::timeout(Duration::from_secs(5), next_message(&mut socket))
            .await
            .expect("the agent gave up on the silent peer");
        let Some(server_message::Kind::Error(error)) = message.kind else {
            panic!("expected a refusal, got {message:?}");
        };
        assert_eq!(error.code, "bad_hello");
    }

    #[tokio::test]
    async fn an_oversized_first_frame_is_dropped_unread() {
        // The library default would buffer 64 MiB from a peer that has not yet
        // shown a credential. A hello is a few hundred bytes.
        let url = serve(agent()).await;
        let mut socket = connect(&url).await;
        let huge = "x".repeat(MAX_CLIENT_MESSAGE_BYTES + 1);
        let _ = socket.send(Frame::text(huge)).await;

        // The agent closes the connection rather than parsing it; whatever the
        // client sees next, it is never a welcome.
        let next = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("the agent reacted");
        if let Some(Ok(Frame::Text(text))) = next {
            assert!(
                !text.contains("welcome"),
                "an oversized frame was accepted: {text}"
            );
        }
    }

    #[tokio::test]
    async fn a_configuration_change_is_pushed_down_an_open_socket() {
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let url = serve(Arc::clone(&agent)).await;

        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;

        // Change the configuration from *outside* the socket, then wait for the
        // agent to push the difference down the same connection.
        agent
            .replace_config(r#"{"schema":2,"rev":2,"profiles":[]}"#)
            .expect("commits");

        let message = next_message(&mut socket).await;
        let Some(server_message::Kind::Patch(patch)) = message.kind else {
            panic!("expected a patch, got {message:?}");
        };

        assert_eq!(patch.rev, 2);
        assert!(patch.patch.starts_with('['), "a JSON Patch is an array");
    }

    #[tokio::test]
    async fn a_press_travels_from_the_socket_to_the_action_engine() {
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");

        let (performed, mut heard) = mpsc::unbounded_channel();
        agent.install_live_executor(Box::new(SignallingExecutor { performed }));

        let url = serve(Arc::clone(&agent)).await;
        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;

        say(
            &mut socket,
            json!({"press": {"widgetId": "mute", "gesture": "GESTURE_SHORT_PRESS"}}),
        )
        .await;

        // Wait for the action to actually run. The timeout only bounds how long
        // a genuine failure takes to report; it is not a sleep.
        let action = tokio::time::timeout(Duration::from_secs(5), heard.recv())
            .await
            .expect("the press should reach the action engine")
            .expect("a value");

        let CoreAction::Hotkey(hotkey) = action else {
            panic!("expected a hotkey, got {action:?}");
        };
        assert_eq!(hotkey.to_string(), "ctrl+shift+m");
    }

    #[tokio::test]
    async fn a_press_for_a_widget_that_does_not_exist_is_harmless() {
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");

        let (performed, mut heard) = mpsc::unbounded_channel();
        agent.install_live_executor(Box::new(SignallingExecutor { performed }));

        let url = serve(Arc::clone(&agent)).await;
        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;

        say(
            &mut socket,
            json!({"press": {"widgetId": "never-existed", "gesture": "GESTURE_SHORT_PRESS"}}),
        )
        .await;

        // Prove the frame was processed by making the agent push something
        // afterwards, then confirm nothing ran.
        agent
            .replace_config(r#"{"schema":2,"rev":2,"profiles":[]}"#)
            .expect("commits");
        let _patch = next_message(&mut socket).await;

        assert!(
            heard.try_recv().is_err(),
            "an unknown widget must not run anything"
        );
    }

    fn type_text(text: Value) -> Value {
        json!({"block": "doAction", "plugin": "core", "action": "text", "args": {"text": text}})
    }

    fn press_key(keys: &str) -> Value {
        json!({"block": "doAction", "plugin": "core", "action": "hotkey", "args": {"keys": {"text": keys}}})
    }

    /// An agent serving one deck of `widgets`, with an executor that reports.
    async fn deck_agent(
        widgets: Value,
    ) -> (Arc<Agent>, String, mpsc::UnboundedReceiver<CoreAction>) {
        let store = ConfigStore::open_in_memory().expect("store opens");
        store
            .commit(
                &json!({"schema": 3, "profiles": [{"id": "p", "pages": [{"id": "page", "widgets": widgets}]}]})
                    .to_string(),
            )
            .expect("commits");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        let agent = Arc::new(Agent::new(store, identity, "127.0.0.1".to_string(), 7878));
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let (performed, heard) = mpsc::unbounded_channel();
        agent.install_live_executor(Box::new(SignallingExecutor { performed }));
        let url = serve(Arc::clone(&agent)).await;
        (agent, url, heard)
    }

    async fn heard_within(heard: &mut mpsc::UnboundedReceiver<CoreAction>, what: &str) -> CoreAction {
        tokio::time::timeout(Duration::from_secs(5), heard.recv())
            .await
            .unwrap_or_else(|_| panic!("{what} never ran"))
            .expect("a value")
    }

    #[tokio::test]
    async fn a_held_button_is_released_when_the_tablet_goes_away() {
        // Push-to-talk on a tablet whose Wi-Fi drops mid-hold: the agent must
        // let go for it, or the microphone stays open.
        let (_agent, url, mut heard) = deck_agent(json!([{
            "id": "ptt", "type": "WIDGET_TYPE_BUTTON", "col": 0, "row": 0, "w": 1, "h": 1,
            "flows": [
                {"trigger": {"gesture": "GESTURE_HOLD_START"}, "blocks": [press_key("ctrl+f1")]},
                {"trigger": {"gesture": "GESTURE_HOLD_END"}, "blocks": [press_key("ctrl+f2")]}
            ]
        }]))
        .await;
        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;

        say(&mut socket, json!({"press": {"widgetId": "ptt", "gesture": "GESTURE_HOLD_START"}})).await;
        let CoreAction::Hotkey(down) = heard_within(&mut heard, "the hold").await else {
            panic!("a hotkey")
        };
        assert_eq!(down.to_string(), "ctrl+f1");

        drop(socket);
        let CoreAction::Hotkey(up) = heard_within(&mut heard, "the release").await else {
            panic!("a hotkey")
        };
        assert_eq!(up.to_string(), "ctrl+f2", "released for the tablet that left");
    }

    #[tokio::test]
    async fn a_released_hold_is_not_released_twice() {
        let (_agent, url, mut heard) = deck_agent(json!([{
            "id": "ptt", "type": "WIDGET_TYPE_BUTTON", "col": 0, "row": 0, "w": 1, "h": 1,
            "flows": [
                {"trigger": {"gesture": "GESTURE_HOLD_START"}, "blocks": [press_key("ctrl+f1")]},
                {"trigger": {"gesture": "GESTURE_HOLD_END"}, "blocks": [press_key("ctrl+f2")]}
            ]
        }]))
        .await;
        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;
        say(&mut socket, json!({"press": {"widgetId": "ptt", "gesture": "GESTURE_HOLD_START"}})).await;
        say(&mut socket, json!({"press": {"widgetId": "ptt", "gesture": "GESTURE_HOLD_END"}})).await;
        heard_within(&mut heard, "the hold").await;
        heard_within(&mut heard, "the release").await;
        drop(socket);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(heard.try_recv().is_err(), "nothing more after a clean release");
    }

    #[tokio::test]
    async fn a_burst_of_slider_values_runs_a_few_times_and_ends_on_the_last() {
        let (agent, url, mut heard) = deck_agent(json!([
            {"id": "level", "type": "WIDGET_TYPE_SLIDER", "col": 0, "row": 0, "w": 1, "h": 3,
             "slider": {"min": 0, "max": 100, "step": 1, "throttleMs": 50},
             "flows": [{"trigger": {"value": {}}, "blocks": [type_text(json!({"input": "value"}))]}]},
            {"id": "mute", "type": "WIDGET_TYPE_BUTTON", "col": 1, "row": 0, "w": 1, "h": 1,
             "flows": [{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [press_key("ctrl+m")]}]}
        ]))
        .await;
        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;

        let started = Instant::now();
        for value in 1..=100 {
            say(
                &mut socket,
                json!({"value": {"widgetId": "level", "value": f64::from(value) + 0.4, "seq": 1, "final": value == 100}}),
            )
            .await;
        }
        // A press in the middle of the drag is answered at once: the slider's
        // runs never sit in front of it.
        say(&mut socket, json!({"press": {"widgetId": "mute", "gesture": "GESTURE_SHORT_PRESS"}})).await;

        let mut typed = Vec::new();
        let mut pressed_after = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let (runs, pending) = agent.behavior().slider_runs("level");
            while let Ok(action) = heard.try_recv() {
                match action {
                    CoreAction::Text(text) => typed.push(text),
                    CoreAction::Hotkey(_) => pressed_after = Some(started.elapsed()),
                }
            }
            if !pending && pressed_after.is_some() && typed.len() as u64 == runs {
                break;
            }
            assert!(Instant::now() < deadline, "the slider never settled: {typed:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let elapsed = started.elapsed();
        let bound = (elapsed.as_millis() / 50) as usize + 2;
        assert!(
            typed.len() <= bound,
            "{} runs in {elapsed:?} is more than one per 50ms",
            typed.len()
        );
        assert!(typed.len() < 100, "coalesced, not one run per frame");
        assert_eq!(typed.last().map(String::as_str), Some("100"), "settled on a step, the last one sent");
        assert!(
            pressed_after.expect("pressed") < Duration::from_secs(2),
            "the press was not held up behind the drag"
        );
    }

    /// Wait for the device to stop being online, or give up.
    ///
    /// Subscribes to the announcement rather than polling, so this is not a
    /// sleep: it returns as soon as the state actually changes.
    async fn wait_until_offline(
        agent: &Arc<Agent>,
        device_id: &str,
        changes: &mut broadcast::Receiver<()>,
    ) {
        let offline = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                changes.recv().await.expect("a change is announced");
                if !agent.presence().is_online(device_id) {
                    break;
                }
            }
        })
        .await;

        assert!(offline.is_ok(), "{device_id} never went offline");
    }

    #[tokio::test]
    async fn a_connected_device_is_online_until_its_socket_closes() {
        // The whole point of presence: it must be true while the socket is open
        // and false afterwards, and it must not be true for a device that is
        // merely paired.
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let url = serve(Arc::clone(&agent)).await;

        let mut socket = connect(&url).await;
        assert!(
            !agent.presence().is_online("tab1"),
            "paired is not connected"
        );

        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;
        assert!(agent.presence().is_online("tab1"), "the socket is open");

        // Subscribe before closing so the closing announcement cannot be missed.
        let mut changes = agent.presence().changed();
        drop(socket);

        wait_until_offline(&agent, "tab1", &mut changes).await;
    }

    #[tokio::test]
    async fn revoking_a_connected_device_disconnects_its_socket() {
        // Revocation must not merely stop the *next* connection. A device that is
        // already connected has to stop being able to drive this machine, and has
        // to stop being reported as connected.
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let url = serve(Arc::clone(&agent)).await;

        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;
        assert!(
            agent.presence().is_online("tab1"),
            "connected to begin with"
        );

        // Subscribe first: the probe announces the drop the moment it happens.
        let mut changes = agent.presence().changed();
        agent.revoke_device("tab1").expect("revokes");

        wait_until_offline(&agent, "tab1", &mut changes).await;
    }

    #[tokio::test]
    async fn a_silent_socket_is_closed_once_it_stops_answering() {
        // The other half of the timer, and the path that had no test at all: a
        // connection that never ends with a close frame — a tablet that slept,
        // or lost Wi-Fi — must not sit there looking connected forever. Nothing
        // is sent after the hello, so once the liveness window passes the agent
        // has to drop the socket on its own.
        let agent = agent();
        agent
            .add_device("tab1", "Kitchen tablet", &hash_token("good-token"))
            .expect("device added");
        let url = serve(Arc::clone(&agent)).await;

        // Deliberately held, NOT dropped: dropping the socket would send a close
        // frame, and the connection would end because of that rather than
        // because the liveness check noticed the silence. The test would then
        // pass for the wrong reason.
        let mut socket = connect(&url).await;
        say(&mut socket, hello("good-token")).await;
        let _welcome = next_message(&mut socket).await;
        assert!(
            agent.presence().is_online("tab1"),
            "connected to begin with"
        );

        let mut changes = agent.presence().changed();
        wait_until_offline(&agent, "tab1", &mut changes).await;

        assert!(
            !agent.presence().is_online("tab1"),
            "a silent socket must not stay online"
        );
    }
}

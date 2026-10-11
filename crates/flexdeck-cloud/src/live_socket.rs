//! A browser's live view of a desktop, through this server.
//!
//! The desktop pushes what changed — a key's look, the page on screen, its
//! devices, its configuration — up its tunnel ([`crate::hub::Hub::push`]).
//! This socket carries those pushes on to a browser that is allowed to see
//! them, and to nobody else.
//!
//! # The desktop decides who watches
//!
//! A fingerprint is not a secret, so naming one cannot be enough. The browser's
//! first frame carries its token as well, and before a single push is passed
//! on, this server asks the desktop — `POST /api/live/watch`, relayed with that
//! token like any other request. The desktop answers with what the browser
//! should start from, and holds the tunnel's live frames open for a short
//! lease. The lease is renewed on a timer, with the same question: a browser
//! revoked at the desktop is refused at the next renewal, and its socket ends.
//! This server still never issues, stores or checks a credential itself.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use flexdeck_protocol::{server_message, ErrorEvent, ServerMessage};
use futures_util::SinkExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::cloud::Cloud;

/// The desktop's route that grants, and renews, a watch.
pub const WATCH_PATH: &str = "/api/live/watch";

/// How often the watch is renewed. Well inside the desktop's 30-second lease,
/// so one slow answer is not a gap — and the longest a revoked browser keeps
/// hearing.
const RENEW_EVERY: Duration = Duration::from_secs(10);

/// How long a socket has to say what it wants.
const HELLO: Duration = Duration::from_secs(10);

/// The browser only ever sends its hello.
const MAX_CLIENT_MESSAGE_BYTES: usize = 16 * 1024;

pub fn router(cloud: Arc<Cloud>) -> Router {
    Router::new().route("/api/live", get(upgrade)).with_state(cloud)
}

/// What a browser asks for: this desktop, with this credential.
#[derive(Debug, Deserialize)]
struct Hello {
    fingerprint: String,
    token: String,
}

async fn upgrade(
    State(cloud): State<Arc<Cloud>>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    socket: WebSocketUpgrade,
) -> Response {
    let address = crate::http::client_address(&cloud, &headers, connect.as_deref());
    socket
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .max_frame_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            // Each connection is a relayed request, counted like one.
            if !cloud.relay_limit().allow(address) {
                return refuse(socket, "too_many", "too many requests; slow down").await;
            }
            connection(cloud, socket).await
        })
}

async fn send(socket: &mut WebSocket, text: String) -> bool {
    socket.send(Message::text(text)).await.is_ok()
}

async fn send_message(socket: &mut WebSocket, message: &ServerMessage) -> bool {
    match serde_json::to_string(message) {
        Ok(text) => send(socket, text).await,
        Err(_) => true,
    }
}

/// End the socket with a contract-shaped reason: the same `ErrorEvent` the
/// desktop's own socket sends, so the portal reads one shape.
async fn refuse(mut socket: WebSocket, code: &str, reason: &str) {
    let message = ServerMessage {
        kind: Some(server_message::Kind::Error(ErrorEvent { code: code.to_string(), message: reason.to_string() })),
    };
    let _ = send_message(&mut socket, &message).await;
    let _ = socket.close().await;
}

async fn hello(socket: &mut WebSocket) -> Option<Hello> {
    loop {
        match socket.recv().await? {
            Ok(Message::Text(text)) => return serde_json::from_str(text.as_str()).ok(),
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            _ => return None,
        }
    }
}

/// Ask the desktop for a watch: the messages to start from, or why not.
async fn watch(cloud: &Cloud, hello: &Hello) -> Result<Vec<Value>, (&'static str, String)> {
    let response = cloud
        .hub()
        .request(hello.fingerprint.trim(), "POST", WATCH_PATH, String::new(), &hello.token)
        .await
        .map_err(|error| ("offline", error.to_string()))?;
    if response.status != 200 {
        let reason = serde_json::from_str::<Value>(&response.body)
            .ok()
            .and_then(|body| body["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("the desktop refused ({})", response.status));
        let code = if response.status == 401 || response.status == 403 { "unauthorised" } else { "refused" };
        return Err((code, reason));
    }
    let body: Value = serde_json::from_str(&response.body).map_err(|_| ("refused", "an unreadable answer".to_string()))?;
    Ok(body["messages"].as_array().cloned().unwrap_or_default())
}

async fn connection(cloud: Arc<Cloud>, mut socket: WebSocket) {
    let hello = match tokio::time::timeout(HELLO, hello(&mut socket)).await {
        Ok(Some(hello)) if !hello.token.trim().is_empty() => hello,
        _ => return refuse(socket, "bad_hello", "say which desktop, with a token").await,
    };

    // Listen first, then ask: a change landing in between arrives twice
    // rather than not at all.
    let Some(mut pushes) = cloud.hub().subscribe(hello.fingerprint.trim()) else {
        return refuse(socket, "offline", "that desktop is not connected").await;
    };
    let start = match watch(&cloud, &hello).await {
        Ok(start) => start,
        Err((code, reason)) => return refuse(socket, code, &reason).await,
    };
    for message in start {
        if !send(&mut socket, message.to_string()).await {
            return;
        }
    }

    let mut renew = tokio::time::interval_at(tokio::time::Instant::now() + RENEW_EVERY, RENEW_EVERY);
    loop {
        tokio::select! {
            push = pushes.recv() => match push {
                Ok(message) => {
                    if !send_message(&mut socket, &message).await {
                        return;
                    }
                }
                // Behind: start over from what the desktop says now.
                Err(RecvError::Lagged(_)) => match watch(&cloud, &hello).await {
                    Ok(start) => {
                        for message in start {
                            if !send(&mut socket, message.to_string()).await {
                                return;
                            }
                        }
                    }
                    Err((code, reason)) => return refuse(socket, code, &reason).await,
                },
                // The tunnel went away, or was replaced by a reconnect: the
                // browser reconnects, and finds whichever is there now.
                Err(RecvError::Closed) => return refuse(socket, "offline", "the desktop disconnected").await,
            },
            _ = renew.tick() => {
                if let Err((code, reason)) = watch(&cloud, &hello).await {
                    return refuse(socket, code, &reason).await;
                }
            }
            frame = socket.recv() => match frame {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use flexdeck_protocol::{tunnel_frame, StateEvent, TunnelResponse};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message as Frame;

    use super::*;

    type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    const DESKTOP: &str = "desk-a";

    async fn serve(cloud: &Arc<Cloud>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let address = listener.local_addr().expect("has an address");
        let app = router(Arc::clone(cloud));
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.expect("serves")
        });
        format!("ws://{address}/api/live")
    }

    /// A desktop on the hub that grants a watch to `good` alone, starting the
    /// browser off with one look.
    fn desktop(cloud: &Arc<Cloud>) {
        let mut frames = cloud.hub().attach(DESKTOP);
        let hub = Arc::clone(cloud);
        tokio::spawn(async move {
            while let Some(frame) = frames.recv().await {
                let Some(tunnel_frame::Kind::Request(request)) = frame.kind else { continue };
                let granted = request.path == WATCH_PATH && request.token == "good";
                let (status, body) = if granted {
                    (200, json!({"leaseSecs": 30, "messages": [{"state": {"widgetId": "cs2", "stateId": "off"}}]}).to_string())
                } else {
                    (401, json!({"error": "unknown or revoked token"}).to_string())
                };
                let _ = hub.hub().resolve(DESKTOP, TunnelResponse { id: request.id, status, body });
            }
        });
    }

    async fn open(url: &str, token: &str) -> Client {
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.expect("connects");
        let hello = json!({"fingerprint": DESKTOP, "token": token});
        socket.send(Frame::text(hello.to_string())).await.expect("says hello");
        socket
    }

    async fn next(socket: &mut Client) -> Value {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .expect("a frame in time")
                .expect("open")
                .expect("not a transport error");
            if let Frame::Text(text) = frame {
                return serde_json::from_str(text.as_str()).expect("json");
            }
        }
    }

    #[tokio::test]
    async fn a_browser_the_desktop_allows_hears_its_pushes() {
        let cloud = Arc::new(Cloud::new());
        desktop(&cloud);
        let mut socket = open(&serve(&cloud).await, "good").await;

        assert_eq!(next(&mut socket).await["state"]["stateId"], "off", "starts where the desktop says");

        cloud.hub().push(
            DESKTOP,
            ServerMessage {
                kind: Some(server_message::Kind::State(StateEvent {
                    widget_id: "cs2".into(),
                    state_id: "on".into(),
                    ..Default::default()
                })),
            },
        );
        let pushed = next(&mut socket).await;
        assert_eq!((pushed["state"]["widgetId"].as_str(), pushed["state"]["stateId"].as_str()), (Some("cs2"), Some("on")));
    }

    #[tokio::test]
    async fn a_browser_the_desktop_refuses_hears_nothing() {
        let cloud = Arc::new(Cloud::new());
        desktop(&cloud);
        let mut socket = open(&serve(&cloud).await, "stolen").await;
        let refused = next(&mut socket).await;
        assert_eq!(refused["error"]["code"], "unauthorised", "{refused}");
    }

    #[tokio::test]
    async fn a_desktop_that_is_not_connected_is_said_so() {
        let cloud = Arc::new(Cloud::new());
        let mut socket = open(&serve(&cloud).await, "good").await;
        assert_eq!(next(&mut socket).await["error"]["code"], "offline");
    }
}

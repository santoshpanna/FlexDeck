//! The same-PC door: how the portal, open in a browser on THIS computer,
//! reaches the desktop app with no sign-in.
//!
//! The website is served from the cloud, but a browser on this machine can talk
//! to `http://127.0.0.1` directly — the way a keyboard vendor's web configurator
//! talks to the keyboard on your desk. That is a door any website could knock
//! on, so it is narrow:
//!
//! * **Loopback only.** Bound to `127.0.0.1`, never the LAN.
//! * **Named host only.** A request whose `Host` is not `127.0.0.1:<port>` or
//!   `localhost:<port>` is refused, which defeats DNS rebinding: a hostile page
//!   that re-points its own domain at 127.0.0.1 still sends its own domain as
//!   the `Host`.
//! * **Allowed origins only.** A browser always says which page is calling. Only
//!   the flexdeck portal (and, in a debug build, its dev server) is answered;
//!   any other page gets a refusal, not a response it could read.
//! * **Its own credential.** `/api/local/connect` issues a *local* browser token,
//!   honoured on this listener and nowhere else (see [`crate::access`]). Stolen
//!   and replayed through the cloud, it is worthless.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use flexdeck_store::DeviceKind;
use serde::Deserialize;
use serde_json::json;

use crate::access::Channel;
use crate::agent::Agent;
use crate::http::ApiError;

/// What the loopback door needs to know.
#[derive(Debug, Clone)]
pub struct LoopbackConfig {
    /// The port it listens on, as the `Host` header must name it.
    pub port: u16,
    /// Exact origins (`scheme://host[:port]`) whose pages may call in.
    pub allowed_origins: Vec<String>,
    /// This desktop's label, for the portal to show.
    pub name: String,
}

/// The origins allowed to call the loopback door, given the portal address.
///
/// The portal's tunnel URL (`wss://portal.example/tunnel`) names the site the
/// portal is served from (`https://portal.example`). A debug build also allows
/// the portal's dev server, so `pnpm dev:web` works against a local agent.
pub fn allowed_origins(cloud_url: Option<&str>) -> Vec<String> {
    let mut origins = Vec::new();

    if let Some(url) = cloud_url {
        let (scheme, rest) = match url.split_once("://") {
            Some(("wss", rest)) => ("https", rest),
            Some(("ws", rest)) => ("http", rest),
            _ => ("", ""),
        };
        let authority = rest.split('/').next().unwrap_or_default();
        if !scheme.is_empty() && !authority.is_empty() {
            origins.push(format!("{scheme}://{authority}"));
        }
    }

    if cfg!(debug_assertions) {
        for dev in ["http://localhost:5173", "http://127.0.0.1:5173"] {
            origins.push(dev.to_string());
        }
    }

    origins
}

struct Door {
    agent: Arc<Agent>,
    config: LoopbackConfig,
}

/// The loopback router: `api` behind the door's checks, plus the two routes
/// that let the portal find this desktop and connect to it.
pub fn router(agent: Arc<Agent>, api: Router, config: LoopbackConfig) -> Router {
    let door = Arc::new(Door { agent, config });

    Router::new()
        .route("/api/local/hello", get(hello))
        .route("/api/local/connect", post(connect))
        .with_state(Arc::clone(&door))
        .merge(api)
        .layer(middleware::from_fn_with_state(door, guard))
}

/// Host check, origin check, CORS, and the channel mark, for every request.
async fn guard(State(door): State<Arc<Door>>, mut request: Request, next: Next) -> Response {
    if !host_is_loopback(request.headers(), door.config.port) {
        return refuse(
            StatusCode::MISDIRECTED_REQUEST,
            "this listener only answers to 127.0.0.1 and localhost",
        );
    }

    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let allowed = match &origin {
        // Not a browser page (or a same-origin request): no cross-site risk,
        // and the routes still need a credential. `connect` checks for itself.
        None => None,
        Some(origin) if door.config.allowed_origins.iter().any(|o| o == origin) => {
            Some(origin.clone())
        }
        Some(_) => {
            return refuse(
                StatusCode::FORBIDDEN,
                "this page is not allowed to reach the desktop app",
            )
        }
    };

    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        cors(response.headers_mut(), allowed.as_deref(), true);
        return response;
    }

    request.extensions_mut().insert(Channel::Loopback);
    let mut response = next.run(request).await;
    cors(response.headers_mut(), allowed.as_deref(), false);
    response
}

fn host_is_loopback(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// Let an allowed page read the response — and, for a preflight, say which
/// methods and headers it may use, including Chrome's private-network opt-in.
fn cors(headers: &mut HeaderMap, origin: Option<&str>, preflight: bool) {
    let Some(origin) = origin.and_then(|o| HeaderValue::from_str(o).ok()) else {
        return;
    };
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    if preflight {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PUT"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("authorization, content-type"),
        );
        headers.insert(
            header::HeaderName::from_static("access-control-allow-private-network"),
            HeaderValue::from_static("true"),
        );
        headers.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
    }
}

fn refuse(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// Is a desktop app here, and which one? Needs no credential: it is how the
/// portal notices this computer has the app, and it says nothing secret.
async fn hello(State(door): State<Arc<Door>>) -> Json<serde_json::Value> {
    Json(json!({
        "fingerprint": door.agent.identity().fingerprint(),
        "name": door.config.name,
    }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectRequest {
    #[serde(default)]
    name: String,
    /// The browser's own id, so connecting again replaces its old row rather
    /// than adding one per visit.
    #[serde(default)]
    client_id: String,
}

/// Issue this browser a local credential.
///
/// Only for a page from an allowed origin — the guard has already refused any
/// other origin, and this refuses a request with none, so a local program that
/// is not a browser cannot quietly mint one. (Such a program could read the
/// agent's files anyway; this keeps the door honest about who it is for.)
async fn connect(
    State(door): State<Arc<Door>>,
    headers: HeaderMap,
    body: Option<Json<ConnectRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if headers.get(header::ORIGIN).is_none() {
        return Err(ApiError::forbidden(
            "connecting is for the flexdeck website, open in a browser on this computer",
        ));
    }

    let request = body.map(|Json(body)| body).unwrap_or_default();
    let name = match request.name.trim() {
        "" => "Browser on this computer",
        trimmed => trimmed,
    };
    let (token, device) = door
        .agent
        .issue_credential_for(name, DeviceKind::BrowserLocal, Some(&request.client_id))?;

    Ok(Json(json!({
        "token": token,
        "deviceId": device.id,
        "fingerprint": door.agent.identity().fingerprint(),
        "name": door.config.name,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http;
    use axum::body::Body;
    use flexdeck_pairing::Identity;
    use flexdeck_store::ConfigStore;
    use serde_json::Value;
    use tower::ServiceExt;

    const PORT: u16 = 7879;
    const PORTAL: &str = "https://portal.example";

    fn door() -> (Arc<Agent>, Router) {
        let store = ConfigStore::open_in_memory().expect("store");
        store.commit(crate::agent::DEFAULT_CONFIG).expect("commits");
        let identity = Identity::generate(vec!["localhost".into()]).expect("identity");
        let agent = Arc::new(Agent::new(store, identity, "127.0.0.1".into(), 7878));
        let app = router(
            Arc::clone(&agent),
            http::router(Arc::clone(&agent)),
            LoopbackConfig {
                port: PORT,
                allowed_origins: allowed_origins(Some("wss://portal.example/tunnel")),
                name: "Studio PC".into(),
            },
        );
        (agent, app)
    }

    async fn send(
        app: &Router,
        method: &str,
        uri: &str,
        host: &str,
        origin: Option<&str>,
        token: Option<&str>,
    ) -> (StatusCode, HeaderMap, Value) {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("host", host)
            .header("content-type", "application/json");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let body = if method == "GET" || method == "OPTIONS" {
            Body::empty()
        } else {
            Body::from("{}")
        };
        let response = app
            .clone()
            .oneshot(request.body(body).expect("builds"))
            .await
            .expect("responds");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reads");
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    const HOST: &str = "127.0.0.1:7879";

    #[test]
    fn the_portal_origin_follows_the_tunnel_address() {
        let origins = allowed_origins(Some("wss://portal.example:8443/tunnel"));
        assert_eq!(origins[0], "https://portal.example:8443");
        assert!(allowed_origins(Some("not a url"))
            .iter()
            .all(|origin| origin.contains("5173")));
    }

    #[tokio::test]
    async fn the_portal_on_this_computer_connects_with_no_sign_in() {
        let (_, app) = door();

        let (status, headers, hello) =
            send(&app, "GET", "/api/local/hello", HOST, Some(PORTAL), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(hello["name"], "Studio PC");
        assert_eq!(
            headers.get("access-control-allow-origin").unwrap(),
            PORTAL,
            "the portal may read the answer"
        );

        let (status, _, connected) =
            send(&app, "POST", "/api/local/connect", HOST, Some(PORTAL), None).await;
        assert_eq!(status, StatusCode::OK, "{connected}");
        let token = connected["token"].as_str().expect("a token");

        // And the token is a configurator's, here.
        let (status, _, _) =
            send(&app, "PUT", "/api/config", HOST, Some(PORTAL), Some(token)).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED);
        assert_ne!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn any_other_website_is_turned_away() {
        let (_, app) = door();
        for uri in ["/api/local/hello", "/api/local/connect"] {
            let method = if uri.ends_with("hello") {
                "GET"
            } else {
                "POST"
            };
            let (status, headers, _) =
                send(&app, method, uri, HOST, Some("https://evil.example"), None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
            assert!(headers.get("access-control-allow-origin").is_none());
        }
    }

    #[tokio::test]
    async fn a_rebound_hostname_is_refused() {
        // DNS rebinding: evil.example now resolves to 127.0.0.1, but the browser
        // still sends evil.example as the Host.
        let (_, app) = door();
        let (status, _, _) = send(
            &app,
            "GET",
            "/api/local/hello",
            "evil.example:7879",
            Some(PORTAL),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    }

    #[tokio::test]
    async fn a_private_network_preflight_is_answered_for_the_portal() {
        let (_, app) = door();
        let (status, headers, _) =
            send(&app, "OPTIONS", "/api/config", HOST, Some(PORTAL), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            headers.get("access-control-allow-private-network").unwrap(),
            "true"
        );
        assert!(headers
            .get("access-control-allow-headers")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("authorization"));
    }

    #[tokio::test]
    async fn a_program_that_is_not_a_browser_cannot_mint_a_token() {
        let (_, app) = door();
        let (status, _, _) = send(&app, "POST", "/api/local/connect", HOST, None, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_remote_browsers_token_does_not_work_here() {
        let (agent, app) = door();
        let (remote, _) = agent
            .issue_credential("Laptop", DeviceKind::BrowserRemote)
            .expect("issues");
        let (status, _, _) = send(
            &app,
            "GET",
            "/api/status",
            HOST,
            Some(PORTAL),
            Some(&remote),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

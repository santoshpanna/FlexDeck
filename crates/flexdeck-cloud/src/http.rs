//! The portal's own HTTP surface: what the web UI talks to.
//!
//! # There are no accounts
//!
//! A browser reaches a desktop by having paired with it: the person typed a code
//! shown on that desktop's screen, and the desktop issued the browser a token.
//! The cloud never issues, stores or checks credentials. [`relay`] forwards the
//! browser's own token down the desktop's tunnel, and the **desktop** decides —
//! so a request this server forwards can never be more powerful than what the
//! browser itself was given.
//!
//! What the cloud does own is *which* tunnel a fingerprint names (proved by
//! signature at connect, see [`crate::tunnel_socket`]) and the rate limits that
//! make a 40-bit pairing code unguessable from here.
//!
//! # This surface is hand-built JSON, not the proto3 mapping
//!
//! The tunnel speaks the proto3 JSON mapping, where 64-bit integers travel as
//! strings. This surface is assembled with `serde_json::json!`, like the agent's
//! own REST API, so a number is a number. The shape of an error matches the
//! agent's: a JSON object with an `error` key.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::connect_info::ConnectInfo;
use axum::extract::{Extension, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::cloud::{now_secs, Cloud};
use crate::hub::RelayError;

/// The largest request body this surface reads. A relayed configuration is a
/// few kilobytes; the agent itself refuses anything over 1 MiB.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// The route on the desktop that trades a code for a browser credential.
const REDEEM_PATH: &str = "/api/pairing/browser/redeem";

/// The portal's routes.
pub fn router(cloud: Arc<Cloud>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/relay", post(relay))
        .route("/api/pair", post(pair))
        .route("/api/presence", post(presence))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .with_state(cloud)
}

/// Whether this deployment is up, and how much it is carrying. Unauthenticated
/// for a readiness probe, and it names no desktop.
async fn health(State(cloud): State<Arc<Cloud>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "tunnels": cloud.hub().online_count(),
    }))
}

/// The address a request came from, for rate limiting.
///
/// `X-Forwarded-For` only when the operator said a proxy sets it: believed by
/// default, it would let any client claim any address and walk past the limits.
pub(crate) fn client_address(
    cloud: &Cloud,
    headers: &HeaderMap,
    connect: Option<&ConnectInfo<SocketAddr>>,
) -> IpAddr {
    if cloud.trusts_proxy() {
        let forwarded = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .and_then(|first| first.trim().parse().ok());
        if let Some(address) = forwarded {
            return address;
        }
    }
    connect
        .map(|ConnectInfo(address)| address.ip())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// The caller's bearer token, if it sent one.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let (scheme, token) = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|token| !token.is_empty())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayRequest {
    /// The desktop, by the fingerprint the browser learned when it paired.
    fingerprint: String,
    method: String,
    path: String,
    /// The JSON body as text, exactly as the desktop's API expects it.
    #[serde(default)]
    body: String,
}

/// Forward one call to one desktop and return its answer.
///
/// The browser's `Authorization` header is the only credential involved, and
/// it travels to the desktop untouched. The desktop's answer comes back
/// untouched too — status included — so the browser sees exactly what the
/// desktop decided, its 401s and 403s among it.
async fn relay(
    State(cloud): State<Arc<Cloud>>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<RelayRequest>,
) -> Result<Json<Value>, ApiError> {
    let address = client_address(&cloud, &headers, connect.as_deref());
    if !cloud.relay_limit().allow(address) {
        return Err(ApiError::too_many("too many requests; slow down"));
    }

    // A request with no credential could only reach the code-redeem route, and
    // that has its own, much tighter, door: `/api/pair`.
    let Some(token) = bearer(&headers) else {
        return Err(ApiError::unauthorized(
            "an 'Authorization: Bearer <token>' header is required",
        ));
    };

    let response = cloud
        .hub()
        .request(
            request.fingerprint.trim(),
            &request.method,
            &request.path,
            request.body,
            token,
        )
        .await?;

    // `body` stays a JSON *string*: the portal already knows how to read the
    // agent's responses, and re-encoding here would silently drop anything this
    // server does not understand.
    Ok(Json(json!({
        "status": response.status,
        "body": response.body,
    })))
}

#[derive(Debug, Deserialize)]
struct PairRequest {
    /// What the person typed, as they typed it.
    code: String,
    /// What to call this browser in the desktop's device list.
    #[serde(default)]
    name: String,
    /// The browser's own id, passed through so the desktop replaces an earlier
    /// pairing from the same browser instead of listing it twice.
    #[serde(default, rename = "clientId")]
    client_id: String,
}

/// Pair this browser with whichever desktop is showing `code`.
///
/// Rate-limited per address: a code is 40 bits and the desktop allows five
/// guesses per code, so with this limit in front, guessing is not a strategy.
async fn pair(
    State(cloud): State<Arc<Cloud>>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<PairRequest>,
) -> Result<Json<Value>, ApiError> {
    let address = client_address(&cloud, &headers, connect.as_deref());
    if !cloud.pair_limit().allow(address) {
        return Err(ApiError::too_many(
            "too many attempts from here; wait a minute and try again",
        ));
    }

    let code = flexdeck_pairing::normalise_code(&request.code);
    if code.is_empty() {
        return Err(ApiError::bad_request("type the code your desktop shows"));
    }

    // The desktop filed the HASH of its code; this is the same hash, computed
    // from what was typed. The cloud never held the code itself.
    let hash = flexdeck_pairing::hash_token(&code);
    let fingerprint = cloud
        .offers()
        .route(&hash, now_secs())
        .ok_or_else(|| ApiError::not_found("no desktop is showing that code right now"))?;

    let body = json!({
        "code": code,
        "name": request.name.trim(),
        "clientId": request.client_id.trim(),
    })
    .to_string();
    let response = cloud
        .hub()
        .request(&fingerprint, "POST", REDEEM_PATH, body, "")
        .await?;

    let mut answer: Value = serde_json::from_str(&response.body)
        .map_err(|_| ApiError::bad_gateway("the desktop's answer could not be read"))?;
    if response.status != 200 {
        let status = StatusCode::from_u16(response.status as u16).unwrap_or(StatusCode::FORBIDDEN);
        let message = answer["error"]
            .as_str()
            .unwrap_or("the desktop refused")
            .to_string();
        return Err(ApiError { status, message });
    }

    // What the browser keeps: the desktop's identity, its label, and the token.
    answer["fingerprint"] = Value::String(fingerprint.clone());
    answer["name"] = cloud
        .hub()
        .name_of(&fingerprint)
        .map(Value::String)
        .unwrap_or(Value::Null);
    Ok(Json(answer))
}

#[derive(Debug, Deserialize)]
struct PresenceRequest {
    #[serde(default)]
    fingerprints: Vec<String>,
}

/// Which of the desktops this browser has paired with are connected right now.
///
/// Answers only for fingerprints the caller already knows, so it lists nothing.
async fn presence(
    State(cloud): State<Arc<Cloud>>,
    Json(request): Json<PresenceRequest>,
) -> Json<Value> {
    let desktops: Vec<Value> = request
        .fingerprints
        .iter()
        .take(64)
        .map(|fingerprint| {
            json!({
                "fingerprint": fingerprint,
                "online": cloud.hub().is_online(fingerprint),
                "name": cloud.hub().name_of(fingerprint),
            })
        })
        .collect();
    Json(json!({ "desktops": desktops }))
}

// -- security headers --------------------------------------------------------

/// Every response's protective headers, for the portal and the API alike.
///
/// The CSP allows exactly the portal's own scripts plus the one inline script
/// `index.html` carries (by hash), and lets the page reach the desktop app on
/// this computer's loopback — the same-PC connection — and nothing else.
pub fn with_security_headers(router: Router, index_html: Option<&str>) -> Router {
    let csp = content_security_policy(index_html);
    let fixed = [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
    ];

    let mut router = router.layer(SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&csp).expect("a CSP is plain ASCII"),
    ));
    for (name, value) in fixed {
        router = router.layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        ));
    }
    router
}

/// The CSP for a portal whose `index.html` is `index_html`.
pub fn content_security_policy(index_html: Option<&str>) -> String {
    let hashes: String = index_html
        .map(inline_scripts)
        .unwrap_or_default()
        .iter()
        .map(|script| format!(" 'sha256-{}'", script_hash(script)))
        .collect();

    format!(
        "default-src 'self'; script-src 'self'{hashes}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; connect-src 'self' http://127.0.0.1:* http://localhost:* ws://127.0.0.1:* ws://localhost:*; \
         object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
    )
}

/// The bodies of `<script>` elements that have no `src`.
fn inline_scripts(html: &str) -> Vec<&str> {
    let mut scripts = Vec::new();
    let mut rest = html;
    while let Some(open) = rest.find("<script") {
        let after_open = &rest[open..];
        let Some(tag_end) = after_open.find('>') else {
            break;
        };
        let tag = &after_open[..tag_end];
        let body_start = &after_open[tag_end + 1..];
        let Some(close) = body_start.find("</script>") else {
            break;
        };
        if !tag.contains("src=") {
            scripts.push(&body_start[..close]);
        }
        rest = &body_start[close..];
    }
    scripts
}

fn script_hash(script: &str) -> String {
    use base64::Engine as _;
    use sha2::{Digest, Sha256};
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(script.as_bytes()))
}

// -- errors ------------------------------------------------------------------

/// A JSON error body, so a client never has to guess a content type.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn too_many(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
        }
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

/// A desktop that is offline, or that did not answer, is this server's problem
/// to report honestly rather than a 200 with nothing in it.
impl From<RelayError> for ApiError {
    fn from(error: RelayError) -> Self {
        match error {
            RelayError::Offline => Self::bad_gateway("that desktop is not connected"),
            RelayError::Closed => Self::bad_gateway("that desktop's connection dropped"),
            RelayError::Timeout => Self::bad_gateway("that desktop did not answer in time"),
            RelayError::TooManyInFlight { .. } => {
                Self::bad_gateway("that desktop is already busy with other requests")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tunnel_socket;
    use axum::body::Body;
    use axum::http::Request;
    use flexdeck_pairing::Identity;
    use flexdeck_protocol::{tunnel_frame, PairOffer, TunnelFrame, TunnelResponse};
    use futures_util::StreamExt;
    use std::time::Duration;
    use tower::ServiceExt;

    async fn call(
        cloud: &Arc<Cloud>,
        uri: &str,
        body: Value,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router(Arc::clone(cloud))
            .oneshot(request.body(Body::from(body.to_string())).expect("builds"))
            .await
            .expect("responds");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reads");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A desktop on a real, proven tunnel that answers every request with
    /// `answer`, records what it was asked, and has offered the code `7KQ4-M9XR`.
    async fn desktop(
        cloud: &Arc<Cloud>,
        answer: impl Fn(&flexdeck_protocol::TunnelRequest) -> (u32, String) + Send + 'static,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<flexdeck_protocol::TunnelRequest>,
    ) {
        use futures_util::SinkExt;

        let url = tunnel_socket::tests::serve(Arc::clone(cloud)).await;
        let identity = Identity::generate(vec!["localhost".into()]).expect("identity");
        let (mut client, _) = tunnel_socket::tests::connect_as(&url, &identity).await;

        // Offer a code through the same path a real desktop uses.
        let hash = flexdeck_pairing::hash_token(&flexdeck_pairing::normalise_code("7KQ4-M9XR"));
        tunnel_socket::tests::say(
            &mut client,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::PairOffer(PairOffer {
                    code_hash: hash,
                    expires_at: now_secs() + 300,
                })),
            },
        )
        .await;

        let (mut sink, mut stream) = client.split();
        let (seen, requests) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                let Ok(text) = message.to_text() else {
                    continue;
                };
                let Ok(frame) = flexdeck_tunnel::decode(text) else {
                    continue;
                };
                if let Some(tunnel_frame::Kind::Request(request)) = frame.kind {
                    let (status, body) = answer(&request);
                    let _ = seen.send(request.clone());
                    let reply = TunnelFrame {
                        kind: Some(tunnel_frame::Kind::Response(TunnelResponse {
                            id: request.id,
                            status,
                            body,
                        })),
                    };
                    let text = flexdeck_tunnel::encode(&reply).expect("encodes");
                    let message = tokio_tungstenite::tungstenite::Message::text(text);
                    if sink.send(message).await.is_err() {
                        break;
                    }
                }
            }
        });

        let fingerprint = identity.fingerprint();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !cloud.hub().is_online(&fingerprint) || cloud.offers().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the desktop never settled"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        (fingerprint, requests)
    }

    #[tokio::test]
    async fn the_account_routes_are_gone() {
        let cloud = Arc::new(Cloud::new());
        for uri in [
            "/api/session",
            "/api/desktops",
            "/api/link/redeem",
            "/api/link/open",
        ] {
            let (status, _) = call(&cloud, uri, json!({}), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri} still exists");
        }
    }

    #[tokio::test]
    async fn a_relay_carries_the_browsers_own_token_and_nothing_else() {
        let cloud = Arc::new(Cloud::new());
        let (fingerprint, mut seen) = desktop(&cloud, |_| (200, r#"{"rev":3}"#.to_string())).await;

        let (status, body) = call(
            &cloud,
            "/api/relay",
            json!({"fingerprint": fingerprint, "method": "GET", "path": "/api/status"}),
            Some("browser-token"),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], 200);
        let request = seen.recv().await.expect("the desktop was asked");
        assert_eq!(request.token, "browser-token");
    }

    #[tokio::test]
    async fn a_relay_without_a_token_is_refused_before_it_reaches_a_desktop() {
        let cloud = Arc::new(Cloud::new());
        let (fingerprint, mut seen) = desktop(&cloud, |_| (200, "{}".to_string())).await;

        let (status, _) = call(
            &cloud,
            "/api/relay",
            json!({"fingerprint": fingerprint, "method": "GET", "path": "/api/status"}),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(seen.try_recv().is_err(), "nothing was forwarded");
    }

    #[tokio::test]
    async fn relaying_to_an_offline_desktop_is_a_bad_gateway() {
        let cloud = Arc::new(Cloud::new());
        let (status, _) = call(
            &cloud,
            "/api/relay",
            json!({"fingerprint": "nobody", "method": "GET", "path": "/api/status"}),
            Some("t"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn a_typed_code_reaches_the_desktop_showing_it() {
        let cloud = Arc::new(Cloud::new());
        let (fingerprint, mut seen) = desktop(&cloud, |_| {
            (200, r#"{"token":"issued","deviceId":"d"}"#.to_string())
        })
        .await;

        let (status, body) = call(
            &cloud,
            "/api/pair",
            json!({"code": "7kq4 m9xr", "name": "Laptop"}),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["token"], "issued");
        assert_eq!(body["fingerprint"], fingerprint);

        let request = seen.recv().await.expect("the desktop was asked");
        assert_eq!(request.path, REDEEM_PATH);
        assert_eq!(request.token, "", "the cloud adds no credential");
        let sent: Value = serde_json::from_str(&request.body).expect("JSON");
        assert_eq!(
            sent["code"], "7KQ4M9XR",
            "normalised for the desktop to check"
        );
    }

    #[tokio::test]
    async fn a_desktops_refusal_comes_back_as_a_refusal() {
        let cloud = Arc::new(Cloud::new());
        let (_fingerprint, _seen) = desktop(&cloud, |_| {
            (403, r#"{"error":"that code is not right"}"#.to_string())
        })
        .await;

        let (status, body) = call(&cloud, "/api/pair", json!({"code": "7KQ4-M9XR"}), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "that code is not right");
    }

    #[tokio::test]
    async fn an_unknown_code_never_reaches_any_desktop() {
        let cloud = Arc::new(Cloud::new());
        let (_fingerprint, mut seen) = desktop(&cloud, |_| (200, "{}".to_string())).await;

        let (status, _) = call(&cloud, "/api/pair", json!({"code": "AAAA-AAAA"}), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(seen.try_recv().is_err());
    }

    #[tokio::test]
    async fn guessing_codes_is_rate_limited() {
        let cloud = Arc::new(Cloud::new());
        let mut statuses = Vec::new();
        for _ in 0..(crate::cloud::PAIR_ATTEMPTS_PER_MINUTE + 1) {
            let (status, _) = call(&cloud, "/api/pair", json!({"code": "ZZZZ-ZZZZ"}), None).await;
            statuses.push(status);
        }
        assert_eq!(statuses.last(), Some(&StatusCode::TOO_MANY_REQUESTS));
        assert!(statuses[..statuses.len() - 1]
            .iter()
            .all(|status| *status == StatusCode::NOT_FOUND));
    }

    #[tokio::test]
    async fn presence_answers_only_for_the_fingerprints_asked_about() {
        let cloud = Arc::new(Cloud::new());
        let (fingerprint, _seen) = desktop(&cloud, |_| (200, "{}".to_string())).await;

        let (status, body) = call(
            &cloud,
            "/api/presence",
            json!({"fingerprints": [fingerprint, "unknown"]}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["desktops"][0]["online"], true);
        assert_eq!(body["desktops"][0]["name"], "Studio PC");
        assert_eq!(body["desktops"][1]["online"], false);
        assert_eq!(body["desktops"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn the_csp_allows_exactly_the_inline_script_index_html_carries() {
        let html = "<head><script>console.log(1)</script>\
                    <script type=\"module\" src=\"/a.js\"></script></head>";
        let csp = content_security_policy(Some(html));

        assert!(csp.contains(&format!("'sha256-{}'", script_hash("console.log(1)"))));
        assert_eq!(
            csp.matches("'sha256-").count(),
            1,
            "the src= script needs no hash"
        );
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.contains("connect-src 'self' http://127.0.0.1:*"));
    }

    #[tokio::test]
    async fn every_response_carries_the_protective_headers() {
        let cloud = Arc::new(Cloud::new());
        let app = with_security_headers(router(cloud), None);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("builds"),
            )
            .await
            .expect("responds");

        for name in [
            "content-security-policy",
            "x-content-type-options",
            "x-frame-options",
            "referrer-policy",
        ] {
            assert!(response.headers().contains_key(name), "{name} is missing");
        }
    }
}

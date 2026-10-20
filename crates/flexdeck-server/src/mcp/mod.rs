//! A Model Context Protocol endpoint, so a coding agent on this computer can
//! edit the deck. Stateless Streamable HTTP: one JSON-RPC message (or a batch)
//! per POST, answered with JSON, no event stream and no session.
//!
//! Mounted on the loopback door only, behind its Host and Origin checks, and
//! answering to an agent token (Setup → Coding agents) or the admin token.

pub mod edit;
mod tools;

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::access::Authority;
use crate::agent::Agent;

pub const BODY_LIMIT: usize = 8 * 1024 * 1024;
const SUPPORTED: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
const PREFERRED: &str = "2025-06-18";
const DEFAULT_VERSION: &str = "2025-03-26";
const INSTRUCTIONS: &str = "FlexDeck is a stream deck: a grid of keys on a tablet that run actions on this computer. \
These tools are the user's live deck: use them for any question about their FlexDeck widgets, keys, buttons, pages or automations, \
never FlexDeck's files or database, which the running app owns. \
Call deck_overview first. Every write takes the rev you last read and returns the new one; if the deck changed, re-read and retry. \
Call get_guide before building a widget, and list_actions / list_choices for what a key can do. \
Tool names are stable; changes are additive.";

pub fn router(agent: Arc<Agent>) -> Router {
    Router::new()
        .route("/mcp", post(handle).get(not_allowed).delete(not_allowed))
        .with_state(agent)
}

async fn not_allowed() -> Response {
    let mut response = (StatusCode::METHOD_NOT_ALLOWED, "this endpoint takes POST only").into_response();
    response.headers_mut().insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}

fn refusal(status: StatusCode, message: &str) -> Response {
    let mut response = (status, Json(json!({ "error": message }))).into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

const NEEDS_AGENT_TOKEN: &str =
    "this endpoint needs a coding-agent token: create one in the FlexDeck app under Setup → Coding agents";

async fn handle(State(agent): State<Arc<Agent>>, request: Request) -> Response {
    let authority = match crate::http::authority_of(&agent, &request) {
        Ok(authority @ (Authority::Agent(_) | Authority::Admin)) => authority,
        Ok(_) => return refusal(StatusCode::FORBIDDEN, NEEDS_AGENT_TOKEN),
        Err(error) if error.status() == StatusCode::UNAUTHORIZED => {
            return refusal(StatusCode::UNAUTHORIZED, &format!("{}; {NEEDS_AGENT_TOKEN}", error.message()))
        }
        Err(error) => return error.into_response(),
    };

    let version = request
        .headers()
        .get("mcp-protocol-version")
        .map(|value| value.to_str().unwrap_or_default().trim().to_string());
    if let Some(version) = &version {
        if !SUPPORTED.contains(&version.as_str()) {
            return refusal(StatusCode::BAD_REQUEST, &format!("unsupported MCP-Protocol-Version {version}"));
        }
    }

    let bytes = match axum::body::to_bytes(request.into_body(), BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => return refusal(StatusCode::PAYLOAD_TOO_LARGE, "the request is larger than 8 MB"),
    };
    let message: Value = match serde_json::from_slice(&bytes) {
        Ok(message) => message,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(failure(Value::Null, -32700, &format!("parse error: {error}"))))
                .into_response()
        }
    };

    match message {
        Value::Array(batch) if version.as_deref().unwrap_or(DEFAULT_VERSION) == DEFAULT_VERSION => {
            if batch.is_empty() {
                return Json(failure(Value::Null, -32600, "an empty batch")).into_response();
            }
            let mut replies = Vec::new();
            for message in &batch {
                if let Some(reply) = answer(&agent, &authority, message).await {
                    replies.push(reply);
                }
            }
            if replies.is_empty() {
                accepted()
            } else {
                Json(Value::Array(replies)).into_response()
            }
        }
        Value::Array(_) => Json(failure(Value::Null, -32600, "batches need protocol version 2025-03-26")).into_response(),
        message => match answer(&agent, &authority, &message).await {
            Some(reply) => Json(reply).into_response(),
            None => accepted(),
        },
    }
}

fn accepted() -> Response {
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::empty())
        .unwrap_or_default()
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

async fn answer(agent: &Agent, authority: &Authority, message: &Value) -> Option<Value> {
    let Some(object) = message.as_object() else {
        return Some(failure(Value::Null, -32600, "a message must be a JSON object"));
    };
    let id = object.get("id")?.clone();
    if !matches!(id, Value::String(_) | Value::Number(_)) {
        return Some(failure(Value::Null, -32600, "id must be a string or a number"));
    }
    let method = object.get("method").and_then(Value::as_str)?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(failure(id, -32600, "jsonrpc must be \"2.0\""));
    }
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = if SUPPORTED.contains(&asked) { asked } else { PREFERRED };
            success(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "flexdeck", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({"tools": tools::list()})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(failure(id, -32602, "params.name is required"));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            if !args.is_object() {
                return Some(failure(id, -32602, "params.arguments must be an object"));
            }
            let logged: String = name.chars().take(64).collect();
            tracing::info!(tool = %logged, device = %authority.requester(), "mcp tool call");
            match tools::call(agent, authority, name, &args).await {
                Ok(value) => success(id, json!({"content": [{"type": "text", "text": text_of(value)}]})),
                Err(tools::Failure::UnknownTool) => failure(id, -32602, &format!("unknown tool {name}")),
                Err(tools::Failure::Message(message)) => {
                    success(id, json!({"content": [{"type": "text", "text": message}], "isError": true}))
                }
            }
        }
        other => failure(id, -32601, &format!("method not found: {other}")),
    })
}

fn text_of(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::{self, LoopbackConfig};
    use flexdeck_pairing::Identity;
    use flexdeck_store::{ConfigStore, DeviceKind};
    use tower::ServiceExt;

    const PORT: u16 = 7879;
    const HOST: &str = "127.0.0.1:7879";

    fn door() -> (Arc<Agent>, Router) {
        let store = ConfigStore::open_in_memory().expect("store");
        let identity = Identity::generate(vec!["localhost".into()]).expect("identity");
        let agent = Arc::new(Agent::new(store, identity, "127.0.0.1".into(), 7878));
        agent.replace_config(crate::agent::DEFAULT_CONFIG).expect("seeds");
        let api = crate::http::router(Arc::clone(&agent)).merge(router(Arc::clone(&agent)));
        let app = loopback::router(
            Arc::clone(&agent),
            api,
            LoopbackConfig { port: PORT, allowed_origins: Vec::new(), name: "Studio PC".into() },
        );
        (agent, app)
    }

    fn agent_token(agent: &Agent) -> String {
        agent.issue_credential("Claude", DeviceKind::Agent).expect("issues").0
    }

    struct Reply {
        status: StatusCode,
        headers: axum::http::HeaderMap,
        body: Value,
    }

    async fn raw(app: &Router, method: &str, host: &str, token: Option<&str>, version: Option<&str>, body: &str) -> Reply {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri("/mcp")
            .header("host", host)
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if let Some(version) = version {
            request = request.header("mcp-protocol-version", version);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).expect("builds"))
            .await
            .expect("responds");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        Reply { status, headers, body: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
    }

    async fn rpc(app: &Router, token: &str, method: &str, params: Value) -> Value {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        let reply = raw(app, "POST", HOST, Some(token), Some("2025-06-18"), &body).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        reply.body
    }

    async fn call(app: &Router, token: &str, tool: &str, arguments: Value) -> (bool, Value) {
        let reply = rpc(app, token, "tools/call", json!({"name": tool, "arguments": arguments})).await;
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
        let failed = result["isError"].as_bool().unwrap_or(false);
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (failed, value)
    }

    #[tokio::test]
    async fn a_request_without_an_agent_token_is_turned_away() {
        let (agent, app) = door();
        let reply = raw(&app, "POST", HOST, None, None, "{}").await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.headers.get(header::WWW_AUTHENTICATE).unwrap(), "Bearer");
        assert!(reply.body["error"].as_str().unwrap().contains("Coding agents"));

        let browser = agent.issue_credential("Laptop", DeviceKind::BrowserLocal).unwrap().0;
        assert_eq!(raw(&app, "POST", HOST, Some(&browser), None, "{}").await.status, StatusCode::FORBIDDEN);

        let token = agent_token(&agent);
        assert_eq!(raw(&app, "POST", "evil:80", Some(&token), None, "{}").await.status, StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(raw(&app, "GET", HOST, Some(&token), None, "").await.status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(raw(&app, "POST", HOST, Some(&token), Some("1999-01-01"), "{}").await.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_endpoint_exists_only_behind_the_loopback_door() {
        let (agent, _) = door();
        let token = agent_token(&agent);
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from("{}"))
            .unwrap();
        let response = crate::http::router(Arc::clone(&agent)).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_agent_adds_a_key_and_reads_it_back() {
        let (agent, app) = door();
        let token = agent_token(&agent);

        let hello = rpc(&app, &token, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})).await;
        assert_eq!(hello["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(hello["result"]["serverInfo"]["name"], "flexdeck");
        let listed = rpc(&app, &token, "tools/list", json!({})).await;
        assert!(listed["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "upsert_widget"));

        let (failed, overview) = call(&app, &token, "deck_overview", json!({})).await;
        assert!(!failed);
        let rev = overview["rev"].as_u64().unwrap();

        let (failed, written) = call(&app, &token, "upsert_widget", json!({"widget": {"props": {"label": "Hi"}}, "rev": rev})).await;
        assert!(!failed, "{written}");
        assert_eq!(written["widgetId"], "widget-1");
        assert_eq!(written["rev"], rev + 1);

        let (failed, read) = call(&app, &token, "get_widget", json!({"widgetId": "widget-1"})).await;
        assert!(!failed);
        assert_eq!(read["pageId"], "home");
        assert_eq!(read["widget"]["col"], 2);
        assert_eq!(read["widget"]["props"]["label"], "Hi");
        let (_, deck) = call(&app, &token, "get_deck", json!({"path": "/schema"})).await;
        assert_eq!(deck["value"], 4);

        let (failed, message) = call(&app, &token, "remove_widget", json!({"widgetId": "widget-1", "rev": rev})).await;
        assert!(failed);
        assert!(message.as_str().unwrap().contains("now rev"), "{message}");

        let (failed, _) = call(&app, &token, "undo", json!({"rev": rev + 1})).await;
        assert!(!failed);
        assert_eq!(agent.rev().unwrap(), Some(rev));
    }

    #[tokio::test]
    async fn a_patch_that_breaks_the_grid_changes_nothing() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        let (failed, message) = call(&app, &token, "patch_deck", json!({"rev": 1, "patch": [
            {"op": "add", "path": "/profiles/0/pages/0/widgets/-", "value": {"id": "x", "type": "WIDGET_TYPE_BUTTON", "col": 1, "row": 0, "w": 1, "h": 1}}
        ]})).await;
        assert!(failed);
        assert!(message.as_str().unwrap().contains("another widget"), "{message}");
        assert_eq!(agent.rev().unwrap(), Some(1));

        let (failed, _) = call(&app, &token, "patch_deck", json!({"rev": 1, "patch": [
            {"op": "replace", "path": "/profiles/0/pages/0/name", "value": "Main"}
        ]})).await;
        assert!(!failed);
        assert_eq!(agent.rev().unwrap(), Some(2));
    }

    #[tokio::test]
    async fn secrets_are_never_taken_through_mcp() {
        let (agent, app) = door();
        let doc: crate::sources::SourcesDoc = serde_json::from_value(json!({"sources": [{
            "id": "weather", "kind": "http", "http": {"url": "https://example.com/{{args.key}}"},
            "args": [{"name": "key", "secret": true}]
        }]}))
        .unwrap();
        agent.behavior().replace_sources(doc).expect("sources");
        let token = agent_token(&agent);
        let (failed, message) = call(&app, &token, "upsert_widget", json!({"rev": 1,
            "widget": {"sources": [{"source": "weather", "args": {"key": "s3cret"}}]}})).await;
        assert!(failed);
        assert!(message.as_str().unwrap().contains("portal"), "{message}");
        assert!(agent.behavior().sources().secrets().is_empty());
        assert_eq!(agent.rev().unwrap(), Some(1));
    }

    fn launcher_key() -> Value {
        json!({"id": "run", "flows": [{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [
            {"block": "doAction", "plugin": "launcher", "action": "run", "args": {"program": {"text": "calc"}}}
        ]}]})
    }

    #[tokio::test]
    async fn a_program_launch_from_an_agent_asks_at_the_desktop() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        let (failed, _) = call(&app, &token, "upsert_widget", json!({"rev": 1, "widget": launcher_key()})).await;
        assert!(failed, "no window to ask in means no");
        assert_eq!(agent.rev().unwrap(), Some(1));

        agent.approvals().enable();
        let answered = crate::access::answer_next(Arc::clone(&agent), true);
        let (failed, written) = call(&app, &token, "upsert_widget", json!({"rev": 1, "widget": launcher_key()})).await;
        assert!(!failed, "{written}");
        let asked = answered.await.unwrap();
        assert_eq!(asked.detail, "run: calc");
        assert_eq!(asked.requester, "Claude");

        let (failed, _) = call(&app, &token, "remove_widget", json!({"rev": 2, "widgetId": "run"})).await;
        assert!(!failed);
        let (failed, message) = call(&app, &token, "undo", json!({"rev": 3})).await;
        assert!(failed, "an undo that brings the launch back is for the desktop");
        assert_eq!(message, crate::http::STEP_RESTORES_LAUNCH);

        let admin = agent.admin_token().unwrap();
        let (failed, _) = call(&app, &admin, "undo", json!({"rev": 3})).await;
        assert!(!failed, "the admin token is the person at this computer");
    }

    #[tokio::test]
    async fn automations_are_revision_checked_too() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        let (_, read) = call(&app, &token, "get_automations", json!({})).await;
        let rev = read["automationsRev"].as_u64().unwrap();
        let document = json!({"schema": 3, "automations": [{"id": "a", "name": "A", "trigger": {"on": "manual"}, "body": []}]});
        let (failed, message) = call(&app, &token, "put_automations", json!({"document": document, "automationsRev": rev + 5})).await;
        assert!(failed, "{message}");
        let (failed, written) = call(&app, &token, "put_automations", json!({"document": document, "automationsRev": rev})).await;
        assert!(!failed, "{written}");
        assert!(written["automationsRev"].as_u64().unwrap() > rev);
    }

    #[tokio::test]
    async fn an_automation_that_presses_keys_asks_at_the_desktop() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        let (_, read) = call(&app, &token, "get_automations", json!({})).await;
        let rev = read["automationsRev"].as_u64().unwrap();
        let press = json!({"block": "doAction", "plugin": "core", "action": "hotkey", "args": {"keys": {"text": "win+r"}}});
        let document = json!({"schema": 3, "automations": [{"id": "a", "name": "A", "trigger": {"on": "manual"}, "body": [press]}]});
        let (failed, message) = call(&app, &token, "put_automations", json!({"document": document, "automationsRev": rev})).await;
        assert!(failed, "{message}");
        let (_, after) = call(&app, &token, "get_automations", json!({})).await;
        assert_eq!(after["automationsRev"].as_u64().unwrap(), rev);

        agent.approvals().enable();
        let answered = crate::access::answer_next(Arc::clone(&agent), true);
        let (failed, written) = call(&app, &token, "put_automations", json!({"document": document, "automationsRev": rev})).await;
        assert!(!failed, "{written}");
        assert_eq!(answered.await.unwrap().detail, "press on its own: keys=win+r");
    }

    #[tokio::test]
    async fn a_key_that_presses_keys_on_an_event_asks_at_the_desktop() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        let widget = json!({"id": "k", "flows": [{"trigger": {"event": {"plugin": "timer", "event": "finished"}}, "blocks": [
            {"block": "doAction", "plugin": "core", "action": "hotkey", "args": {"keys": {"text": "win+r"}}}
        ]}]});
        let (failed, message) = call(&app, &token, "upsert_widget", json!({"rev": 1, "widget": widget})).await;
        assert!(failed, "{message}");
        assert_eq!(agent.rev().unwrap(), Some(1));

        agent.approvals().enable();
        let answered = crate::access::answer_next(Arc::clone(&agent), true);
        let (failed, written) = call(&app, &token, "upsert_widget", json!({"rev": 1, "widget": widget})).await;
        assert!(!failed, "{written}");
        assert_eq!(answered.await.unwrap().detail, "press on its own: keys=win+r");
    }

    #[tokio::test]
    async fn a_launch_read_from_a_variable_is_refused_without_asking() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        agent.approvals().enable();
        let widget = json!({"id": "run", "flows": [{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [
            {"block": "doAction", "plugin": "launcher", "action": "run", "args": {"program": {"variable": "vars.program"}}}
        ]}]});
        let (failed, message) = call(&app, &token, "upsert_widget", json!({"rev": 1, "widget": widget})).await;
        assert!(failed);
        assert_eq!(message, crate::launches::DYNAMIC_LAUNCH);
        assert_eq!(agent.rev().unwrap(), Some(1));
    }

    #[tokio::test]
    async fn variables_are_searched_by_name_and_description_and_capped() {
        use flexdeck_vars::{Kind, VariableSpec};
        let (agent, app) = door();
        let token = agent_token(&agent);
        let vars = agent.behavior().variables();
        vars.declare(VariableSpec::new("obs.preview_scene", Kind::Text, "The scene in Preview"));
        vars.declare(VariableSpec::new("obs.program_scene", Kind::Text, "The scene on air"));
        vars.declare(VariableSpec::new("obs.input.mic.volume", Kind::Number, "Mic level"));
        let names = |listed: &Value| -> Vec<String> {
            listed["variables"].as_array().unwrap().iter().map(|entry| entry["spec"]["name"].as_str().unwrap().to_string()).collect()
        };

        let (_, listed) = call(&app, &token, "list_variables", json!({"query": "PREVIEW scene"})).await;
        assert_eq!(names(&listed), ["obs.preview_scene"]);
        let (_, listed) = call(&app, &token, "list_variables", json!({"query": "on air"})).await;
        assert_eq!(names(&listed), ["obs.program_scene"]);
        let (_, listed) = call(&app, &token, "list_variables", json!({"prefix": "obs.", "query": "scene", "limit": 1})).await;
        assert_eq!(names(&listed).len(), 1);
        assert_eq!(listed["matched"], 2);
    }

    #[tokio::test]
    async fn a_revoked_token_stops_working() {
        let (agent, app) = door();
        let (token, device) = agent.issue_credential("Claude", DeviceKind::Agent).unwrap();
        rpc(&app, &token, "ping", json!({})).await;
        agent.revoke_device(&device.id).unwrap();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}).to_string();
        assert_eq!(raw(&app, "POST", HOST, Some(&token), None, &body).await.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_wire_follows_json_rpc() {
        let (agent, app) = door();
        let token = agent_token(&agent);
        let batch = json!([
            {"jsonrpc": "2.0", "id": "a", "method": "ping"},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 7, "method": "nope"}
        ])
        .to_string();
        let reply = raw(&app, "POST", HOST, Some(&token), None, &batch).await;
        let replies = reply.body.as_array().expect("a batch is answered with a batch");
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["id"], "a");
        assert_eq!(replies[1]["error"]["code"], -32601);

        let reply = raw(&app, "POST", HOST, Some(&token), Some("2025-06-18"), &batch).await;
        assert_eq!(reply.body["error"]["code"], -32600);

        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
        assert_eq!(raw(&app, "POST", HOST, Some(&token), Some("2025-06-18"), &note).await.status, StatusCode::ACCEPTED);

        let reply = raw(&app, "POST", HOST, Some(&token), None, "{not json").await;
        assert_eq!(reply.body["error"]["code"], -32700);
        assert_eq!(reply.body["id"], Value::Null);

        let unknown = rpc(&app, &token, "tools/call", json!({"name": "format_disk", "arguments": {}})).await;
        assert_eq!(unknown["error"]["code"], -32602);
    }
}

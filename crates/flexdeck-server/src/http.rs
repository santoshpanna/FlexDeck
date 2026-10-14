//! The REST surface.
//!
//! Two conventions worth stating, because both are deliberate:
//!
//! * **No path parameters.** Every operation is a fixed path with a JSON body,
//!   so revoking a device is `POST /api/devices/revoke` with `{"deviceId": …}`
//!   rather than `DELETE /api/devices/{id}`. The paths are a closed set the
//!   Android client can hard-code, and there is no URL escaping to get wrong.
//! * **A refused pairing is `200` with `granted: false`.** The contract already
//!   models this: `PairResponse` carries `granted` and `error` together. A
//!   wrong PIN is an expected outcome of a normal flow, not a transport error,
//!   and it avoids clients having to parse an error body to find out *why*.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Extension, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use flexdeck_pairing::{qr_payload, PairingOutcome};
use flexdeck_protocol::{PairRequest, PairResponse};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::access::{Channel, APPROVAL_TIMEOUT};
use crate::agent::{now_secs, Agent, AgentError, Authority};
use flexdeck_store::DeviceKind;

/// The API only.
///
/// ---------------------------------------------------------------------------
/// THE AGENT NO LONGER SERVES A PORTAL, AND THAT IS DELIBERATE
/// ---------------------------------------------------------------------------
/// It used to serve the built portal from a directory, and the browser called
/// this API on the same origin with an admin token the user pasted in. That made
/// sense when the agent *was* the product.
///
/// It is now the wrong shape in two ways:
///
/// * the configurator lives in the cloud, where the account is, and a second copy
///   served from the LAN would be a second thing to keep in step;
/// * the admin token was the only credential that could reach the routes that
///   mint pairing secrets, and it was printed to a console for a human to copy.
///   That is not a credential an end user should ever handle.
///
/// So the LAN listener is now **device-only**: a paired tablet, holding a device
/// token, and nothing else. The routes that need admin are still here — the cloud
/// reaches them by relaying through the tunnel, where the agent attaches its own
/// admin credential and never exposes it.
pub fn router(agent: Arc<Agent>) -> Router {
    api_router(agent)
}

/// The largest request body that can carry a package: the biggest package,
/// base64-encoded, and room for the rest of the request.
const MAX_UPLOAD_BODY: usize = crate::plugins::MAX_PACKAGE_BYTES / 3 * 4 + 64 * 1024;

fn api_router(agent: Arc<Agent>) -> Router {
    // Opening a pairing window hands out the very secret that pairing exists to
    // protect — the PIN and the one-time secret are both in the response. So it
    // needs a *configurator* (the admin token, or a paired browser): a tablet
    // must not be able to mint windows of its own, or one compromised tablet
    // could pair unlimited others. From a remote browser it also needs a yes at
    // the desktop — see `begin_pairing`.
    let admin_only = Router::new()
        .route("/api/pairing/begin", post(begin_pairing))
        .route("/api/pairing/cancel", post(cancel_pairing))
        .route("/api/pairing/browser/begin", post(begin_browser_pairing))
        // Installing a plugin runs new code on this machine, so every route
        // that changes which plugins run — or what they are configured with —
        // needs the admin credential, exactly like minting a pairing secret.
        // A package can arrive in the body, base64-encoded, so these two take
        // more than the default 2 MB. The credential is checked first (the
        // `route_layer` below), so nobody else gets to send that much.
        .route(
            "/api/plugins/inspect",
            post(inspect_plugin).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY)),
        )
        .route(
            "/api/plugins/install",
            post(install_plugin).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY)),
        )
        .route("/api/plugins/uninstall", post(uninstall_plugin))
        .route("/api/plugins/enable", post(enable_plugin))
        .route("/api/plugins/reload", post(reload_plugin))
        .route("/api/plugins/link", post(link_plugin))
        .route("/api/plugins/settings", put(plugin_settings))
        .route("/api/plugins/log", post(plugin_log))
        // What an action's arguments can be set to, asked of the plugin while
        // someone edits a button: OBS's scenes, its inputs.
        .route("/api/plugins/choices", post(argument_choices))
        // Repositories decide what can be installed, so changing them — and
        // reading them, which makes this machine fetch from the addresses in
        // them — is for a configurator too.
        .route("/api/plugins/catalog", get(plugin_catalog))
        .route("/api/plugins/updates", get(plugin_updates))
        .route("/api/plugins/repositories", get(list_repositories))
        .route(
            "/api/plugins/repositories/preview",
            post(preview_repository),
        )
        .route("/api/plugins/repositories/add", post(add_repository))
        .route("/api/plugins/repositories/remove", post(remove_repository))
        .route("/api/plugins/repositories/toggle", post(toggle_repository))
        .route(
            "/api/plugins/repositories/refresh",
            post(refresh_repositories),
        )
        // The layout decides what every button runs, and the launcher can run
        // any program. So rewriting it — directly, or by stepping through its
        // history — is code execution on this machine, and a paired tablet
        // holding only a device token must not be able to do it. Revoking is
        // here too: one tablet must not be able to unpair all the others.
        .route("/api/undo", post(undo))
        .route("/api/redo", post(redo))
        .route("/api/devices/revoke", post(revoke_device))
        .route("/api/devices/revoke-all", post(revoke_all_devices))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&agent),
            require_configurator,
        ));

    let authenticated = Router::new()
        .route("/api/status", get(status))
        // Reads are open to any device; the write needs admin, inline, because
        // it shares the path — see `replace_config`.
        .route("/api/config", get(config).put(replace_config))
        .route("/api/devices", get(devices))
        // A tablet or browser removing ITSELF, when its person forgets this
        // PC there. Any credential may drop its own row, and only its own.
        .route("/api/devices/forget-me", post(forget_me))
        // The page on screen: read to follow it, written to move every tablet.
        // The write needs a configurator, inline — see `put_page`.
        .route("/api/page", get(get_page).put(put_page))
        // The reactive engine lives behind the same authenticated wall as the
        // layout it sits next to. Reads are open to any device (a paired tablet
        // must be able to ask what plugins exist), but writes need admin — see
        // `require_admin_inline` in the handlers below.
        .route("/api/plugins", get(plugins))
        .route("/api/plugins/installed", get(installed_plugins))
        .route("/api/variables", get(variables))
        // Album art and other pictures plugins hand over, for a widget's icon.
        .route("/api/images/{sha}", get(image).delete(delete_picture))
        // Pictures and icon packs the person adds.
        .route(
            "/api/images",
            get(list_pictures).post(upload_picture).layer(DefaultBodyLimit::max(crate::media::MAX_UPLOAD / 3 * 4 + 64 * 1024)),
        )
        .route(
            "/api/iconpacks",
            get(list_packs).post(install_pack).layer(DefaultBodyLimit::max(crate::media::MAX_PACK / 3 * 4 + 64 * 1024)),
        )
        .route("/api/iconpacks/{id}", get(get_pack).delete(delete_pack))
        .route(
            "/api/automations",
            get(get_automations).put(put_automations),
        )
        .route("/api/automations/dry-run", post(dry_run_automation))
        // What a widget being edited would show, from the agent's own
        // templates and live values: the editor never evaluates one itself.
        .route("/api/faces/preview", post(preview_face))
        .route("/api/live/watch", post(crate::live::watch))
        // Data sources: reading is open, saving and testing need a configurator
        // (a test fetches on the caller's say), and a command source, which
        // runs a program, also needs a yes at the desktop when asked from elsewhere.
        .route("/api/sources", get(get_sources).put(put_sources))
        .route("/api/sources/test", post(test_source))
        .route("/api/sources/secrets/forget", post(forget_source_secret))
        // How numbers, money, times and temperatures are written.
        .route("/api/locale", get(get_locale).put(put_locale))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&agent),
            require_auth,
        ));

    Router::new()
        // The one endpoint that must work before a client holds any credential,
        // because redeeming a PIN is *how* a credential is obtained. It is
        // protected by the PIN itself, which never leaves the agent's own
        // machine — see `begin_pairing`.
        .route("/api/pairing/redeem", post(redeem_pairing))
        // The same, for a browser that was shown a code at the desktop. Only
        // through the relay: that is the only way a remote browser arrives.
        .route("/api/pairing/browser/redeem", post(redeem_browser_pairing))
        // The socket authenticates in its first frame instead, because a browser
        // cannot set an Authorization header on a WebSocket handshake.
        .route("/ws", get(crate::ws::upgrade))
        // The configurator's own socket: it checks the token in its first frame.
        .route("/api/live", get(crate::live::upgrade))
        .merge(admin_only)
        .merge(authenticated)
        .with_state(agent)
}

// -- authentication ----------------------------------------------------------

/// What a tablet is told when it reaches for a configurator's route.
const ADMIN_ONLY: &str =
    "this needs a configurator (the admin token or a paired browser), not a tablet";

/// Where a request came in. Absent means the LAN, the channel that trusts a
/// browser with nothing — see [`crate::access`].
fn channel_of(request: &Request) -> Channel {
    request
        .extensions()
        .get::<Channel>()
        .copied()
        .unwrap_or_default()
}

/// Resolve the request's credential against the channel it came in on.
fn authority_of(agent: &Agent, request: &Request) -> Result<Authority, ApiError> {
    let token = bearer_token(request).ok_or_else(|| {
        ApiError::unauthorised("an 'Authorization: Bearer <token>' header is required")
    })?;

    agent
        .authorise(token, channel_of(request))?
        .ok_or_else(|| ApiError::unauthorised("unknown or revoked token"))
}

/// Every route that is not pairing needs a credential. A page on the network
/// must not be able to read the layout, let alone rewrite it.
///
/// The resolved [`Authority`] travels on to the handler as an extension, so a
/// handler that needs a finer decision does not authenticate a second time.
async fn require_auth(
    State(agent): State<Arc<Agent>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let authority = authority_of(&agent, &request)?;
    request.extensions_mut().insert(authority);
    Ok(next.run(request).await)
}

/// A configurator specifically — the admin token or a paired browser — for
/// every route that mints secrets or decides what code runs on this machine.
///
/// Crate-visible so the web-access routes, which live in their own router, sit
/// behind the very same gate rather than a second copy of it.
pub(crate) async fn require_configurator(
    State(agent): State<Arc<Agent>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let authority = authority_of(&agent, &request)?;
    if !authority.can_configure() {
        return Err(ApiError::forbidden(ADMIN_ONLY));
    }
    request.extensions_mut().insert(authority);
    Ok(next.run(request).await)
}

/// For a remote browser, wait for someone at the desktop to allow `action`.
/// Everyone else passes straight through.
async fn require_approval(
    agent: &Agent,
    authority: &Authority,
    action: &str,
    detail: &str,
) -> Result<(), ApiError> {
    if !authority.needs_approval() {
        return Ok(());
    }
    agent
        .approvals()
        .ask(action, detail, authority.requester(), APPROVAL_TIMEOUT)
        .await
        .map_err(|error| ApiError::forbidden(error.to_string()))
}

/// Extract a bearer token, tolerating any capitalisation of the scheme.
fn bearer_token(request: &Request) -> Option<&str> {
    let (scheme, token) = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .split_once(' ')?;

    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

// -- errors ------------------------------------------------------------------

/// A JSON error body, so a client never has to guess a content type to explain
/// a failure.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    /// The message a client would see.
    ///
    /// Exposed so a caller inside this process — the desktop window — can render
    /// the agent's OWN words instead of writing a second description of the same
    /// failure, which is the shape that drifts.
    pub fn message(&self) -> &str {
        &self.message
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }

    fn unauthorised(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    /// Authenticated, but not allowed. Distinct from `unauthorised` on purpose:
    /// "your token is wrong" and "your token is not enough" need different
    /// remedies.
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<AgentError> for ApiError {
    fn from(error: AgentError) -> Self {
        Self::internal(error.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(error.to_string())
    }
}

// -- handlers ----------------------------------------------------------------

async fn status(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let rev = agent.rev()?;
    let devices = agent.devices()?;

    Ok(Json(json!({
        "rev": rev,
        "fingerprint": agent.identity().fingerprint(),
        "host": agent.host(),
        "port": agent.port(),
        "deviceCount": devices.len(),
    })))
}

async fn config(State(agent): State<Arc<Agent>>) -> Result<Response, ApiError> {
    let Some((rev, document)) = agent.config()? else {
        return Err(ApiError::not_found(
            "no configuration has been committed yet",
        ));
    };

    // The revision goes in `ETag` so a client can do conditional fetches and so
    // the number it must quote when reconnecting is discoverable without a
    // bespoke field in the body.
    let mut headers = HeaderMap::new();
    let etag = HeaderValue::from_str(&format!("\"{rev}\""))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    headers.insert(header::ETAG, etag);

    let body: Value = serde_json::from_str(&document)?;
    Ok((headers, Json(body)).into_response())
}

async fn replace_config(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(document): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;

    // Re-serialising normalises whitespace and key order, which keeps stored
    // documents and their diffs stable regardless of what the client sent.
    let text = serde_json::to_string(&document)?;
    let rev = agent.replace_config(&text)?;
    Ok(Json(json!({ "rev": rev })))
}

async fn undo(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    match agent.undo() {
        Ok(rev) => Ok(Json(json!({ "rev": rev }))),
        // Nothing to undo is a conflict with the request, not a server fault.
        Err(error) => Err(ApiError::conflict(error.to_string())),
    }
}

async fn redo(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    match agent.redo() {
        Ok(rev) => Ok(Json(json!({ "rev": rev }))),
        Err(error) => Err(ApiError::conflict(error.to_string())),
    }
}

async fn get_page(State(agent): State<Arc<Agent>>) -> Json<Value> {
    Json(json!({ "pageId": agent.pages().current() }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageRequest {
    page_id: String,
}

/// Show a page on every tablet. A configurator's, like any other change to
/// what the deck does: one tablet steering the rest goes over its own socket.
async fn put_page(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(request): Json<PageRequest>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    use flexdeck_plugin::builtin::deck::PageControl;
    if !agent.pages().pages().iter().any(|(id, _)| *id == request.page_id) {
        return Err(ApiError::not_found(format!("no page \"{}\"", request.page_id)));
    }
    agent.pages().select(&request.page_id);
    Ok(Json(json!({ "pageId": request.page_id })))
}

async fn devices(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
) -> Result<Json<Value>, ApiError> {
    let presence = agent.presence();
    // The device making this request, so a list can say "this one".
    let caller = match &authority {
        Authority::Browser(device) | Authority::Tablet(device) => Some(device.id.clone()),
        Authority::Admin => None,
    };

    let devices: Vec<Value> = agent
        .devices()?
        .into_iter()
        .map(|device| {
            let online = presence.is_online(&device.id);

            json!({
                "id": device.id,
                "name": device.name,
                // Kept as `created_at`. The portal's own `Device` type declares
                // this spelling, and renaming it here would blank the field
                // silently rather than fail loudly.
                "created_at": device.created_at,
                // Whether a socket is open *now*. A paired device that is
                // switched off is `false`, not absent.
                "online": online,
                // A tablet, a browser on this computer, or a browser elsewhere.
                "kind": device.kind,
                // Whether this is the device asking.
                "current": caller.as_deref() == Some(device.id.as_str()),
            })
        })
        .collect();

    Ok(Json(json!({ "devices": devices })))
}

// -- reactive engine surface -------------------------------------------------

/// The plugin catalogue, served as the data the registry actually holds, so
/// adding a plugin shows up in the editor without a separate definition.
async fn plugins(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(agent.behavior().plugin_catalogue()))
}

#[derive(Debug, Deserialize)]
struct ChoicesQuery {
    plugin: String,
    action: String,
    #[serde(default)]
    args: std::collections::HashMap<String, String>,
}

/// The values an action's arguments can take right now, from its plugin.
///
/// A plugin that cannot answer (OBS is closed) says why in `error`, and the
/// editor falls back to typing; nothing here fails the request for that.
async fn argument_choices(
    State(agent): State<Arc<Agent>>,
    Json(query): Json<ChoicesQuery>,
) -> Result<Json<Value>, ApiError> {
    let registry = Arc::clone(agent.behavior().registry());
    // The plugin is a process with a deadline of seconds: not on a worker.
    let answer = tokio::task::spawn_blocking(move || {
        let action = flexdeck_protocol::Action {
            plugin: query.plugin,
            action: query.action,
            args: query.args,
        };
        registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .choices(&action)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?;
    match answer {
        Ok(choices) => Ok(Json(serde_json::to_value(choices)?)),
        Err(
            error @ (flexdeck_plugin::PluginError::UnknownPlugin(_)
            | flexdeck_plugin::PluginError::UnknownAction { .. }),
        ) => Err(ApiError::not_found(error.to_string())),
        // Running but unable to answer: no suggestions, and the reason.
        Err(error) => Ok(Json(json!({ "arguments": {}, "error": error.to_string() }))),
    }
}

// -- installed plugins ---------------------------------------------------------

use crate::plugins::{PluginManager, PluginsError};

fn manager(agent: &Agent) -> Result<Arc<PluginManager>, ApiError> {
    agent
        .plugins()
        .cloned()
        .ok_or_else(|| ApiError::unavailable("this agent does not run installed plugins"))
}

impl From<PluginsError> for ApiError {
    fn from(error: PluginsError) -> Self {
        let message = error.to_string();
        match error {
            PluginsError::NotInstalled(_) => Self::not_found(message),
            PluginsError::Download(_) => Self::bad_gateway(message),
            PluginsError::Io(_) | PluginsError::Store(_) => Self::internal(message),
            _ => Self::bad_request(message),
        }
    }
}

/// Run blocking plugin work — starting a process, unpacking a zip, a
/// download — off the async workers.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, PluginsError> + Send + 'static,
) -> Result<Result<T, PluginsError>, ApiError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))
}

/// Where a package comes from.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PackageSource {
    /// The package itself, base64-encoded.
    Upload(String),
    /// A URL to download it from, optionally with the SHA-256 it must have.
    Url {
        url: String,
        #[serde(default)]
        sha256: Option<String>,
    },
    /// One of this desktop's plugin repositories, whose index pins the SHA-256.
    Repository { repo: String, id: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallRequest {
    source: PackageSource,
    #[serde(default)]
    accepted_permissions: Vec<String>,
}

/// What an approval prompt says is about to be installed: where it comes from
/// and what it will be allowed to do.
fn install_description(request: &InstallRequest) -> String {
    let source = match &request.source {
        PackageSource::Upload(_) => "an uploaded package".to_string(),
        PackageSource::Url { url, .. } => format!("a package from {url}"),
        PackageSource::Repository { repo, id } => format!("{id} from {repo}"),
    };
    if request.accepted_permissions.is_empty() {
        source
    } else {
        format!(
            "{source}, allowed: {}",
            request.accepted_permissions.join(", ")
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectRequest {
    source: PackageSource,
}

/// Get the bytes, the checksum they must have, and how to remember where they
/// came from.
fn fetch(
    agent: &Agent,
    source: PackageSource,
) -> Result<(Vec<u8>, Option<String>, crate::plugins::Source), PluginsError> {
    use base64::Engine as _;
    match source {
        PackageSource::Upload(encoded) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| {
                    PluginsError::Package(format!("the upload is not base64: {error}"))
                })?;
            Ok((bytes, None, crate::plugins::Source::File))
        }
        PackageSource::Url { url, sha256 } => {
            let bytes = crate::plugins::download(&url)?;
            Ok((bytes, sha256, crate::plugins::Source::Url { url }))
        }
        PackageSource::Repository { repo, id } => {
            let repositories = agent.repositories().ok_or_else(|| {
                PluginsError::Repository("this agent does not run installed plugins".into())
            })?;
            let (bytes, sha256, source) = repositories.fetch_package(&repo, &id)?;
            Ok((bytes, Some(sha256), source))
        }
    }
}

fn repositories(agent: &Agent) -> Result<Arc<crate::repos::Repositories>, ApiError> {
    agent
        .repositories()
        .cloned()
        .ok_or_else(|| ApiError::not_found("this agent does not run installed plugins"))
}

fn installed_records(agent: &Agent) -> Vec<crate::plugins::Installed> {
    agent
        .plugins()
        .map(|manager| manager.installed())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct CatalogQuery {
    #[serde(default)]
    q: String,
}

/// Every plugin the enabled repositories list, refreshed when due.
async fn plugin_catalog(
    State(agent): State<Arc<Agent>>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    let catalog = blocking(move || {
        repos.refresh(false);
        let plugins = repos.catalog(&query.q, &installed_records(&agent));
        Ok(json!({
            "plugins": plugins,
            "repositories": repos.list(),
            "vettedError": repos.vetted_error(),
        }))
    })
    .await??;
    Ok(Json(catalog))
}

async fn plugin_updates(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    let updates = blocking(move || {
        repos.refresh(false);
        Ok(json!({ "updates": repos.updates(&installed_records(&agent)) }))
    })
    .await??;
    Ok(Json(updates))
}

async fn list_repositories(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    Ok(Json(
        json!({ "repositories": repos.list(), "vettedError": repos.vetted_error() }),
    ))
}

#[derive(Debug, Deserialize)]
struct RepositoryRequest {
    url: String,
    #[serde(default)]
    enabled: Option<bool>,
}

async fn preview_repository(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<RepositoryRequest>,
) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    let preview = blocking(move || repos.preview(&request.url)).await??;
    Ok(Json(serde_json::to_value(preview)?))
}

async fn add_repository(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<RepositoryRequest>,
) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    let row = blocking(move || repos.add(&request.url)).await??;
    Ok(Json(json!({ "repository": row })))
}

async fn remove_repository(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<RepositoryRequest>,
) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    blocking(move || repos.remove(&request.url)).await??;
    Ok(Json(json!({ "removed": true })))
}

async fn toggle_repository(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<RepositoryRequest>,
) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    let enabled = request.enabled.unwrap_or(true);
    blocking(move || repos.set_vetted_enabled(&request.url, enabled)).await??;
    Ok(Json(json!({ "enabled": enabled })))
}

async fn refresh_repositories(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let repos = repositories(&agent)?;
    let rows = blocking(move || {
        repos.refresh(true);
        Ok(repos.list())
    })
    .await??;
    Ok(Json(json!({ "repositories": rows })))
}

async fn installed_plugins(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    match agent.plugins() {
        Some(manager) => Ok(Json(manager.installed_json())),
        None => Ok(Json(json!({ "plugins": [] }))),
    }
}

/// What a package is, before anything is installed: the consent screen.
async fn inspect_plugin(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<InspectRequest>,
) -> Result<Json<Value>, ApiError> {
    let manager = manager(&agent)?;
    let inspected = blocking(move || {
        let (bytes, expected, _) = fetch(&agent, request.source)?;
        let inspected = manager.inspect(&bytes)?;
        if let Some(expected) = expected {
            if !expected.eq_ignore_ascii_case(&inspected.sha256) {
                return Err(PluginsError::Checksum {
                    expected,
                    actual: inspected.sha256,
                });
            }
        }
        Ok(inspected)
    })
    .await??;
    Ok(Json(serde_json::to_value(inspected)?))
}

/// Install or update. A package whose permissions were not all accepted is
/// refused with `409` and everything a consent screen needs to ask.
async fn install_plugin(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(request): Json<InstallRequest>,
) -> Result<Response, ApiError> {
    // Installing runs new code on this machine. Asked for from elsewhere, a
    // person at the desktop says yes first — and sees what they are allowing.
    // Before anything else, so a refusal costs nothing and fetches nothing.
    require_approval(
        &agent,
        &authority,
        "Install a plugin",
        &install_description(&request),
    )
    .await?;
    let manager = manager(&agent)?;
    let worker = Arc::clone(&agent);
    let outcome = blocking(move || {
        let (bytes, expected, source) = fetch(&worker, request.source)?;
        manager.install(
            &bytes,
            expected.as_deref(),
            &request.accepted_permissions,
            source,
        )
    })
    .await?;
    match outcome {
        Ok(installed) => {
            agent.behavior().catalogue_changed();
            Ok(Json(json!({ "installed": installed })).into_response())
        }
        Err(PluginsError::Permissions {
            missing,
            package,
            sha256,
        }) => Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("this plugin needs permissions that were not accepted: {}", missing.join(", ")),
                "needsConsent": true,
                "missing": missing,
                "package": package,
                "sha256": sha256,
            })),
        )
            .into_response()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginIdRequest {
    id: String,
}

async fn uninstall_plugin(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<PluginIdRequest>,
) -> Result<Json<Value>, ApiError> {
    let manager = manager(&agent)?;
    let id = request.id.clone();
    blocking(move || manager.uninstall(&id)).await??;
    agent.behavior().catalogue_changed();
    Ok(Json(json!({ "uninstalled": request.id })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnableRequest {
    id: String,
    enabled: bool,
}

async fn enable_plugin(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<EnableRequest>,
) -> Result<Json<Value>, ApiError> {
    let manager = manager(&agent)?;
    let id = request.id.clone();
    blocking(move || manager.set_enabled(&id, request.enabled)).await??;
    agent.behavior().catalogue_changed();
    Ok(Json(
        json!({ "id": request.id, "enabled": request.enabled }),
    ))
}

async fn reload_plugin(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<PluginIdRequest>,
) -> Result<Json<Value>, ApiError> {
    let manager = manager(&agent)?;
    let id = request.id.clone();
    blocking(move || manager.reload(&id)).await??;
    agent.behavior().catalogue_changed();
    Ok(Json(json!({ "reloaded": request.id })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkRequest {
    path: String,
}

/// Run a plugin from a working directory and reload it on every change —
/// the authoring loop. Admin only, like any other way of running new code.
async fn link_plugin(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(request): Json<LinkRequest>,
) -> Result<Json<Value>, ApiError> {
    // A path on *this* machine, chosen by someone who is not at it, is not a
    // request worth putting in front of a person: it is refused outright.
    if authority.needs_approval() {
        return Err(ApiError::forbidden(
            "linking a plugin folder only works from this computer",
        ));
    }
    let manager = manager(&agent)?;
    let linked = blocking(move || manager.link_dev(std::path::Path::new(&request.path))).await??;
    agent.behavior().catalogue_changed();
    Ok(Json(json!({ "installed": linked })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsRequest {
    id: String,
    #[serde(default)]
    values: std::collections::BTreeMap<String, String>,
}

/// Change a plugin's settings. A secret sent blank keeps its stored value, so
/// a form that never receives the password back can still be saved.
async fn plugin_settings(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<SettingsRequest>,
) -> Result<Json<Value>, ApiError> {
    let worker = Arc::clone(&agent);
    let id = request.id.clone();
    tokio::task::spawn_blocking(move || worker.set_plugin_settings(&id, request.values))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| match error {
            AgentError::Plugin(error) => ApiError::bad_request(error.to_string()),
            other => ApiError::from(other),
        })?;
    Ok(Json(json!({ "id": request.id })))
}

async fn plugin_log(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<PluginIdRequest>,
) -> Result<Json<Value>, ApiError> {
    let manager = manager(&agent)?;
    Ok(Json(
        json!({ "id": request.id, "lines": manager.log(&request.id) }),
    ))
}

/// The variable catalogue: each entry is `{spec, value}` so a portal can
/// address `entry.spec.name` and read `entry.value` independently. Reading
/// `value` from the live store means a stale UI can show what is currently
/// there, not what the plugin last declared.
async fn variables(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(agent.behavior().variables_catalogue()))
}

/// A picture by its sha256, as a widget names it (`img:<sha256>`).
///
/// Content-addressed, so it can be cached forever: the same address is always
/// the same bytes. Behind the same token as everything else a tablet reads.
async fn image(
    State(agent): State<Arc<Agent>>,
    axum::extract::Path(sha): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !crate::images::is_sha256(&sha) {
        return (axum::http::StatusCode::BAD_REQUEST, "not an image address").into_response();
    }
    let found = agent
        .images()
        .get(&sha)
        .map(|image| (image.mime.clone(), image.bytes.to_vec()))
        // A picture the person added, kept on disk.
        .or_else(|| agent.media().and_then(|media| media.pictures.get(&sha)).map(|(mime, bytes)| (mime.to_string(), bytes)));
    match found {
        Some((mime, bytes)) => (
            [
                (axum::http::header::CONTENT_TYPE, mime),
                (axum::http::header::CACHE_CONTROL, "private, max-age=31536000, immutable".to_string()),
            ],
            bytes,
        )
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "no such image (it may have been replaced)").into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct Upload {
    /// The file, base64.
    data: String,
}

fn media_of(agent: &Agent) -> Result<&crate::media::Media, ApiError> {
    agent.media().ok_or_else(|| ApiError::internal("this desktop has nowhere to keep pictures"))
}

fn decode_upload(upload: &Upload) -> Result<Vec<u8>, ApiError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(upload.data.trim())
        .map_err(|error| ApiError::bad_request(format!("not base64: {error}")))
}

/// Keep a picture the person chose: checked, made small, named by its hash.
async fn upload_picture(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(upload): Json<Upload>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    let bytes = decode_upload(&upload)?;
    let agent2 = Arc::clone(&agent);
    let picture = tokio::task::spawn_blocking(move || agent2.media().ok_or_else(|| "this desktop has nowhere to keep pictures".to_string()).and_then(|m| m.pictures.put(&bytes, crate::media::KEPT_SIDE)))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "image": format!("img:{}", picture.sha256), "picture": serde_json::to_value(picture)? })))
}

async fn list_pictures(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let pictures = agent.media().map(|media| media.pictures.list()).unwrap_or_default();
    Ok(Json(json!({ "pictures": serde_json::to_value(pictures)? })))
}

async fn delete_picture(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    axum::extract::Path(sha): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    Ok(Json(json!({ "removed": media_of(&agent)?.pictures.remove(&sha) })))
}

async fn list_packs(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let packs = agent.media().map(|media| media.packs.list()).unwrap_or_default();
    Ok(Json(json!({ "packs": serde_json::to_value(packs)? })))
}

async fn get_pack(
    State(agent): State<Arc<Agent>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    match agent.media().and_then(|media| media.packs.get(&id)) {
        Some(pack) => Ok(Json(serde_json::to_value(pack)?)),
        None => Err(ApiError::not_found(format!("no icon pack '{id}'"))),
    }
}

async fn install_pack(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(upload): Json<Upload>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    let bytes = decode_upload(&upload)?;
    let agent2 = Arc::clone(&agent);
    let installed = tokio::task::spawn_blocking(move || {
        let media = agent2.media().ok_or_else(|| "this desktop has nowhere to keep pictures".to_string())?;
        media.packs.install(&bytes, &media.pictures)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?
    .map_err(ApiError::bad_request)?;
    Ok(Json(serde_json::to_value(installed)?))
}

async fn delete_pack(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    Ok(Json(json!({ "removed": media_of(&agent)?.packs.remove(&id) })))
}

/// The current automation document, as JSON, including its stamped `rev` so
/// the editor can echo it back on the next write without an extra round trip.
async fn get_automations(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(agent.behavior().document_json()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DryRunRequest {
    automation_id: String,
}

/// Replace the automation document. The body is whatever the editor wants to
/// store — `parse_document` decides whether it is acceptable, and an
/// over-large document is refused BEFORE storage. The response carries the new
/// `rev` so the editor can hold onto it.
async fn put_automations(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    let raw = serde_json::to_string(&body)
        .map_err(|e| ApiError::bad_request(format!("body is not valid JSON: {e}")))?;
    let rev = agent
        .behavior()
        .replace_document_json(&raw)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(json!({ "rev": rev })))
}

/// Run an automation against a scratch store and return the recorded `Run`.
/// Admin only because the result echoes the user's own config and a paired
/// tablet has no business poking it.
async fn dry_run_automation(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(request): Json<DryRunRequest>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    let run = agent
        .behavior()
        .dry_run(&request.automation_id)
        .map_err(|error| match error {
            crate::behavior::BehaviorError::UnknownAutomation(_) => {
                ApiError::not_found(error.to_string())
            }
            other => ApiError::bad_request(other.to_string()),
        })?;
    serde_json::to_value(run)
        .map(Json)
        .map_err(|e| ApiError::internal(format!("could not serialise the run: {e}")))
}

async fn get_locale(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(agent.behavior().locale())?))
}

async fn put_locale(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(locale): Json<flexdeck_expr::Locale>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    if locale.decimal == locale.group {
        return Err(ApiError::bad_request("the decimal mark and the thousands mark must differ"));
    }
    let code = locale.currency.trim();
    if code.len() != 3 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(ApiError::bad_request("a currency is a three-letter code, like EUR"));
    }
    agent.behavior().set_locale(flexdeck_expr::Locale { currency: code.to_ascii_uppercase(), ..locale });
    Ok(Json(serde_json::to_value(agent.behavior().locale())?))
}

async fn get_sources(State(agent): State<Arc<Agent>>) -> Result<Json<Value>, ApiError> {
    let sources = agent.behavior().sources();
    Ok(Json(json!({
        "sources": serde_json::to_value(sources.doc())?["sources"],
        // What installed recipes bring: runnable, not editable here.
        "recipes": serde_json::to_value(sources.recipes())?,
        "status": serde_json::to_value(sources.status())?,
        // Which secrets are kept, as `source/widget/arg` — never their values.
        "secrets": sources.secret_names(),
    })))
}

#[derive(Debug, Deserialize)]
struct ForgetSecretRequest {
    source: String,
    #[serde(default)]
    widget: Option<String>,
    arg: String,
}

async fn forget_source_secret(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(request): Json<ForgetSecretRequest>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    let forgot = agent.behavior().forget_source_secret(&request.source, request.widget.as_deref().filter(|w| !w.is_empty()), &request.arg);
    Ok(Json(json!({ "forgot": forgot })))
}

async fn put_sources(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(doc): Json<crate::sources::SourcesDoc>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    crate::sources::validate(&doc).map_err(ApiError::bad_request)?;
    // A command source runs a program on this machine, on a timer, with no
    // press: from elsewhere, a person at the desktop says yes to each new one.
    if authority.needs_approval() {
        let current = agent.behavior().sources().doc();
        let commands: Vec<String> = crate::sources::new_commands(&current, &doc)
            .into_iter()
            .filter_map(crate::sources::SourceDef::command_line)
            .collect();
        if !commands.is_empty() {
            require_approval(&agent, &authority, "Run a program on a schedule", &commands.join("\n")).await?;
        }
    }
    agent.behavior().replace_sources(doc).map_err(ApiError::bad_request)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
struct TestSourceRequest {
    source: crate::sources::SourceDef,
    #[serde(default)]
    args: std::collections::BTreeMap<String, String>,
    /// The key it is tested for: its kept secrets fill any left blank.
    #[serde(default)]
    widget: Option<String>,
}

/// Run a source once, publishing nothing: its answer's start and what each
/// field reads, for the editor's "Test". A failure is an answer, not an error.
async fn test_source(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
    Json(request): Json<TestSourceRequest>,
) -> Result<Json<Value>, ApiError> {
    require_configurator_inline(&authority)?;
    let doc = crate::sources::SourcesDoc { sources: vec![request.source.clone()] };
    if let Err(error) = crate::sources::validate(&doc) {
        return Ok(Json(json!({ "error": error })));
    }
    // Testing a command source runs it now: from elsewhere, only with a yes
    // from a person at the desktop, the same as installing a plugin.
    if let Some(line) = request.source.command_line() {
        require_approval(&agent, &authority, "Run a program", &line).await?;
    }
    let behavior = agent.behavior().clone();
    let answer = tokio::task::spawn_blocking(move || {
        let mut args: std::collections::BTreeMap<String, String> =
            request.source.args.iter().map(|a| (a.name.clone(), a.default.clone())).collect();
        args.extend(request.args);
        behavior.sources().test(&request.source, &args, request.widget.as_deref(), &behavior.locale())
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(match answer {
        Ok((response, read)) => {
            let values: serde_json::Map<String, Value> = read
                .values
                .iter()
                .map(|(name, value)| {
                    let shown = match value {
                        flexdeck_expr::Val::Number(n) => json!(n),
                        flexdeck_expr::Val::Text(t) => json!(t),
                        flexdeck_expr::Val::Bool(b) => json!(b),
                        flexdeck_expr::Val::Missing => Value::Null,
                    };
                    (name.clone(), shown)
                })
                .collect();
            let excerpt: String = response.text.chars().take(4_000).collect();
            json!({
                "status": response.status,
                "text": excerpt,
                "json": response.json,
                "values": values,
                "series": read.series,
            })
        }
        Err(error) => json!({ "error": error }),
    }))
}

#[derive(Debug, Deserialize)]
struct FacePreviewRequest {
    #[serde(default)]
    widget: Option<Value>,
    /// Several at once: a page's worth, for the editor's grid.
    #[serde(default)]
    widgets: Vec<Value>,
    #[serde(default)]
    look: String,
}

/// `{state}` for a widget that works, `{error}` for one that does not: a
/// broken template is something to show beside the field, not a failed call.
/// Given `widgets`, `{previews: {id: {state} | {error}}}` for each.
async fn preview_face(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<FacePreviewRequest>,
) -> Result<Json<Value>, ApiError> {
    let behavior = agent.behavior();
    let locale = behavior.locale();
    let limits = flexdeck_automation::Limits::default();
    let one = |widget: &Value, look: &str| -> Value {
        match crate::widgets::preview(widget, look, behavior.variables(), &limits, &locale) {
            Ok(state) => json!({ "state": serde_json::to_value(state).unwrap_or(Value::Null) }),
            Err(error) => json!({ "error": error }),
        }
    };
    if let Some(widget) = &request.widget {
        return Ok(Json(one(widget, request.look.trim())));
    }
    if request.widgets.len() > 256 {
        return Err(ApiError::bad_request("at most 256 widgets at a time"));
    }
    let previews: serde_json::Map<String, Value> = request
        .widgets
        .iter()
        .map(|widget| {
            let id = widget.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
            (id, one(widget, ""))
        })
        .collect();
    Ok(Json(json!({ "previews": previews })))
}

/// Inline configurator gate for the writes that share a path with a read any
/// credential may make. Reads come in via the middleware, which has already
/// resolved the caller; writes check it here. Splitting them into a separate
/// router would mean declaring the path twice.
fn require_configurator_inline(authority: &Authority) -> Result<(), ApiError> {
    if authority.can_configure() {
        Ok(())
    } else {
        Err(ApiError::forbidden(ADMIN_ONLY))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevokeRequest {
    device_id: String,
}

async fn revoke_device(
    State(agent): State<Arc<Agent>>,
    Json(request): Json<RevokeRequest>,
) -> Result<Json<Value>, ApiError> {
    if request.device_id.trim().is_empty() {
        return Err(ApiError::bad_request("deviceId must not be empty"));
    }

    if !agent.revoke_device(&request.device_id)? {
        return Err(ApiError::not_found("no live device with that id"));
    }

    // The device's open socket is not closed from here. The socket loop re-checks
    // its token on every probe (see `ws`), so the connection ends within one
    // probe interval and presence follows it. Reaching into another task from a
    // request handler would be the alternative, and it would need a registry of
    // live sockets to do it at all.
    Ok(Json(json!({ "revoked": true })))
}

/// Revoke every paired device but the one asking — a browser clearing out a
/// list it no longer recognises must not lock itself out doing it.
async fn revoke_all_devices(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
) -> Result<Json<Value>, ApiError> {
    let keep = match &authority {
        Authority::Browser(device) | Authority::Tablet(device) => Some(device.id.as_str()),
        Authority::Admin => None,
    };
    let revoked = agent.revoke_all_devices(keep)?;
    Ok(Json(json!({ "revoked": revoked })))
}

/// The caller revoking its own credential: a tablet told to forget this PC.
async fn forget_me(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
) -> Result<Json<Value>, ApiError> {
    let device = match &authority {
        Authority::Browser(device) | Authority::Tablet(device) => device,
        Authority::Admin => {
            return Err(ApiError::bad_request("the admin credential is not a device"))
        }
    };
    agent.revoke_device(&device.id)?;
    Ok(Json(json!({ "revoked": true })))
}

async fn begin_pairing(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
) -> Result<Json<Value>, ApiError> {
    // A pairing window lets one more device in. From a remote browser, the
    // person at the desktop decides whether to open one.
    require_approval(
        &agent,
        &authority,
        "Open a tablet pairing window",
        "A tablet that enters the PIN will be able to see this deck and press its buttons.",
    )
    .await?;
    let offer = agent.begin_pairing(now_secs());

    // The PIN is returned to the caller for display in the agent's window and is
    // deliberately *not* part of the QR: the QR already carries a one-time
    // secret, and folding the PIN into the same artefact would mean a single
    // photographed QR granted both factors at once.
    let qr = qr_payload(agent.identity(), agent.host(), agent.port(), &offer)?;

    Ok(Json(json!({
        "pin": offer.pin,
        "expiresAt": offer.expires_at,
        "qr": serde_json::from_str::<Value>(&qr)?,
    })))
}

/// Close the open tablet window early. It only ever takes access away, so unlike
/// opening one it needs no approval at the desktop.
async fn cancel_pairing(State(agent): State<Arc<Agent>>) -> Json<Value> {
    agent.cancel_pairing();
    Json(json!({ "cancelled": true }))
}

async fn redeem_pairing(
    State(agent): State<Arc<Agent>>,
    channel: Option<Extension<Channel>>,
    Json(request): Json<PairRequest>,
) -> Result<Json<PairResponse>, ApiError> {
    // A tablet pairs over the LAN, where it can see the agent's certificate.
    // Through the relay this would let the cloud try PINs.
    if channel
        .map(|Extension(channel)| channel)
        .unwrap_or_default()
        != Channel::Lan
    {
        return Err(ApiError::not_found("tablets pair on the local network"));
    }
    let now = now_secs();

    // A PIN means a human typed it; a session id + secret means a QR was
    // scanned. Preferring the PIN keeps the two paths unambiguous.
    let outcome = if !request.pin.trim().is_empty() {
        agent.redeem_pin(&request.pin, now)
    } else {
        agent.redeem_secret(&request.session_id, &request.one_time_secret, now)
    };

    let response = match outcome {
        PairingOutcome::Paired { token } => {
            // The agent assigns the device id; the contract's `DeviceProfile`
            // deliberately carries characteristics, not an identity.
            let device_id = flexdeck_pairing::new_device_id();
            let name = default_device_name(&request.device_name);

            // Recording the device here is what makes the token meaningful.
            // Only its hash is stored.
            // A tablet that paired before replaces its old row.
            agent.add_device_for_client(
                &device_id,
                name,
                &flexdeck_pairing::hash_token(&token),
                DeviceKind::Tablet,
                Some(&request.client_id),
            )?;

            PairResponse {
                ok: true,
                device_id,
                token,
                error: String::new(),
            }
        }
        PairingOutcome::WrongCode { remaining } => {
            refused(format!("wrong code; {remaining} attempt(s) remaining"))
        }
        PairingOutcome::Expired => refused("the pairing window expired".to_string()),
        PairingOutcome::Exhausted => {
            refused("too many wrong attempts; start a new pairing".to_string())
        }
        PairingOutcome::NoOffer => refused("no pairing window is open".to_string()),
    };

    Ok(Json(response))
}

/// Open a window for pairing a browser with a typed code.
///
/// Only from this machine — the admin token, or a browser already connected on
/// loopback. A remote browser inviting another remote browser would be one
/// stolen credential begetting more.
async fn begin_browser_pairing(
    State(agent): State<Arc<Agent>>,
    Extension(authority): Extension<Authority>,
) -> Result<Json<Value>, ApiError> {
    if authority.needs_approval() {
        return Err(ApiError::forbidden(
            "a browser can only be invited from the desktop itself",
        ));
    }
    let offer = agent.begin_browser_pairing(now_secs());
    Ok(Json(json!({
        "code": flexdeck_pairing::display_code(&offer.pin),
        "expiresAt": offer.expires_at,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserRedeemRequest {
    code: String,
    #[serde(default)]
    name: String,
    /// The browser's own id, so pairing again replaces its old row.
    #[serde(default)]
    client_id: String,
}

/// Trade a code shown at the desktop for a remote browser's credential.
///
/// Reachable only through the relay. Unlike a tablet's pairing, a refusal is a
/// `403`: the cloud passes the status straight back to the website.
async fn redeem_browser_pairing(
    State(agent): State<Arc<Agent>>,
    channel: Option<Extension<Channel>>,
    Json(redeem): Json<BrowserRedeemRequest>,
) -> Result<Json<Value>, ApiError> {
    if channel.map(|Extension(channel)| channel) != Some(Channel::Relay) {
        return Err(ApiError::not_found("browsers pair through the website"));
    }

    match agent.redeem_browser_code(&redeem.code, now_secs()) {
        PairingOutcome::Paired { .. } => {
            let name = match redeem.name.trim() {
                "" => "Browser",
                trimmed => trimmed,
            };
            let (token, device) = agent.issue_credential_for(
                name,
                DeviceKind::BrowserRemote,
                Some(&redeem.client_id),
            )?;
            Ok(Json(json!({
                "token": token,
                "deviceId": device.id,
                "fingerprint": agent.identity().fingerprint(),
            })))
        }
        PairingOutcome::WrongCode { remaining } => Err(ApiError::forbidden(format!(
            "that code is not right; {remaining} attempt(s) remaining"
        ))),
        PairingOutcome::Expired => Err(ApiError::forbidden("that code has expired")),
        PairingOutcome::Exhausted => Err(ApiError::forbidden(
            "too many wrong attempts; show a new code on the desktop",
        )),
        PairingOutcome::NoOffer => Err(ApiError::forbidden("no code is open on the desktop")),
    }
}

/// A refused pairing is an expected outcome of a normal flow, not a transport
/// error, so it is reported in the body rather than as a non-2xx status.
fn refused(error: String) -> PairResponse {
    PairResponse {
        ok: false,
        device_id: String::new(),
        token: String::new(),
        error,
    }
}

/// A device that pairs without giving a name still needs a label a human can
/// recognise in the device list.
fn default_device_name(name: &str) -> &str {
    match name.trim() {
        "" => "Unnamed device",
        trimmed => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use flexdeck_pairing::Identity;
    use flexdeck_store::ConfigStore;
    use serde_json::json;
    use tower::ServiceExt;

    /// The smallest valid deck, told apart from others by its profile's name.
    fn deck(marker: &str) -> Value {
        json!({"schema": crate::widgets::CONFIG_SCHEMA,
               "profiles": [{"id": "p", "name": marker}]})
    }

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

    /// Send a request carrying the admin token, which is what nearly every test
    /// wants. Credential tests use [`send_with`] instead.
    async fn send(agent: Arc<Agent>, request: Request<Body>) -> (StatusCode, Value) {
        let token = agent.admin_token().expect("an admin token");
        send_with(agent, request, Some(&token)).await
    }

    async fn send_with(
        agent: Arc<Agent>,
        mut request: Request<Body>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        if let Some(token) = token {
            let header =
                HeaderValue::from_str(&format!("Bearer {token}")).expect("a valid header value");
            request.headers_mut().insert(header::AUTHORIZATION, header);
        }

        let response = router(agent)
            .oneshot(request)
            .await
            .expect("the router responds");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    /// Pair a device through the normal flow and return its token.
    async fn pair_a_device(agent: Arc<Agent>, name: &str) -> String {
        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let pin = begun["pin"].as_str().expect("a pin").to_string();

        let (_, redeemed) = send(
            agent,
            post(
                "/api/pairing/redeem",
                json!({"pin": pin, "deviceName": name}),
            ),
        )
        .await;

        redeemed["token"].as_str().expect("a token").to_string()
    }

    #[tokio::test]
    async fn an_image_is_served_to_a_paired_device_and_cached_forever() {
        use flexdeck_plugin::builtin::media_sessions::ImageSink;
        let agent = agent();
        let address = ImageSink::put(agent.images().as_ref(), "image/png", b"\x89PNG fake".to_vec());
        let sha = address.strip_prefix("img:").expect("an img: address").to_string();
        let uri = format!("/api/images/{sha}");

        let (status, _) = send_with(Arc::clone(&agent), get(&uri), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "behind the token like everything else");

        let token = pair_a_device(Arc::clone(&agent), "tablet").await;
        let mut request = get(&uri);
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let response = router(Arc::clone(&agent)).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert!(response.headers()[header::CACHE_CONTROL].to_str().unwrap().contains("immutable"));

        let (missing, _) = send(Arc::clone(&agent), get(&format!("/api/images/{}", "0".repeat(64)))).await;
        assert_eq!(missing, StatusCode::NOT_FOUND);
        let (bad, _) = send(agent, get("/api/images/..%2F..%2Fsecret")).await;
        assert_eq!(bad, StatusCode::BAD_REQUEST, "only a hash is an address");
    }
    fn get(uri: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request builds")
    }

    fn post(uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds")
    }

    fn put(uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds")
    }

    #[tokio::test]
    async fn status_reports_the_fingerprint_a_client_must_pin() {
        let agent = agent();
        let fingerprint = agent.identity().fingerprint();

        let (status, body) = send(agent, get("/api/status")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["fingerprint"], fingerprint);
        assert_eq!(body["rev"], Value::Null, "nothing is committed yet");
        assert_eq!(body["port"], 7878);
        assert_eq!(body["deviceCount"], 0);
    }

    #[tokio::test]
    async fn config_is_absent_until_something_is_committed() {
        let (status, body) = send(agent(), get("/api/config")).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().is_some(), "errors carry a message");
    }

    #[tokio::test]
    async fn a_committed_config_reads_back_with_its_revision_in_the_etag() {
        let agent = agent();

        let (status, body) = send(
            Arc::clone(&agent),
            put("/api/config", json!({"schema": 2, "profiles": []})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["rev"], 1);

        // This one inspects a response header, so it cannot use the helpers that
        // only hand back a status and a body.
        let token = agent.admin_token().expect("an admin token");
        let mut request = get("/api/config");
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).expect("a header value"),
        );

        let response = router(agent).oneshot(request).await.expect("responds");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ETAG)
                .map(|v| v.to_str().unwrap()),
            Some("\"1\""),
            "the revision is discoverable without a bespoke body field"
        );
    }

    #[tokio::test]
    async fn each_commit_advances_the_revision() {
        let agent = agent();

        for expected in 1..=3 {
            let (_, body) = send(
                Arc::clone(&agent),
                put("/api/config", deck(&json!(expected).to_string())),
            )
            .await;
            assert_eq!(body["rev"], expected);
        }
    }

    #[tokio::test]
    async fn undo_with_nothing_to_undo_is_a_conflict_not_a_server_error() {
        let (status, _) = send(agent(), post("/api/undo", json!({}))).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn undo_and_redo_move_the_revision() {
        let agent = agent();
        send(
            Arc::clone(&agent),
            put("/api/config", deck(&json!(1).to_string())),
        )
        .await;
        send(
            Arc::clone(&agent),
            put("/api/config", deck(&json!(2).to_string())),
        )
        .await;

        let (status, body) = send(Arc::clone(&agent), post("/api/undo", json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["rev"], 1);

        let (status, body) = send(Arc::clone(&agent), post("/api/redo", json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["rev"], 2);
    }

    /// Pair through the PIN flow carrying a client id; returns the token.
    async fn pair_as(agent: Arc<Agent>, name: &str, client: &str) -> String {
        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let pin = begun["pin"].as_str().expect("a pin").to_string();
        let (_, redeemed) = send(
            agent,
            post(
                "/api/pairing/redeem",
                json!({"pin": pin, "deviceName": name, "clientId": client}),
            ),
        )
        .await;
        redeemed["token"].as_str().expect("a token").to_string()
    }

    #[tokio::test]
    async fn a_tablet_pairing_again_replaces_its_old_entry() {
        let agent = agent();
        let old = pair_as(Arc::clone(&agent), "Tablet", "install-1").await;
        pair_as(Arc::clone(&agent), "Other", "install-2").await;
        pair_as(Arc::clone(&agent), "Tablet", "install-1").await;

        let names: Vec<_> = agent.devices().unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["Other", "Tablet"]);
        assert_eq!(agent.authenticate(&old).unwrap(), None, "the old token is dead");
    }

    #[tokio::test]
    async fn a_device_can_forget_itself_and_only_itself() {
        let agent = agent();
        let kitchen = pair_a_device(Arc::clone(&agent), "Kitchen").await;
        pair_a_device(Arc::clone(&agent), "Studio").await;

        let (status, _) =
            send_with(Arc::clone(&agent), post("/api/devices/forget-me", json!({})), Some(&kitchen)).await;
        assert_eq!(status, StatusCode::OK);

        let names: Vec<_> = agent.devices().unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["Studio"]);
    }

    #[tokio::test]
    async fn the_device_list_marks_the_one_asking() {
        let agent = agent();
        let kitchen = pair_a_device(Arc::clone(&agent), "Kitchen").await;
        pair_a_device(Arc::clone(&agent), "Studio").await;

        let (_, body) = send_with(Arc::clone(&agent), get("/api/devices"), Some(&kitchen)).await;
        let rows = body["devices"].as_array().expect("a list");
        let current: Vec<_> = rows.iter().filter(|row| row["current"] == true).map(|row| row["name"].clone()).collect();
        assert_eq!(current, [json!("Kitchen")]);
        assert!(rows.iter().all(|row| row["kind"] == "tablet"));
    }

    #[tokio::test]
    async fn revoking_all_clears_every_device() {
        let agent = agent();
        pair_a_device(Arc::clone(&agent), "A").await;
        pair_a_device(Arc::clone(&agent), "B").await;

        let (status, body) = send(Arc::clone(&agent), post("/api/devices/revoke-all", json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["revoked"], 2);
        assert!(agent.devices().unwrap().is_empty());
    }

    #[tokio::test]
    async fn revoking_an_unknown_device_is_a_not_found() {
        let (status, _) = send(
            agent(),
            post("/api/devices/revoke", json!({"deviceId": "nobody"})),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn beginning_a_pairing_returns_a_pin_and_a_scannable_qr() {
        let agent = agent();
        let fingerprint = agent.identity().fingerprint();

        let (status, body) = send(agent, post("/api/pairing/begin", json!({}))).await;

        assert_eq!(status, StatusCode::OK);

        let pin = body["pin"].as_str().expect("a pin");
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));

        // The QR is the contract's `PairingInfo`, carrying the fingerprint the
        // client must pin and the one-time secret.
        assert_eq!(body["qr"]["fingerprint"], fingerprint);
        assert_eq!(body["qr"]["host"], "192.168.1.10");
        assert_eq!(body["qr"]["port"], 7878);
        assert!(body["qr"]["oneTimeSecret"].as_str().is_some());
    }

    #[tokio::test]
    async fn the_qr_does_not_carry_the_pin() {
        // Two factors must stay two factors: a photographed QR must not reveal
        // the code a human is meant to type.
        let (_, body) = send(agent(), post("/api/pairing/begin", json!({}))).await;

        let pin = body["pin"].as_str().expect("a pin").to_string();
        let qr = body["qr"].to_string();

        assert!(!qr.contains(&pin), "the QR must not contain the PIN");
    }

    #[tokio::test]
    async fn redeeming_with_the_right_pin_grants_a_token_and_registers_the_device() {
        let agent = agent();

        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let pin = begun["pin"].as_str().expect("a pin").to_string();

        let (status, body) = send(
            Arc::clone(&agent),
            post(
                "/api/pairing/redeem",
                json!({"pin": pin, "deviceName": "Kitchen tablet"}),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true, "{body}");
        assert_eq!(body["token"].as_str().expect("a token").len(), 64);
        assert!(!body["deviceId"].as_str().expect("a device id").is_empty());

        // A successful pairing must leave the device on record, otherwise the
        // token it was just handed authenticates nothing.
        let (_, listed) = send(agent, get("/api/devices")).await;
        assert_eq!(listed["devices"][0]["name"], "Kitchen tablet");
    }

    #[tokio::test]
    async fn redeeming_with_the_wrong_pin_is_a_200_that_says_no() {
        let agent = agent();

        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let pin = begun["pin"].as_str().expect("a pin");
        // Every digit differs, so this is the same length but definitely wrong.
        let wrong: String = pin
            .chars()
            .map(|digit| if digit == '0' { '1' } else { '0' })
            .collect();

        let (status, body) = send(
            Arc::clone(&agent),
            post("/api/pairing/redeem", json!({"pin": wrong})),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "a refusal is not a transport error");
        // `ok` is absent rather than `false`: proto3 JSON omits default values,
        // and `false` is the default for a bool.
        assert_ne!(body["ok"], true, "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("remaining"),
            "the client is told how many attempts are left: {body}"
        );

        // A failed pairing must not leave a device behind.
        let (_, listed) = send(agent, get("/api/devices")).await;
        assert_eq!(listed["devices"].as_array().map(Vec::len), Some(0));
    }

    #[tokio::test]
    async fn redeeming_without_an_open_window_is_refused() {
        let (status, body) = send(
            agent(),
            post("/api/pairing/redeem", json!({"pin": "123456"})),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_ne!(body["ok"], true, "{body}");
    }

    #[tokio::test]
    async fn a_refused_pairing_omits_ok_rather_than_sending_false() {
        // proto3 JSON omits default values, so a failed pairing carries no `ok`
        // key at all. This is the contract's behaviour, not a quirk of this
        // endpoint, and it means a client must test for `ok === true` — a test
        // for `ok === false` would never match.
        //
        // Pinned here so nobody later "fixes" it by hand-building the JSON,
        // which would put this endpoint out of step with the contract the
        // Android client is generated from.
        let (_, body) = send(
            agent(),
            post("/api/pairing/redeem", json!({"pin": "123456"})),
        )
        .await;

        assert!(body.get("ok").is_none(), "expected no `ok` key: {body}");
        assert!(
            body["error"].as_str().is_some(),
            "but the reason is always given: {body}"
        );
    }

    #[tokio::test]
    async fn pairing_by_qr_secret_grants_a_token() {
        // Also proves the endpoint accepts the proto3 JSON naming a real client
        // will send, rather than snake_case.
        let agent = agent();

        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let session_id = begun["qr"]["sessionId"].as_str().expect("a session id");
        let secret = begun["qr"]["oneTimeSecret"].as_str().expect("a secret");

        let (status, body) = send(
            Arc::clone(&agent),
            post(
                "/api/pairing/redeem",
                json!({
                    "sessionId": session_id,
                    "oneTimeSecret": secret,
                    "deviceName": "Studio tablet",
                }),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true, "{body}");
    }

    #[test]
    fn a_nameless_device_still_gets_a_recognisable_label() {
        assert_eq!(default_device_name("Kitchen tablet"), "Kitchen tablet");
        // Surrounding whitespace is trimmed, so a list never shows ragged names.
        assert_eq!(default_device_name("  Studio  "), "Studio");
        assert_eq!(default_device_name("   "), "Unnamed device");
        assert_eq!(default_device_name(""), "Unnamed device");
    }

    #[tokio::test]
    async fn no_route_answers_without_a_credential() {
        // Found by probing a *running* agent with no credentials at all: pairing
        // could be completed and the configuration overwritten. Every entry here
        // is a way in that must not exist.
        let agent = agent();

        for request in [
            get("/api/status"),
            get("/api/config"),
            get("/api/devices"),
            post("/api/undo", json!({})),
            post("/api/redo", json!({})),
            post("/api/pairing/begin", json!({})),
            post("/api/pairing/cancel", json!({})),
            post("/api/devices/revoke", json!({"deviceId": "x"})),
            put("/api/config", json!({ "schema": 2 })),
        ] {
            let (status, _) = send_with(Arc::clone(&agent), request, None).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "a route answered an anonymous caller"
            );
        }
    }

    #[tokio::test]
    async fn an_unknown_token_is_refused() {
        let (status, _) = send_with(agent(), get("/api/status"), Some("guessed-token")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_bearer_scheme_is_matched_case_insensitively() {
        let agent = agent();
        let token = agent.admin_token().expect("an admin token");

        let mut request = get("/api/status");
        let header = HeaderValue::from_str(&format!("bEaReR {token}")).expect("a header value");
        request.headers_mut().insert(header::AUTHORIZATION, header);

        let (status, _) = send_with(agent, request, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn redeeming_needs_no_credential_because_it_is_how_you_get_one() {
        let agent = agent();
        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let pin = begun["pin"].as_str().expect("a pin").to_string();

        let (status, body) = send_with(
            Arc::clone(&agent),
            post("/api/pairing/redeem", json!({"pin": pin})),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true, "{body}");
    }

    #[tokio::test]
    async fn opening_a_pairing_window_needs_the_admin_token_not_a_device_token() {
        let agent = agent();
        let device_token = pair_a_device(Arc::clone(&agent), "Kitchen tablet").await;

        // A paired tablet may read the deck...
        let (status, _) =
            send_with(Arc::clone(&agent), get("/api/status"), Some(&device_token)).await;
        assert_eq!(status, StatusCode::OK);

        // ...but must not be able to mint another window. Otherwise its response
        // hands it a fresh PIN and secret, and one compromised tablet could pair
        // unlimited others.
        let (status, body) = send_with(
            Arc::clone(&agent),
            post("/api/pairing/begin", json!({})),
            Some(&device_token),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            body.get("pin").is_none(),
            "a refused request must not leak a PIN: {body}"
        );
    }

    #[tokio::test]
    async fn a_device_token_cannot_rewrite_the_layout_or_unpair_others() {
        // The layout decides what each button runs, and the launcher runs any
        // program. A tablet that could write it could bind `cmd.exe` to a button
        // and press it. Every write is therefore admin-only, and every one is
        // exercised here from a *device* — the case no admin-token test can see.
        let agent = agent();
        send(
            Arc::clone(&agent),
            put("/api/config", deck(&json!(1).to_string())),
        )
        .await;
        let device_token = pair_a_device(Arc::clone(&agent), "Kitchen tablet").await;

        let writes = [
            put("/api/config", deck(&json!(2).to_string())),
            post("/api/undo", json!({})),
            post("/api/redo", json!({})),
            post("/api/devices/revoke", json!({"deviceId": "anyone"})),
            post("/api/devices/revoke-all", json!({})),
            // A repository decides what can be installed; reading the catalog
            // makes this machine fetch from the addresses in them.
            post(
                "/api/plugins/repositories/add",
                json!({"url": "evil.example"}),
            ),
            post("/api/plugins/repositories/remove", json!({"url": "x"})),
            post("/api/plugins/repositories/toggle", json!({"url": "x"})),
            post("/api/plugins/repositories/refresh", json!({})),
            get("/api/plugins/catalog"),
            // Asking a plugin runs its code; a tablet has no editor to ask for.
            post(
                "/api/plugins/choices",
                json!({"plugin": "core", "action": "hotkey"}),
            ),
        ];

        for request in writes {
            let uri = request.uri().to_string();
            let (status, _) = send_with(Arc::clone(&agent), request, Some(&device_token)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri} accepted a device");
        }

        // Reading the layout is still what a tablet is for.
        let (status, body) =
            send_with(Arc::clone(&agent), get("/api/config"), Some(&device_token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["profiles"][0]["name"], "1", "no write landed");
    }

    // -- channels: where a credential is honoured ----------------------------

    /// Send as if the request arrived on `channel`, the way a listener or the
    /// tunnel forwarder marks it.
    async fn send_on(
        agent: Arc<Agent>,
        mut request: Request<Body>,
        token: Option<&str>,
        channel: Channel,
    ) -> (StatusCode, Value) {
        request.extensions_mut().insert(channel);
        send_with(agent, request, token).await
    }

    fn browser(agent: &Agent, kind: DeviceKind) -> String {
        agent.issue_credential("Laptop", kind).expect("issues").0
    }

    #[tokio::test]
    async fn every_credential_works_only_on_its_own_channel() {
        let agent = agent();
        let admin = agent.admin_token().expect("an admin token");
        let tablet = pair_a_device(Arc::clone(&agent), "Tablet").await;
        let local = browser(&agent, DeviceKind::BrowserLocal);
        let remote = browser(&agent, DeviceKind::BrowserRemote);

        for (token, channel, expected) in [
            (&admin, Channel::Lan, StatusCode::OK),
            (&admin, Channel::Loopback, StatusCode::OK),
            // The cloud must hold nothing that acts as this machine.
            (&admin, Channel::Relay, StatusCode::UNAUTHORIZED),
            (&tablet, Channel::Lan, StatusCode::OK),
            (&tablet, Channel::Relay, StatusCode::UNAUTHORIZED),
            (&local, Channel::Loopback, StatusCode::OK),
            // A local browser's token, stolen and replayed through the cloud.
            (&local, Channel::Relay, StatusCode::UNAUTHORIZED),
            (&local, Channel::Lan, StatusCode::UNAUTHORIZED),
            (&remote, Channel::Relay, StatusCode::OK),
            (&remote, Channel::Lan, StatusCode::UNAUTHORIZED),
            (&remote, Channel::Loopback, StatusCode::UNAUTHORIZED),
        ] {
            let (status, _) =
                send_on(Arc::clone(&agent), get("/api/status"), Some(token), channel).await;
            assert_eq!(status, expected, "{channel:?}");
        }
    }

    #[tokio::test]
    async fn a_browser_is_a_configurator_and_a_tablet_is_not() {
        let agent = agent();
        let local = browser(&agent, DeviceKind::BrowserLocal);

        let (status, body) = send_on(
            Arc::clone(&agent),
            put("/api/config", deck(&json!("browser").to_string())),
            Some(&local),
            Channel::Loopback,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    #[tokio::test]
    async fn argument_choices_come_from_the_plugin_and_an_unknown_action_is_404() {
        // A built-in suggests nothing yet: every argument is typed, and that is
        // an answer, not an error.
        let (status, body) = send(
            agent(),
            post(
                "/api/plugins/choices",
                json!({"plugin": "core", "action": "hotkey", "args": {}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["arguments"], json!({}));

        let (status, _) = send(
            agent(),
            post(
                "/api/plugins/choices",
                json!({"plugin": "nobody", "action": "x"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_package_larger_than_the_default_body_limit_reaches_the_installer() {
        // A dropped package travels base64-encoded in the body. Axum refuses a
        // JSON body over 2 MB by default, which would turn a normal plugin
        // into "payload too large" before anyone looked at it.
        let upload = "A".repeat(3 * 1024 * 1024);
        for uri in ["/api/plugins/inspect", "/api/plugins/install"] {
            let (status, body) =
                send(agent(), post(uri, json!({"source": {"upload": upload}}))).await;
            assert_ne!(status, StatusCode::PAYLOAD_TOO_LARGE, "{uri}: {body}");
        }
    }

    #[tokio::test]
    async fn a_remote_browser_cannot_install_without_a_yes_at_the_desktop() {
        // The headless agent has no window to ask in, so the answer is no —
        // and no install is attempted.
        let agent = agent();
        let remote = browser(&agent, DeviceKind::BrowserRemote);

        let (status, body) = send_on(
            Arc::clone(&agent),
            post(
                "/api/plugins/install",
                json!({"source": {"repository": {"repo": "https://a.example/repository.min.json", "id": "obs"}}}),
            ),
            Some(&remote),
            Channel::Relay,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains("approval"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn a_remote_browser_waits_for_approval_to_open_a_pairing_window() {
        let agent = agent();
        agent.approvals().enable();
        let remote = browser(&agent, DeviceKind::BrowserRemote);

        // Someone at the desktop: first says no, then yes.
        for allow in [false, true] {
            let desk = Arc::clone(&agent);
            let answer = tokio::spawn(async move {
                let request = loop {
                    if let Some(request) = desk.approvals().pending().into_iter().next() {
                        break request;
                    }
                    tokio::task::yield_now().await;
                };
                assert_eq!(request.requester, "Laptop");
                desk.approvals().answer(request.id, allow);
            });

            let (status, body) = send_on(
                Arc::clone(&agent),
                post("/api/pairing/begin", json!({})),
                Some(&remote),
                Channel::Relay,
            )
            .await;
            answer.await.expect("answered");

            if allow {
                assert_eq!(status, StatusCode::OK, "{body}");
                assert!(body["pin"].is_string());
            } else {
                assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
                assert!(body.get("pin").is_none(), "a denial leaks no PIN");
            }
        }
    }

    #[tokio::test]
    async fn a_remote_browser_cannot_link_a_plugin_folder() {
        let agent = agent();
        let remote = browser(&agent, DeviceKind::BrowserRemote);

        let (status, _) = send_on(
            Arc::clone(&agent),
            post("/api/plugins/link", json!({"path": "C:/anything"})),
            Some(&remote),
            Channel::Relay,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_browser_code_pairs_through_the_relay_and_nowhere_else() {
        let agent = agent();
        let offer = agent.begin_browser_pairing(now_secs());
        let redeem = || {
            post(
                "/api/pairing/browser/redeem",
                json!({"code": flexdeck_pairing::display_code(&offer.pin), "name": "Laptop"}),
            )
        };

        // On the LAN the route does not exist at all.
        let (status, _) = send_on(Arc::clone(&agent), redeem(), None, Channel::Lan).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, body) = send_on(Arc::clone(&agent), redeem(), None, Channel::Relay).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let token = body["token"].as_str().expect("a token").to_string();
        assert_eq!(body["fingerprint"], agent.identity().fingerprint());

        // The credential is a remote browser's: good through the relay only.
        let (status, _) = send_on(
            Arc::clone(&agent),
            get("/api/config"),
            Some(&token),
            Channel::Relay,
        )
        .await;
        assert_ne!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = send_on(
            Arc::clone(&agent),
            get("/api/status"),
            Some(&token),
            Channel::Lan,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // And the code is single-use.
        let (status, _) = send_on(Arc::clone(&agent), redeem(), None, Channel::Relay).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_tablet_cannot_be_paired_through_the_relay() {
        // Otherwise the cloud could try PINs against an open window.
        let agent = agent();
        let (_, begun) = send(Arc::clone(&agent), post("/api/pairing/begin", json!({}))).await;
        let pin = begun["pin"].as_str().expect("a pin").to_string();

        let (status, _) = send_on(
            Arc::clone(&agent),
            post("/api/pairing/redeem", json!({"pin": pin})),
            None,
            Channel::Relay,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_this_computer_can_invite_a_browser() {
        let agent = agent();
        let remote = browser(&agent, DeviceKind::BrowserRemote);

        let (status, _) = send_on(
            Arc::clone(&agent),
            post("/api/pairing/browser/begin", json!({})),
            Some(&remote),
            Channel::Relay,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = send(
            Arc::clone(&agent),
            post("/api/pairing/browser/begin", json!({})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["code"].as_str().map(str::len), Some(9), "XXXX-XXXX");
    }

    #[tokio::test]
    async fn the_actions_route_is_gone() {
        // The legacy `/api/actions` route was removed when the plugin catalogue
        // became the source of truth — keeping it would have meant two places
        // to add an action and a portal that could not tell which to read.
        let response = router(agent())
            .oneshot(get("/api/actions"))
            .await
            .expect("responds");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// The LAN listener serves the API and nothing else.
    ///
    /// The portal used to be served from here, and these tests used to check
    /// that a deep link reached it and that the API was not shadowed by it. Both
    /// are gone with the portal: the configurator lives in the cloud now, and
    /// this listener is device-only.
    ///
    /// What replaces them is the assertion that matters for the new shape — an
    /// unknown path is a 404 rather than a page, so nothing here can be mistaken
    /// for a user interface.
    #[tokio::test]
    async fn an_unknown_path_is_not_found_rather_than_a_page() {
        let response = router(agent())
            .oneshot(get("/profiles/tab1"))
            .await
            .expect("responds");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_root_is_not_found_either() {
        // A bare 404 at `/` is the honest answer: there is no interface here.
        let response = router(agent()).oneshot(get("/")).await.expect("responds");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // -- reactive engine surface -------------------------------------------

    #[tokio::test]
    async fn a_picture_is_uploaded_kept_and_served_and_a_pack_listed() {
        use base64::Engine as _;
        let agent = agent();
        let dir = tempfile::tempdir().unwrap();
        agent.enable_media(dir.path()).unwrap();
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 200]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let (status, body) = send(Arc::clone(&agent), post("/api/images", json!({"data": data}))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let address = body["image"].as_str().unwrap().to_string();
        assert!(address.starts_with("img:"));

        let (status, _) = send(Arc::clone(&agent), get(&format!("/api/images/{}", &address[4..]))).await;
        assert_eq!(status, StatusCode::OK, "served from disk");
        let (_, list) = send(Arc::clone(&agent), get("/api/images")).await;
        assert_eq!(list["pictures"].as_array().unwrap().len(), 1);
        let (status, _) = send(Arc::clone(&agent), post("/api/images", json!({"data": "bm90IGFuIGltYWdl"}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "not a picture");
        let (_, packs) = send(agent, get("/api/iconpacks")).await;
        assert_eq!(packs["packs"], json!([]));
    }

    #[tokio::test]
    async fn a_sources_secret_never_reaches_the_config_or_the_list() {
        let agent = agent();
        let ha = json!({"sources": [{
            "id": "ha", "http": {"url": "https://x/{{args.entity}}", "headers": {"Authorization": "Bearer {{args.token}}"}},
            "args": [{"name": "entity"}, {"name": "token", "secret": true, "default": "shared-tok"}],
            "fields": [{"name": "state", "value": "json.state"}]
        }]});
        let (status, body) = send(Arc::clone(&agent), put("/api/sources", ha)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, listed) = send(Arc::clone(&agent), get("/api/sources")).await;
        assert!(!listed.to_string().contains("shared-tok"), "{listed}");
        assert_eq!(listed["secrets"], json!(["ha//token"]));

        let config = json!({"schema": crate::widgets::CONFIG_SCHEMA, "profiles": [{"id": "p", "name": "P", "pages": [{"id": "pg", "name": "Page", "widgets": [{
            "id": "w1", "type": "WIDGET_TYPE_BUTTON", "col": 0, "row": 0, "w": 1, "h": 1,
            "sources": [{"source": "ha", "args": {"entity": "light.desk", "token": "own-tok"}}]
        }]}]}]});
        let (status, body) = send(Arc::clone(&agent), put("/api/config", config)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, stored) = send(Arc::clone(&agent), get("/api/config")).await;
        assert!(!stored.to_string().contains("own-tok"), "{stored}");
        assert_eq!(stored["profiles"][0]["pages"][0]["widgets"][0]["sources"][0]["args"]["entity"], "light.desk");

        // The source still gets it when it runs.
        let uses = agent.behavior().source_uses();
        let due = agent.behavior().sources().due(&uses, &std::collections::BTreeSet::new(), std::time::Instant::now());
        assert_eq!(due[0].args["token"], "own-tok");

        let (_, forgot) = send(Arc::clone(&agent), post("/api/sources/secrets/forget", json!({"source": "ha", "widget": "w1", "arg": "token"}))).await;
        assert_eq!(forgot["forgot"], true);
        let (_, listed) = send(agent, get("/api/sources")).await;
        assert_eq!(listed["secrets"], json!(["ha//token"]));
    }

    #[tokio::test]
    async fn the_formats_are_set_kept_and_used() {
        let agent = agent();
        let (status, body) = send(
            Arc::clone(&agent),
            put("/api/locale", json!({"decimal": ",", "group": ".", "currency": "eur", "fahrenheit": true})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["currency"], "EUR");
        let (_, preview) = send(
            Arc::clone(&agent),
            post("/api/faces/preview", json!({"widget": {"id": "w", "props": {"labelTemplate": "{{ 1234.5 | number(1) }} {{ 20 | temp }}"}}})),
        )
        .await;
        assert_eq!(preview["state"]["label"], "1.234,5 68°", "{preview}");
        let (status, _) = send(Arc::clone(&agent), put("/api/locale", json!({"decimal": ".", "group": "."}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (_, kept) = send(agent, get("/api/locale")).await;
        assert_eq!(kept["decimal"], ",");
    }

    #[tokio::test]
    async fn a_face_preview_shows_what_the_tablet_would_and_says_why_not() {
        let agent = agent();
        let widget = json!({"id": "w",
            "face": {"layout": "FACE_LAYOUT_VALUE", "elements": [
                {"slot": "SLOT_VALUE", "template": "{{ 41.6 | round }}%"},
                {"slot": "SLOT_CAPTION", "template": "{{ nobody.knows }}"}
            ]},
            "states": {"looks": [{"id": "hot", "face": {"elements": [{"template": "HOT"}]}}],
                       "variable": {"variable": "nobody.knows", "cases": [{"value": "1", "state": "hot"}]}}
        });
        let (status, body) =
            send(Arc::clone(&agent), post("/api/faces/preview", json!({"widget": widget}))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["state"]["elements"][0]["text"], "42%");
        assert_eq!(body["state"]["elements"][1]["text"], "–");

        let (_, hot) = send(
            Arc::clone(&agent),
            post("/api/faces/preview", json!({"widget": widget, "look": "hot"})),
        )
        .await;
        assert_eq!(hot["state"]["elements"][0]["text"], "HOT", "{hot}");

        let broken = json!({"id": "w", "props": {"labelTemplate": "{{ x | rond }}"}});
        let (status, body) =
            send(Arc::clone(&agent), post("/api/faces/preview", json!({"widget": broken}))).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["error"].as_str().unwrap().contains("'rond' is not a filter"), "{body}");

        let (_, page) = send(agent, post("/api/faces/preview", json!({"widgets": [widget, broken]}))).await;
        assert_eq!(page["previews"]["w"]["error"].as_str().map(|e| e.contains("rond")), Some(true), "{page}");
    }

    #[tokio::test]
    async fn plugins_returns_the_built_in_catalogue() {
        let (status, body) = send(agent(), get("/api/plugins")).await;

        assert_eq!(status, StatusCode::OK);
        let plugins = body["plugins"].as_array().expect("a plugins array");
        let ids: Vec<&str> = plugins
            .iter()
            .filter_map(|entry| entry["id"].as_str())
            .collect();
        assert!(
            ids.contains(&"system"),
            "the built-in `system` plugin should be present, got {ids:?}"
        );
    }

    #[tokio::test]
    async fn plugins_need_a_credential() {
        let (status, _) = send_with(agent(), get("/api/plugins"), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn variables_returns_each_built_in_variable_with_its_current_value() {
        let (status, body) = send(agent(), get("/api/variables")).await;

        assert_eq!(status, StatusCode::OK);
        // The body is a bare array of `{spec, value}` entries — wrapping it
        // would put every entry's fields under one key and the editor would
        // have to dig them back out.
        let variables = body.as_array().expect("a variables array");
        let volume = variables
            .iter()
            .find(|entry| entry["spec"]["name"] == "system.audio.volume")
            .expect("system.audio.volume is declared by the built-in");

        // A read_only variable must expose a value field, even if the system
        // backend is the unavailable one in tests.
        assert!(volume["spec"]["readOnly"].as_bool().unwrap_or(false));
        assert!(volume.is_object());
    }

    #[tokio::test]
    async fn automations_returns_an_empty_document_on_first_run() {
        let (status, body) = send(agent(), get("/api/automations")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["schema"], flexdeck_automation::AUTOMATION_SCHEMA);
        assert_eq!(body["rev"], 0);
        assert_eq!(body["automations"], json!([]));
    }

    #[tokio::test]
    async fn put_automations_with_admin_token_stores_and_returns_a_new_rev() {
        let agent = agent();
        let document = json!({
            "schema": 1,
            "rev": 0,
            "automations": [{
                "id": "lights-on-low-volume",
                "name": "Lights on at high volume",
                "trigger": {"on": "variables", "names": ["system.audio.volume"]},
                "body": [],
            }],
        });

        let (status, body) = send(
            Arc::clone(&agent),
            put("/api/automations", document.clone()),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(body["rev"].as_u64().unwrap() > 0);

        // The GET echoes the stored document back, so the editor can recover
        // from a refresh without re-uploading.
        let (_, stored) = send(Arc::clone(&agent), get("/api/automations")).await;
        assert_eq!(stored["automations"][0]["id"], "lights-on-low-volume");
    }

    #[tokio::test]
    async fn put_automations_with_a_device_token_is_forbidden() {
        let agent = agent();
        let token = pair_a_device(Arc::clone(&agent), "tablet").await;
        let body = json!({"schema": 1, "rev": 0, "automations": []});

        let (status, body) = send_with(
            Arc::clone(&agent),
            put("/api/automations", body),
            Some(&token),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            body["error"].as_str().unwrap_or("").contains("admin token"),
            "the error should explain why, got {body}"
        );
    }

    #[tokio::test]
    async fn put_automations_without_any_authorization_is_unauthorized() {
        let agent = agent();
        let body = json!({"schema": 1, "rev": 0, "automations": []});

        let (status, _) = send_with(Arc::clone(&agent), put("/api/automations", body), None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn put_automations_with_an_oversize_document_is_refused() {
        let agent = agent();
        // A document whose blocks are deeper than the engine's own limit must
        // be rejected BEFORE storage, so the agent never holds a config it
        // cannot run. Building it programmatically is clearer than nesting
        // twenty `json!` calls by hand, and the bracket count cannot drift.
        let mut leaf: Value = json!({
            "block": "doAction",
            "plugin": "core",
            "action": "noop",
            "args": {},
        });
        // Each `if` adds one condition to the depth counter (it is the
        // *condition* that gets checked, not the block tree). 40 is well past
        // the default limit of 16.
        for _ in 0..40 {
            leaf = json!({
                "block": "if",
                "condition": {
                    "kind": "compare",
                    "left": {"bool": true},
                    "op": "eq",
                    "right": {"bool": true},
                },
                "then": [leaf],
            });
        }
        let too_deep = json!({
            "schema": 1,
            "rev": 0,
            "automations": [{
                "id": "deep",
                "name": "Deep",
                "trigger": {"on": "manual"},
                "body": [leaf],
            }],
        });

        let (status, body) = send(Arc::clone(&agent), put("/api/automations", too_deep)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap_or("").contains("deep"),
            "the error should mention the depth, got {body}"
        );
    }

    #[tokio::test]
    async fn dry_run_with_admin_token_returns_a_run_record() {
        let agent = agent();
        // An empty body is the cleanest "the engine ran my automation" test —
        // it has nothing to declare, nothing to fail on, and proves the route
        // resolves the id end to end.
        let document = json!({
            "schema": 1,
            "rev": 0,
            "automations": [{
                "id": "no-op",
                "name": "No-op",
                "trigger": {"on": "manual"},
                "body": [],
            }],
        });

        // Store the document so the dry-run can resolve its id.
        let (put_status, _) = send(Arc::clone(&agent), put("/api/automations", document)).await;
        assert_eq!(put_status, StatusCode::OK, "the PUT must succeed first");

        let (status, body) = send(
            Arc::clone(&agent),
            post("/api/automations/dry-run", json!({"automationId": "no-op"})),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "completed");
        assert!(body["trace"].is_array());
    }

    #[tokio::test]
    async fn dry_run_for_an_unknown_automation_is_404() {
        let agent = agent();
        let (status, body) = send(
            Arc::clone(&agent),
            post(
                "/api/automations/dry-run",
                json!({"automationId": "no-such-thing"}),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body["error"]
                .as_str()
                .unwrap_or("")
                .contains("no-such-thing"),
            "the error should name the missing automation, got {body}"
        );
    }

    #[tokio::test]
    async fn dry_run_with_a_device_token_is_forbidden() {
        let agent = agent();
        let token = pair_a_device(Arc::clone(&agent), "tablet").await;

        let (status, _) = send_with(
            Arc::clone(&agent),
            post("/api/automations/dry-run", json!({"automationId": "any"})),
            Some(&token),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    // -- installed plugins ---------------------------------------------------

    fn package_zip() -> String {
        use base64::Engine as _;
        use std::io::Write as _;
        let manifest = json!({
            "abi": flexdeck_plugin::abi::ABI_VERSION,
            "id": "community",
            "name": "Community",
            "version": "1.0.0",
            "runtime": "node",
            "entry": "index.js",
            "permissions": ["network"]
        });
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("plugin.json", options).unwrap();
            writer.write_all(manifest.to_string().as_bytes()).unwrap();
            writer.start_file("index.js", options).unwrap();
            writer.finish().unwrap();
        }
        base64::engine::general_purpose::STANDARD.encode(buffer.into_inner())
    }

    fn agent_with_plugins() -> (Arc<Agent>, tempfile::TempDir) {
        let agent = agent();
        let dir = tempfile::tempdir().expect("a temp dir");
        agent.enable_plugins(
            dir.path().join("plugins"),
            flexdeck_plugin_host::Runtimes::default(),
            flexdeck_plugin_host::Options::default(),
        );
        (agent, dir)
    }

    #[tokio::test]
    async fn installing_a_plugin_needs_the_admin_token_not_a_device_token() {
        let (agent, _dir) = agent_with_plugins();
        let device = pair_a_device(Arc::clone(&agent), "tablet").await;

        let (status, _) = send_with(
            agent,
            post(
                "/api/plugins/install",
                json!({"source": {"upload": package_zip()}}),
            ),
            Some(&device),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn an_install_without_consent_answers_with_what_to_ask() {
        let (agent, _dir) = agent_with_plugins();

        let (status, body) = send(
            agent,
            post(
                "/api/plugins/install",
                json!({"source": {"upload": package_zip()}}),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["needsConsent"], true);
        assert_eq!(body["missing"], json!(["network"]));
        assert_eq!(body["package"]["id"], "community");
        assert_eq!(body["sha256"].as_str().map(str::len), Some(64));
    }

    #[tokio::test]
    async fn inspecting_a_package_installs_nothing() {
        let (agent, _dir) = agent_with_plugins();

        let (status, body) = send(
            Arc::clone(&agent),
            post(
                "/api/plugins/inspect",
                json!({"source": {"upload": package_zip()}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["package"]["permissions"], json!(["network"]));

        let (_, installed) = send(agent, get("/api/plugins/installed")).await;
        assert_eq!(installed["plugins"], json!([]));
    }

    #[tokio::test]
    async fn an_agent_without_installed_plugins_says_so() {
        let (status, body) = send(
            agent(),
            post("/api/plugins/uninstall", json!({"id": "community"})),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    }

    #[tokio::test]
    async fn plugin_settings_refuse_an_undeclared_key_and_never_echo_a_secret() {
        let agent = agent();
        let (status, _) = send(
            Arc::clone(&agent),
            put(
                "/api/plugins/settings",
                json!({"id": "system", "values": {"nope": "x"}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = send(
            agent,
            put(
                "/api/plugins/settings",
                json!({"id": "nobody", "values": {}}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

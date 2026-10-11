//! The agent's shared state.
//!
//! Everything a request handler needs lives here: the configuration store, the
//! TLS identity, the pairing desk, the live-connection registry, and a channel
//! that announces configuration changes.
//!
//! The connection registry sits here for the same reason the pairing desk does:
//! both are server-side state about clients that the handlers need, and neither
//! is part of the deck. It is a distinct type ([`crate::presence::Presence`])
//! with its own tests rather than a map inlined here, so the counting and
//! cleanup rules are stated once.
//!
//! # Why the change channel carries a revision, not a patch
//!
//! It is tempting to broadcast the patch itself and let every client apply it.
//! That breaks the moment one client is a revision behind: it receives a patch
//! from *n-1* to *n* and cannot apply it, because it is holding *n-2*. Worse, a
//! slow client may miss notifications entirely — a broadcast channel drops
//! messages rather than queueing without bound.
//!
//! So the channel carries only "the configuration moved to revision *n*". Each
//! connection remembers its own revision and asks [`Agent::catch_up`] what it
//! needs. That way a client that missed three changes still gets one correct
//! answer, and the same [`crate::sync`] policy serves both reconnects and live
//! updates — there is only one code path to reason about.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use crate::presence::Presence;
use crate::settings::Settings;

use flexdeck_actions::{Executor, RecordingExecutor};
use flexdeck_automation::Limits;
use flexdeck_pairing::{Identity, PairingDesk, PairingOutcome, SessionOffer};
use flexdeck_plugin::{Notifier, PluginError, RecordingNotifier, Registry};
use flexdeck_store::DeviceKind;
use flexdeck_store::{ConfigStore, Device};
use flexdeck_vars::VariableStore;
use tokio::sync::broadcast;

use crate::access::{Approvals, Channel};
use crate::behavior::Behavior;
use crate::executor::{executor_owner_thread, ExecutorHandle};
use crate::sync::{catch_up, SyncError, SyncOutcome};

/// How many change notifications may be outstanding before a slow subscriber is
/// told it lagged. Lagging is safe — the subscriber re-derives what it needs —
/// so this can be small.
const CHANGE_BUFFER: usize = 32;

/// Metadata key every plugin's settings are stored under, as one JSON object
/// of `{pluginId: {key: value}}`. Kept for plugins that are not installed right
/// now, so reinstalling one keeps its configuration.
pub const PLUGIN_SETTINGS_KEY: &str = "plugins.settings";

/// Read [`PLUGIN_SETTINGS_KEY`]. A missing or unreadable value is no settings
/// rather than a failed start: the worst outcome is that a plugin asks again.
pub(crate) fn stored_plugin_settings(
    store: &ConfigStore,
) -> BTreeMap<String, BTreeMap<String, String>> {
    store
        .meta(PLUGIN_SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Write every plugin's stored settings back to [`PLUGIN_SETTINGS_KEY`].
fn persist_plugin_settings(
    store: &Mutex<ConfigStore>,
    all: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<(), flexdeck_store::StoreError> {
    let json = serde_json::to_string(all).expect("string maps always serialise");
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .set_meta(PLUGIN_SETTINGS_KEY, &json)
}

/// Opens a web address in the person's browser, for a plugin's sign-in page.
///
/// A capability rather than a call, for the same reason the executor is: a
/// test must not be able to open a browser on a developer's desktop. Until the
/// assembled agent installs one, a request is logged and goes nowhere.
pub type UrlOpener = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// A browser pairing code, as the cloud routes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserOffer {
    /// SHA-256 of the normalised code. The cloud never needs the code itself.
    pub code_hash: String,
    pub expires_at: i64,
}

/// The routing key the cloud files a browser code under.
pub fn browser_code_hash(code: &str) -> String {
    flexdeck_pairing::hash_token(&flexdeck_pairing::normalise_code(code))
}

/// Metadata key the portal's credential is stored under.
pub const ADMIN_TOKEN_KEY: &str = "admin_token";

pub use crate::access::Authority;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Store(#[from] flexdeck_store::StoreError),

    #[error(transparent)]
    Settings(#[from] crate::settings::SettingsError),

    #[error(transparent)]
    Sync(#[from] SyncError),

    #[error(transparent)]
    Widgets(#[from] crate::widgets::WidgetsError),

    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Plugin(#[from] PluginError),

    #[error(transparent)]
    Behavior(#[from] crate::behavior::BehaviorError),
}

/// A minimal valid configuration, used the first time the agent runs.
///
/// It exists so that a freshly paired tablet has something to render and the
/// socket handshake has something to send — not because an untouched deck is
/// useful. Building a real one is the editor's job; this is a starting point
/// that is visibly a placeholder rather than an empty grid, which is
/// indistinguishable from a failure.
pub const DEFAULT_CONFIG: &str = r#"{
  "schema": 2,
  "rev": 1,
  "profiles": [
    {
      "id": "default",
      "name": "Default",
      "grid": {
        "cols": 4, "rows": 3,
        "gapX": 8, "gapY": 8,
        "usableW": 1280, "usableH": 800
      },
      "pages": [
        {
          "id": "home",
          "name": "Home",
          "widgets": [
            {
              "id": "welcome",
              "type": "WIDGET_TYPE_BUTTON",
              "col": 0, "row": 0, "w": 2, "h": 1,
              "props": {"label": "Edit me"}
            }
          ]
        }
      ]
    }
  ]
}"#;

pub struct Agent {
    identity: Identity,
    host: String,
    port: u16,
    store: Arc<Mutex<ConfigStore>>,
    pairing: Mutex<PairingDesk>,
    /// The window for pairing a *browser* with a typed code. Separate from the
    /// tablet desk, so opening one never cancels the other.
    browser_pairing: Mutex<PairingDesk>,
    /// The open browser code, as the cloud should route it: the hash of the
    /// code and its expiry. The tunnel watches this and offers it upstream.
    browser_offer: tokio::sync::watch::Sender<Option<BrowserOffer>>,
    approvals: Approvals,
    presence: Arc<Presence>,
    changes: broadcast::Sender<u64>,
    /// Told when a device is paired or revoked. Connections are [`Presence`]'s.
    devices_changed: broadcast::Sender<()>,
    /// Whether a configurator is watching through the cloud. See [`crate::live`].
    remote_watch: crate::live::RemoteWatch,
    /// The executor lives on a dedicated thread; the agent talks to it
    /// through this handle. Holding it in a `Mutex` (the old shape) is what
    /// made a widget press deadlock when it triggered an automation that
    /// needed input — the new shape cannot re-enter, because the executor is
    /// not behind a lock the press path tries to re-acquire.
    executor: ExecutorHandle,
    executor_is_live: AtomicBool,
    variables: Arc<VariableStore>,
    behavior: Behavior,
    notifier: Arc<Mutex<Box<dyn Notifier>>>,
    backends_are_live: AtomicBool,
    /// Installed plugins. Absent until [`Agent::enable_plugins`] is called,
    /// which an assembled agent always does and a unit test usually does not.
    plugins: OnceLock<Arc<crate::plugins::PluginManager>>,
    /// Where installable plugins are listed. Set up with [`Self::plugins`].
    repositories: OnceLock<Arc<crate::repos::Repositories>>,
    /// Pictures plugins handed over, for widgets to show as `img:<sha256>`.
    images: Arc<crate::images::ImageStore>,
    /// "Go to page" requests, for the tablets.
    pages: Arc<crate::pages::PageBus>,
    /// Pictures and icon packs the person adds. Absent until [`Self::enable_media`].
    media: OnceLock<crate::media::Media>,
    /// See [`UrlOpener`]. `None` until [`Self::install_url_opener`].
    url_opener: Arc<Mutex<Option<UrlOpener>>>,
}

impl Agent {
    pub fn new(store: ConfigStore, identity: Identity, host: String, port: u16) -> Self {
        let (changes, _) = broadcast::channel(CHANGE_BUFFER);
        let store = Arc::new(Mutex::new(store));
        let variables = Arc::new(VariableStore::new());

        // The executor lives on its own thread. The handle the agent carries
        // is the only way anything outside that thread can drive a `perform`,
        // and a single channel removes the re-entrancy that used to deadlock
        // a widget press whose automation needed input.
        let (executor, _executor_thread) =
            executor_owner_thread(Box::new(RecordingExecutor::new()));

        let notifier: Arc<Mutex<Box<dyn Notifier>>> =
            Arc::new(Mutex::new(Box::new(RecordingNotifier::new())));

        // Build the registry with every built-in plugin. The system plugin's
        // backend is the honest `UnavailableSystem` here — a real one is
        // installed via `install_live_system_backend` at startup, exactly as
        // the executor is.
        let mut registry = Registry::new();
        // Settings first, so a plugin's very first call already sees them.
        for (id, values) in
            stored_plugin_settings(&store.lock().unwrap_or_else(PoisonError::into_inner))
        {
            registry.load_settings(&id, values);
        }
        let pages = Arc::new(crate::pages::PageBus::new(Arc::clone(&store)));
        let mut backends = flexdeck_plugin::builtin::Backends::unavailable();
        backends.pages = Some(Arc::clone(&pages) as Arc<dyn flexdeck_plugin::builtin::deck::PageControl>);
        for plugin in flexdeck_plugin::builtin::builtins(backends) {
            registry.register(plugin);
        }
        registry.declare_variables(&variables);
        let registry = Arc::new(Mutex::new(registry));

        let behavior = Behavior::new(
            Arc::clone(&store),
            Arc::clone(&variables),
            Arc::clone(&registry),
            executor.clone(),
            Arc::clone(&notifier),
            Limits::default(),
        );
        // The first-run document is empty; loading it is fine and gives the
        // cache a known shape.
        let _ = behavior.reload();
        behavior.load_user_vars();

        Self {
            identity,
            host,
            port,
            store,
            pairing: Mutex::new(PairingDesk::default()),
            browser_pairing: Mutex::new(PairingDesk::for_browsers()),
            browser_offer: tokio::sync::watch::channel(None).0,
            approvals: Approvals::default(),
            presence: Arc::new(Presence::new()),
            changes,
            devices_changed: broadcast::channel(CHANGE_BUFFER).0,
            remote_watch: crate::live::RemoteWatch::default(),
            executor,
            executor_is_live: AtomicBool::new(false),
            variables,
            behavior,
            notifier,
            backends_are_live: AtomicBool::new(false),
            plugins: OnceLock::new(),
            repositories: OnceLock::new(),
            images: Arc::new(crate::images::ImageStore::new()),
            pages,
            media: OnceLock::new(),
            url_opener: Arc::new(Mutex::new(None)),
        }
    }

    /// Keep pictures and icon packs under `dir`. Once; later calls are ignored.
    pub fn enable_media(&self, dir: &std::path::Path) -> std::io::Result<()> {
        if self.media.get().is_none() {
            let _ = self.media.set(crate::media::Media::new(dir)?);
        }
        Ok(())
    }

    /// Pictures and icon packs, when there is somewhere to keep them.
    pub fn media(&self) -> Option<&crate::media::Media> {
        self.media.get()
    }

    /// Where "go to page" requests are announced.
    pub fn pages(&self) -> &Arc<crate::pages::PageBus> {
        &self.pages
    }

    /// The images plugins have handed over.
    pub fn images(&self) -> &Arc<crate::images::ImageStore> {
        &self.images
    }

    /// Let plugins open web pages (an OAuth sign-in) in the real browser.
    pub fn install_url_opener(&self, opener: UrlOpener) {
        *self.url_opener.lock().unwrap_or_else(PoisonError::into_inner) = Some(opener);
    }

    /// Build an agent, advertising whichever address this machine would use to
    /// reach the network.
    ///
    /// Falls back to loopback when there is no usable route, which keeps the
    /// agent usable for same-machine testing. Check
    /// [`Self::is_reachable_from_a_device`] before showing a pairing QR.
    pub fn with_discovered_host(store: ConfigStore, identity: Identity, port: u16) -> Self {
        Self::new(store, identity, crate::net::advertised_host(), port)
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Who has a socket open right now.
    ///
    /// Deliberately separate from [`Self::devices`], which answers who has
    /// *paired*: a device that has redeemed a PIN may be switched off, and a
    /// window that conflated the two would say a tablet was connected when it
    /// had not been seen for a week.
    pub fn presence(&self) -> &Arc<Presence> {
        &self.presence
    }

    /// Read one of the agent's own settings from the store's metadata.
    ///
    /// These are not the *configuration*, which describes the deck. They answer
    /// how this installation is set up — the same reason `settings.rs` keeps them
    /// in metadata rather than in the timeline, where they would appear as
    /// document revisions.
    pub fn setting(&self, key: &str) -> Result<Option<String>, AgentError> {
        let store = self.store.lock().expect("the store lock is not poisoned");
        Ok(store.meta(key)?)
    }

    /// Write one of the agent's own settings.
    ///
    /// The store refuses to write its own revision key through this path, so a
    /// caller cannot rewind or skip the log by accident.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AgentError> {
        let store = self.store.lock().expect("the store lock is not poisoned");
        store.set_meta(key, value)?;
        Ok(())
    }

    /// Replace the whole persisted settings blob.
    ///
    /// The portal address lives in the SAME blob as the port and bind address, so
    /// it cannot go through `set_setting` — writing it as a bare key would leave
    /// two descriptions of one document, and the next `Settings::load` would
    /// silently discard it. Validation runs here, so an address the agent would
    /// refuse can never reach the disk.
    pub fn save_settings(&self, settings: &Settings) -> Result<(), AgentError> {
        let store = self.store.lock().expect("the store lock is not poisoned");
        settings.save(&store)?;
        Ok(())
    }

    /// Where clients should reach this agent, as advertised in the pairing QR.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Notified with the new revision after every configuration change.
    pub fn subscribe(&self) -> broadcast::Receiver<u64> {
        self.changes.subscribe()
    }

    pub fn rev(&self) -> Result<Option<u64>, AgentError> {
        Ok(self.store().rev()?)
    }

    pub fn config(&self) -> Result<Option<(u64, String)>, AgentError> {
        Ok(self.store().snapshot()?)
    }

    /// What a client at `from` needs in order to be current.
    pub fn catch_up(&self, from: Option<u64>) -> Result<SyncOutcome, AgentError> {
        Ok(catch_up(&self.store(), from)?)
    }

    /// Replace the configuration wholesale, returning the new revision.
    ///
    /// The document's own `rev` field is stamped here to match the revision the
    /// store assigns. Trusting whatever the client sent lets the two disagree,
    /// and a document claiming `rev: 1` while the agent is at revision 7 is
    /// worse than having no field at all — a client that read it would act on a
    /// number that stopped being true several edits ago.
    pub fn replace_config(&self, json: &str) -> Result<u64, AgentError> {
        // Tokens and keys a key's data source needs stay on this computer; the
        // config is what tablets read.
        let json = &self.behavior.take_secrets(json);
        // Refused here rather than discovered on a press: a flow that does not
        // parse, or a state pointing at a look that is not there, is a sentence
        // for the editor, not a dead button on the tablet.
        crate::widgets::validate(json, &flexdeck_automation::Limits::default())?;
        // One guard for the whole operation, so the predicted revision and the
        // committed one cannot diverge.
        let store = self.store();
        let predicted = store.next_rev()?;
        let stamped = stamp_rev(json, predicted)?;

        let committed = store.commit(&stamped)?;
        debug_assert_eq!(
            predicted, committed,
            "the stamped revision must be the committed one"
        );

        self.announce(committed);
        Ok(committed)
    }

    pub fn undo(&self) -> Result<u64, AgentError> {
        let (rev, _) = self.store().undo()?;
        self.announce(rev);
        Ok(rev)
    }

    pub fn redo(&self) -> Result<u64, AgentError> {
        let (rev, _) = self.store().redo()?;
        self.announce(rev);
        Ok(rev)
    }

    pub fn devices(&self) -> Result<Vec<Device>, AgentError> {
        Ok(self.store().devices()?)
    }

    pub fn revoke_device(&self, id: &str) -> Result<bool, AgentError> {
        let revoked = self.store().revoke_device(id)?;
        self.devices_moved();
        Ok(revoked)
    }

    /// Hear about devices being paired or revoked.
    pub fn subscribe_devices(&self) -> broadcast::Receiver<()> {
        self.devices_changed.subscribe()
    }

    /// Whether a configurator is watching through the cloud.
    pub fn remote_watch(&self) -> &crate::live::RemoteWatch {
        &self.remote_watch
    }

    /// No listeners is normal, not an error.
    fn devices_moved(&self) {
        let _ = self.devices_changed.send(());
    }

    /// Record a device that has just paired.
    ///
    /// Only the *hash* of the token is stored, so the database never holds a
    /// credential that would work on the wire.
    pub fn add_device(&self, id: &str, name: &str, token_hash: &str) -> Result<(), AgentError> {
        self.store().add_device(id, name, token_hash)?;
        self.devices_moved();
        Ok(())
    }

    /// Mint a credential of `kind` for a newly trusted client, returning the
    /// token (shown once) and the device it belongs to.
    pub fn issue_credential(
        &self,
        name: &str,
        kind: DeviceKind,
    ) -> Result<(String, Device), AgentError> {
        let token = flexdeck_pairing::generate_token();
        let id = format!(
            "{}-{}",
            kind.as_str(),
            &flexdeck_pairing::generate_token()[..12]
        );
        self.store()
            .add_device_of_kind(&id, name, &flexdeck_pairing::hash_token(&token), kind)?;
        self.devices_moved();
        let device = self
            .authenticate(&token)?
            .expect("a credential just stored authenticates");
        Ok((token, device))
    }

    /// Open a window for pairing a browser with a typed code.
    ///
    /// Also offers the code's hash to the cloud (through the tunnel, if web
    /// access is on), which is how a website that only knows the code finds
    /// this desktop.
    pub fn begin_browser_pairing(&self, now: i64) -> SessionOffer {
        let offer = self
            .browser_pairing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .begin(now);
        self.browser_offer.send_replace(Some(BrowserOffer {
            code_hash: browser_code_hash(&offer.pin),
            expires_at: offer.expires_at,
        }));
        offer
    }

    /// [`Self::begin_browser_pairing`] on the system clock.
    pub fn begin_browser_pairing_now(&self) -> SessionOffer {
        self.begin_browser_pairing(now_secs())
    }

    /// Close the browser pairing window without pairing anything, and stop the
    /// cloud routing its code here.
    pub fn cancel_browser_pairing(&self) {
        self.browser_pairing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancel();
        self.browser_offer.send_replace(None);
    }

    /// [`Self::browser_pairing_offer`] on the system clock.
    pub fn browser_pairing_offer_now(&self) -> Option<SessionOffer> {
        self.browser_pairing_offer(now_secs())
    }

    /// The browser pairing code currently open, if any.
    pub fn browser_pairing_offer(&self, now: i64) -> Option<SessionOffer> {
        self.browser_pairing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .current(now)
            .cloned()
    }

    /// Redeem a typed browser code. The window closes on success, on expiry, or
    /// once its attempts run out, and the cloud's routing entry with it.
    pub fn redeem_browser_code(&self, code: &str, now: i64) -> PairingOutcome {
        let outcome = self
            .browser_pairing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .redeem_pin(code, now);
        if !matches!(outcome, PairingOutcome::WrongCode { .. }) {
            self.browser_offer.send_replace(None);
        }
        outcome
    }

    /// Watch the browser code the cloud should route here.
    pub fn watch_browser_offer(&self) -> tokio::sync::watch::Receiver<Option<BrowserOffer>> {
        self.browser_offer.subscribe()
    }

    /// Requests from remote browsers waiting for a yes at the desktop.
    pub fn approvals(&self) -> &Approvals {
        &self.approvals
    }

    pub fn begin_pairing(&self, now: i64) -> SessionOffer {
        self.pairing().begin(now)
    }

    /// Open a pairing window using the system clock.
    ///
    /// The clock is read *here* rather than inside [`PairingDesk`], so expiry
    /// stays deterministically testable through [`Self::begin_pairing`] while a
    /// caller still has one obvious thing to call.
    pub fn begin_pairing_now(&self) -> SessionOffer {
        self.begin_pairing(now_secs())
    }

    /// Close the tablet pairing window without pairing anyone; its PIN stops
    /// working at once rather than when it would have expired.
    pub fn cancel_pairing(&self) {
        self.pairing().cancel();
    }

    pub fn redeem_pin(&self, pin: &str, now: i64) -> PairingOutcome {
        self.pairing().redeem_pin(pin, now)
    }

    pub fn redeem_secret(&self, session_id: &str, secret: &str, now: i64) -> PairingOutcome {
        self.pairing().redeem_secret(session_id, secret, now)
    }

    /// Look up the device behind a token.
    ///
    /// Only the hash is compared, so a copy of the database is not a copy of the
    /// credentials.
    pub fn authenticate(&self, token: &str) -> Result<Option<Device>, AgentError> {
        let hash = flexdeck_pairing::hash_token(token);
        Ok(self.store().authenticate(&hash)?)
    }

    /// The credential the portal authenticates with.
    ///
    /// Generated on first use and stored, so restarting the agent does not
    /// invalidate a portal session.
    pub fn admin_token(&self) -> Result<String, AgentError> {
        let store = self.store();

        if let Some(existing) = store.meta(ADMIN_TOKEN_KEY)? {
            return Ok(existing);
        }

        let token = flexdeck_pairing::generate_token();
        store.set_meta(ADMIN_TOKEN_KEY, &token)?;
        Ok(token)
    }

    /// Decide what a presented token may do.
    ///
    /// `None` means the token is not recognised — unknown, revoked, or simply
    /// wrong — and callers must treat that as a refusal, never a soft failure.
    ///
    /// `channel` is where the request came in: a credential is only honoured on
    /// its own channel (see [`crate::access`]), so a token used anywhere else is
    /// treated exactly like an unknown one.
    pub fn authorise(
        &self,
        token: &str,
        channel: Channel,
    ) -> Result<Option<Authority>, AgentError> {
        let admin = self.admin_token()?;

        // Constant-time: this is a long-lived secret compared on every request,
        // and a byte-at-a-time comparison would be measurable from outside.
        if flexdeck_pairing::constant_time_eq(token.as_bytes(), admin.as_bytes()) {
            return Ok(channel.accepts_admin().then_some(Authority::Admin));
        }

        Ok(self
            .authenticate(token)?
            .filter(|device| channel.accepts(device.kind))
            .map(|device| match device.kind {
                DeviceKind::Tablet => Authority::Tablet(device),
                DeviceKind::BrowserLocal | DeviceKind::BrowserRemote => Authority::Browser(device),
            }))
    }

    /// Someone touched `widget_id` with `gesture`: run what it does.
    ///
    /// The route is *the* place the jury flagged for a deadlock: a widget press
    /// that triggered an automation whose `DoAction` needed input used to try
    /// to re-lock a non-reentrant `Mutex`. The executor now lives on its own
    /// thread and the registry goes through the plugin path, so the press
    /// cannot re-enter anything it already holds.
    pub fn interact(
        &self,
        widget_id: &str,
        gesture: flexdeck_protocol::Gesture,
    ) -> Result<crate::behavior::Interaction, AgentError> {
        Ok(self.behavior.interact(widget_id, gesture)?)
    }

    /// [`Self::interact`] on a zone of the key's strip; empty: the key.
    pub fn interact_in(
        &self,
        widget_id: &str,
        gesture: flexdeck_protocol::Gesture,
        zone: &str,
    ) -> Result<crate::behavior::Interaction, AgentError> {
        Ok(self.behavior.interact_in(widget_id, gesture, zone)?)
    }

    /// The reactive engine. Exposed so the HTTP layer can read catalogues and
    /// the editor's "Test" button can dry-run an automation.
    pub fn variables(&self) -> &Arc<VariableStore> {
        &self.variables
    }

    pub fn behavior(&self) -> &Behavior {
        &self.behavior
    }

    /// Install the executor that actually performs actions.
    ///
    /// The default is [`RecordingExecutor`], which does nothing at all. That is
    /// a deliberate choice of failure mode: defaulting to real input injection
    /// would mean a test, or an agent built without this call, could type into
    /// whatever the user happens to have focused. Doing nothing is the safe way
    /// to be wrong.
    ///
    /// Named `install_live_executor` rather than `install_executor` so that
    /// passing something inert is harder to do by accident than not calling it
    /// at all: the name asserts that this really does things, and
    /// [`Self::has_live_executor`] records that someone asserted it.
    pub fn install_live_executor(&self, executor: Box<dyn Executor + Send>) {
        // The owner thread is alive for the agent's lifetime. If it ever is
        // not, that is a startup bug and the panic points at it.
        self.executor
            .install(executor)
            .expect("executor owner thread is alive while the agent is running");
        self.executor_is_live.store(true, Ordering::SeqCst);
    }

    /// Rebuild the built-in plugins on real operating-system backends, and
    /// show notifications for real.
    ///
    /// The same failure-mode choice as [`Self::install_live_executor`]: until
    /// this is called, volume reads say "not available" and notifications are
    /// recorded rather than shown, so a test or a forgotten call is inert
    /// rather than loud.
    pub fn install_live_backends(
        &self,
        mut backends: flexdeck_plugin::builtin::Backends,
        notifier: Box<dyn Notifier>,
    ) {
        // Album art goes where a tablet can fetch it.
        if backends.images.is_none() {
            backends.images = Some(Arc::clone(&self.images) as Arc<dyn flexdeck_plugin::builtin::media_sessions::ImageSink>);
        }
        if backends.pages.is_none() {
            backends.pages = Some(Arc::clone(&self.pages) as Arc<dyn flexdeck_plugin::builtin::deck::PageControl>);
        }
        {
            let mut registry = self
                .behavior
                .registry()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for plugin in flexdeck_plugin::builtin::builtins(backends) {
                registry.register_into(plugin, &self.variables);
            }
        }
        *self.notifier.lock().unwrap_or_else(PoisonError::into_inner) = notifier;
        self.backends_are_live.store(true, Ordering::SeqCst);
    }

    /// Start managing installed plugins kept under `root`: load every enabled
    /// one, watch linked ones for changes, and route what they send.
    ///
    /// Idempotent; the second call returns the first manager.
    pub fn enable_plugins(
        &self,
        root: std::path::PathBuf,
        runtimes: flexdeck_plugin_host::Runtimes,
        options: flexdeck_plugin_host::Options,
    ) -> Arc<crate::plugins::PluginManager> {
        if let Some(existing) = self.plugins.get() {
            return Arc::clone(existing);
        }
        let _ = self
            .repositories
            .set(Arc::new(crate::repos::Repositories::new(
                crate::repos::Sources::from_env(),
                root.join(".repos"),
                Arc::clone(&self.store),
            )));
        let (manager, events) = crate::plugins::PluginManager::new(
            root,
            Arc::clone(self.behavior.registry()),
            Arc::clone(&self.variables),
            Arc::clone(&self.store),
            runtimes,
            options,
        );
        let manager = Arc::new(manager);
        if self.plugins.set(Arc::clone(&manager)).is_err() {
            return Arc::clone(self.plugins.get().expect("set by the racing call"));
        }

        let behavior = self.behavior.clone();
        let pump = Arc::clone(&manager);
        let store = Arc::clone(&self.store);
        let images = Arc::clone(&self.images);
        let opener = Arc::clone(&self.url_opener);
        let _ = std::thread::Builder::new()
            .name("plugin-events".into())
            .spawn(move || {
                use flexdeck_plugin_host::HostEvent;
                for event in events {
                    match event {
                        HostEvent::Event {
                            plugin,
                            event,
                            payload,
                        } => behavior.on_plugin_event(&plugin, &event, &payload),
                        HostEvent::Notify {
                            plugin,
                            title,
                            body,
                        } => behavior.notify(&title, &body, &plugin),
                        HostEvent::Log {
                            plugin,
                            level,
                            message,
                        } => pump.record_log(&plugin, format!("[{level}] {message}")),
                        HostEvent::Changed { .. } => behavior.catalogue_changed(),
                        HostEvent::StoreSettings { plugin, values } => {
                            let merged = {
                                let mut registry = behavior
                                    .registry()
                                    .lock()
                                    .unwrap_or_else(PoisonError::into_inner);
                                registry
                                    .merge_settings(&plugin, values)
                                    .map(|()| registry.all_settings())
                            };
                            let saved = merged
                                .map_err(|error| error.to_string())
                                .and_then(|all| {
                                    persist_plugin_settings(&store, &all)
                                        .map_err(|error| error.to_string())
                                });
                            match saved {
                                Ok(()) => behavior.catalogue_changed(),
                                Err(error) => pump.record_log(
                                    &plugin,
                                    format!("[error] could not keep its settings: {error}"),
                                ),
                            }
                        }
                        HostEvent::OpenUrl { plugin, url } => {
                            let opened = match opener
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .as_ref()
                            {
                                Some(open) => open(&url),
                                None => Err("this build cannot open a browser".into()),
                            };
                            let line = match opened {
                                Ok(()) => format!("[info] opened {url} in the browser"),
                                Err(error) => format!("[warn] could not open {url}: {error}"),
                            };
                            pump.record_log(&plugin, line);
                        }
                        HostEvent::Image {
                            sha256,
                            mime,
                            bytes,
                            ..
                        } => images.put(&sha256, &mime, bytes),
                        HostEvent::Series { name, points, .. } => behavior.set_series(&name, points),
                    }
                }
            });

        manager.host_recipes(Arc::clone(self.behavior.sources()));
        manager.load_all();
        manager.watch_dev();
        self.behavior.catalogue_changed();
        manager
    }

    /// The plugin repositories, if [`Self::enable_plugins`] has run.
    pub fn repositories(&self) -> Option<&Arc<crate::repos::Repositories>> {
        self.repositories.get()
    }

    /// The plugin manager, if [`Self::enable_plugins`] has run.
    pub fn plugins(&self) -> Option<&Arc<crate::plugins::PluginManager>> {
        self.plugins.get()
    }

    /// Whether [`Self::install_live_backends`] has run.
    pub fn has_live_backends(&self) -> bool {
        self.backends_are_live.load(Ordering::SeqCst)
    }

    /// What the user set for a plugin, without defaults. Secrets included —
    /// this is the agent's own view; the API layer decides what leaves.
    pub fn plugin_settings(&self, plugin: &str) -> BTreeMap<String, String> {
        self.behavior
            .registry()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stored_settings(plugin)
    }

    /// Change a plugin's settings, tell the plugin, and persist them.
    ///
    /// A secret left blank keeps its stored value, so a settings form that
    /// never receives the password back can be saved without erasing it.
    pub fn set_plugin_settings(
        &self,
        plugin: &str,
        mut values: BTreeMap<String, String>,
    ) -> Result<(), AgentError> {
        let all = {
            let mut registry = self
                .behavior
                .registry()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !registry.contains(plugin) {
                return Err(AgentError::Plugin(PluginError::UnknownPlugin(
                    plugin.to_string(),
                )));
            }
            let previous = registry.stored_settings(plugin);
            let secrets: Vec<String> = registry
                .manifest(plugin)
                .map(|manifest| {
                    manifest
                        .settings
                        .iter()
                        .filter(|setting| setting.secret)
                        .map(|setting| setting.key.clone())
                        .collect()
                })
                .unwrap_or_default();
            for key in secrets {
                let blank = values.get(&key).is_none_or(|value| value.is_empty());
                if blank {
                    match previous.get(&key) {
                        Some(kept) => {
                            values.insert(key, kept.clone());
                        }
                        None => {
                            values.remove(&key);
                        }
                    }
                }
            }
            registry.set_settings(plugin, values)?;
            registry.all_settings()
        };
        persist_plugin_settings(&self.store, &all)?;
        self.behavior.catalogue_changed();
        Ok(())
    }

    /// Give the agent something to serve on its very first run — or after an
    /// upgrade past a configuration schema it no longer reads.
    ///
    /// A deck from an older schema is not deleted: the default is committed as
    /// a new revision on top of it, so it stays in the history. Returns whether
    /// it seeded anything, so a caller can tell the user rather than silently
    /// deciding their deck for them.
    pub fn seed_default_config(&self) -> Result<bool, AgentError> {
        if self.store().seed_if_empty(DEFAULT_CONFIG)?.is_some() {
            return Ok(true);
        }
        let readable = self.config()?.is_some_and(|(_, json)| {
            crate::widgets::validate(&json, &flexdeck_automation::Limits::default()).is_ok()
        });
        if readable {
            return Ok(false);
        }
        self.replace_config(DEFAULT_CONFIG)?;
        Ok(true)
    }

    /// Whether a live executor has been installed.
    ///
    /// A silent no-op in production is its own kind of bug, so the safe default
    /// is at least made *observable*: a shell can check this at startup rather
    /// than pretend that buttons work.
    pub fn has_live_executor(&self) -> bool {
        self.executor_is_live.load(Ordering::SeqCst)
    }

    /// Whether the advertised address could be reached by another device.
    ///
    /// A pairing QR carrying a loopback address scans perfectly and then fails
    /// to connect, which is a baffling thing to debug. This lets a shell warn
    /// *before* the user scans it.
    pub fn is_reachable_from_a_device(&self) -> bool {
        crate::net::is_reachable_host(&self.host)
    }

    /// Things worth telling the user before showing a pairing QR.
    ///
    /// `bind` is the address the caller intends to listen on, which the agent
    /// cannot know on its own. See [`crate::startup::check`] for why each warning
    /// matters and why none of them is an error.
    pub fn startup_warnings(&self, bind: SocketAddr) -> Vec<crate::startup::StartupWarning> {
        crate::startup::check(bind, &self.host, self.has_live_executor())
    }

    /// A send with no listeners is not an error — the agent may simply have no
    /// clients connected yet.
    fn announce(&self, rev: u64) {
        let _ = self.changes.send(rev);
    }

    // -- locking -------------------------------------------------------------
    //
    // A plain `std::sync::Mutex` is correct here because these guards are never
    // held across an `.await`: every critical section is pure synchronous work.
    // A panic while holding one would otherwise poison it for the lifetime of
    // the process, so poisoning is deliberately ignored — the store is a
    // database, and the next call will find it consistent or return an error of
    // its own.

    fn store(&self) -> MutexGuard<'_, ConfigStore> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn pairing(&self) -> MutexGuard<'_, PairingDesk> {
        self.pairing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Set a document's `rev` field to the revision it is about to be committed as.
///
/// A document that is not a JSON object is passed through untouched: the store
/// rejects it a moment later with a clearer error than anything this could
/// invent.
fn stamp_rev(json: &str, rev: u64) -> Result<String, AgentError> {
    let mut document: serde_json::Value = serde_json::from_str(json)?;

    if let Some(object) = document.as_object_mut() {
        object.insert("rev".to_string(), serde_json::Value::from(rev));
    }

    Ok(serde_json::to_string(&document)?)
}

/// Unix seconds.
pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(host: &str) -> Agent {
        let store = ConfigStore::open_in_memory().expect("store opens");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        Agent::new(store, identity, host.to_string(), 7878)
    }

    #[test]
    fn a_committed_document_carries_the_revision_the_store_assigned() {
        // Found by driving a real edit through a running agent: the revision the
        // store reported and the one inside the document had drifted apart, so a
        // client reading `config.rev` saw a number that had stopped being true.
        let agent = agent("192.168.1.10");

        for expected in 1..=3 {
            let rev = agent
                .replace_config(r#"{"schema":2,"profiles":[]}"#)
                .expect("commits");
            assert_eq!(rev, expected);

            let (stored_rev, document) = agent.config().expect("reads").expect("exists");
            assert_eq!(stored_rev, expected);

            let parsed: serde_json::Value = serde_json::from_str(&document).expect("valid JSON");
            assert_eq!(
                parsed["rev"], expected,
                "the document's rev must match the store's, not lag behind it"
            );
        }
    }

    #[test]
    fn stamping_leaves_the_rest_of_the_document_alone() {
        // The stamp is an edit to one field, not a rewrite: dropping anything
        // else would silently discard a user's configuration on every save.
        let stamped =
            stamp_rev(r#"{"schema":2,"profiles":[{"id":"tab1"}],"rev":99}"#, 7).expect("stamps");

        let parsed: serde_json::Value = serde_json::from_str(&stamped).expect("valid JSON");
        assert_eq!(parsed["rev"], 7, "overwritten, whatever the client claimed");
        assert_eq!(parsed["schema"], 2);
        assert_eq!(parsed["profiles"][0]["id"], "tab1");
    }

    #[test]
    fn a_fresh_agent_does_not_claim_its_buttons_work() {
        // The default executor is inert, so anything reporting "actions are
        // wired up" would simply be lying.
        assert!(!agent("192.168.1.10").has_live_executor());
    }

    #[test]
    fn installing_a_live_executor_is_observable() {
        let agent = agent("192.168.1.10");
        agent.install_live_executor(Box::new(RecordingExecutor::new()));

        assert!(agent.has_live_executor());
    }

    #[test]
    fn a_loopback_host_is_reported_as_unreachable() {
        // This is the check that stops a shell from showing a QR nobody can use.
        assert!(!agent("127.0.0.1").is_reachable_from_a_device());
        assert!(!agent("localhost").is_reachable_from_a_device());
    }

    #[test]
    fn a_lan_host_is_reported_as_reachable() {
        assert!(agent("192.168.1.10").is_reachable_from_a_device());
    }

    #[test]
    fn a_discovered_host_is_either_reachable_or_honestly_loopback() {
        let store = ConfigStore::open_in_memory().expect("store opens");
        let identity = Identity::generate(vec!["localhost".to_string()]).expect("identity");
        let agent = Agent::with_discovered_host(store, identity, 7878);

        // Depends on this machine's network, so either answer is acceptable —
        // but the fallback must be *declared* loopback, not something that looks
        // reachable and is not.
        assert!(
            agent.is_reachable_from_a_device() || agent.host() == "127.0.0.1",
            "unexpected advertised host: {}",
            agent.host()
        );
    }

    #[test]
    fn a_first_run_is_seeded_and_a_later_one_is_left_alone() {
        let agent = agent("192.168.1.10");

        assert!(agent.seed_default_config().expect("seeds"));
        assert_eq!(agent.rev().expect("rev"), Some(1));

        // Seeding again must not overwrite whatever the user has since built.
        assert!(!agent.seed_default_config().expect("seeds"));
    }

    #[test]
    fn the_default_config_actually_parses() {
        // It is a hand-written constant, so nothing else would catch a typo in
        // it — and the failure would land on a user's very first run, which is
        // the worst possible moment to discover it.
        let parsed: Result<flexdeck_protocol::Config, _> = serde_json::from_str(DEFAULT_CONFIG);
        assert!(parsed.is_ok(), "{:?}", parsed.err());
    }
}

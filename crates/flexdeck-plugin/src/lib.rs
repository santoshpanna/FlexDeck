//! The flexdeck plugin contract and registry.
//!
//! # The template
//!
//! A plugin is one type implementing [`Plugin`], and the shape it must fill is
//! deliberately small:
//!
//! * a manifest — ids, descriptions, the actions it offers, the settings it
//!   needs, the events it can announce;
//! * [`Plugin::variables`] — the live values it owns;
//! * [`Plugin::sample`] — refresh those values, **only** the ones asked for;
//! * [`Plugin::invoke`] — run one action.
//!
//! Everything a plugin can *do to the world* arrives through [`Ctx`], and every
//! capability in it is a trait rather than a concrete type. That is the house
//! rule this crate inherits from `flexdeck-actions`: `EnigoExecutor` types into
//! a real desktop while `RecordingExecutor` writes it down, and no test may ever
//! hold the first. The same pair exists here for system state
//! ([`SystemBackend`]) and for notifications ([`Notifier`]).
//!
//! # Two kinds of plugin, one catalogue
//!
//! Built-in plugins are compiled in and declare themselves with `'static` data
//! ([`PluginManifest`]). Installed plugins are separate processes (see
//! `flexdeck-plugin-host`) whose manifest only exists at run time. The registry
//! holds both the same way — as an owned [`abi::ManifestDto`] captured at
//! registration — so the editor, the validator and the dispatcher cannot tell
//! them apart, and a plugin can be added or removed while the agent runs.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use flexdeck_protocol::Action;
use flexdeck_vars::{VariableSpec, VariableStore};

pub use flexdeck_actions::{
    ActionSpec, ArgumentKind, ArgumentSpec, CoreAction, Executor, SliderSpec,
};
pub use flexdeck_plugin_abi as abi;
pub use flexdeck_vars::{Kind, Value};

pub mod builtin;
pub mod example;

/// A setting a plugin needs before it can work — an OBS host and password, say.
///
/// Declared on the manifest so the editor can render it and so a plugin never
/// invents its own configuration file. *Values* are held by the [`Registry`]
/// per plugin and handed to it in [`Ctx::settings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingSpec {
    pub key: &'static str,
    pub description: &'static str,
    pub kind: ArgumentKind,
    pub choices: &'static [&'static str],
    /// Used when the user has not set anything.
    pub default_value: &'static str,
    /// Never echoed back over the API.
    pub secret: bool,
}

impl SettingSpec {
    pub const fn text(
        key: &'static str,
        description: &'static str,
        default_value: &'static str,
    ) -> Self {
        Self {
            key,
            description,
            kind: ArgumentKind::Text,
            choices: &[],
            default_value,
            secret: false,
        }
    }

    pub const fn choice(
        key: &'static str,
        description: &'static str,
        default_value: &'static str,
        choices: &'static [&'static str],
    ) -> Self {
        Self {
            key,
            description,
            kind: ArgumentKind::Choice,
            choices,
            default_value,
            secret: false,
        }
    }

    fn to_dto(&self) -> abi::SettingDto {
        abi::SettingDto {
            key: self.key.to_string(),
            description: self.description.to_string(),
            kind: self.kind.as_str().to_string(),
            choices: strings(self.choices),
            default_value: Some(self.default_value.to_string()),
            secret: self.secret,
        }
    }
}

/// How a *built-in* plugin declares itself: static data, written once.
///
/// Converted to an owned [`abi::ManifestDto`] by [`PluginManifest::to_dto`],
/// which is the form the registry actually holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub actions: &'static [ActionSpec],
    pub settings: &'static [SettingSpec],
}

impl PluginManifest {
    pub fn to_dto(&self) -> abi::ManifestDto {
        let actions: Vec<abi::ActionDto> = self.actions.iter().map(action_to_dto).collect();
        let prefix = format!("{}.", self.id);
        let toggles = actions
            .iter()
            .any(|action| action.active.as_ref().is_some_and(|active| !active.is_plain(&prefix)));
        abi::ManifestDto {
            abi: abi::ABI_VERSION,
            id: self.id.to_string(),
            name: self.name.to_string(),
            version: self.version.to_string(),
            description: self.description.to_string(),
            author: "flexdeck".to_string(),
            actions,
            settings: self.settings.iter().map(SettingSpec::to_dto).collect(),
            events: Vec::new(),
            permissions: Vec::new(),
            uses: if toggles { vec![abi::capability::TOGGLES.to_string()] } else { Vec::new() },
            presets: Vec::new(),
            feeds: Vec::new(),
        }
    }
}

/// Convert one static catalogue entry to its owned form.
pub fn action_to_dto(spec: &ActionSpec) -> abi::ActionDto {
    abi::ActionDto {
        action: spec.action.to_string(),
        description: spec.description.to_string(),
        arguments: spec
            .arguments
            .iter()
            .map(|argument| abi::ArgumentDto {
                name: argument.name.to_string(),
                description: argument.description.to_string(),
                example: argument.example.to_string(),
                kind: argument.kind.as_str().to_string(),
                choices: strings(argument.choices),
                optional: argument.optional,
            })
            .collect(),
        slider: spec.slider.map(|slider| abi::SliderDto {
            argument: slider.argument.to_string(),
            min: slider.min,
            max: slider.max,
            step: slider.step,
            feedback: slider.feedback.map(str::to_string),
            mute: slider.mute.map(|mute| abi::SliderMuteDto {
                action: mute.action.to_string(),
                feedback: Some(mute.feedback.to_string()),
            }),
        }),
        active: spec.active.map(|active| abi::ActiveDto {
            variable: active.variable.to_string(),
            equals: active.equals.map(str::to_string),
            is: active.is.map(str::to_string),
        }),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Where a registered plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Compiled into this build.
    BuiltIn,
    /// Installed by the user, running as its own process.
    Installed,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::BuiltIn => "builtIn",
            Origin::Installed => "installed",
        }
    }
}

/// What one action actually did.
///
/// `input` is `Some` only when the action ran through the shared input executor,
/// so a caller can see exactly what was typed or pressed and a log can tell a
/// keypress from a volume change.
#[derive(Debug, Clone, PartialEq)]
pub struct Performed {
    pub plugin: String,
    pub action: String,
    pub input: Option<CoreAction>,
    /// The look the action reported, for a widget whose looks it provides.
    pub state: Option<String>,
}

/// What [`Plugin::invoke`] returns: the input it injected, if any, and the look
/// a widget driven by this action should now show, if it knows.
///
/// `From<Option<CoreAction>>` keeps the common case — "I typed this" or "I did
/// something with no input" — a one-word `.into()`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub input: Option<CoreAction>,
    pub state: Option<String>,
}

impl Outcome {
    /// Nothing typed; the widget should show the look called `state`.
    pub fn state(state: impl Into<String>) -> Self {
        Self {
            input: None,
            state: Some(state.into()),
        }
    }
}

impl From<Option<CoreAction>> for Outcome {
    fn from(input: Option<CoreAction>) -> Self {
        Self { input, state: None }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PluginError {
    #[error("no plugin is registered as '{0}'")]
    UnknownPlugin(String),

    #[error("the '{plugin}' plugin has no action called '{action}'")]
    UnknownAction { plugin: String, action: String },

    #[error("action '{action}' needs the argument '{argument}'")]
    MissingArgument { action: String, argument: String },

    #[error("'{name}' must be one of {choices:?}, not '{value}'")]
    NotAChoice {
        name: String,
        value: String,
        choices: Vec<String>,
    },

    #[error("'{0}' is not available on this system")]
    Unavailable(String),

    #[error("the action failed: {0}")]
    Failed(String),

    #[error(transparent)]
    Action(#[from] flexdeck_actions::ActionError),

    #[error(transparent)]
    Variables(#[from] flexdeck_vars::VarError),

    #[error(transparent)]
    Abi(#[from] abi::AbiError),
}

/// Shows a notification to the person at the machine.
///
/// The default in this crate records instead of showing. That is the same
/// failure mode as the inert `RecordingExecutor`: a build that has not been
/// given a real notifier is silent rather than lying about what it did.
pub trait Notifier: Send + std::fmt::Debug {
    fn notify(&mut self, title: &str, body: &str) -> Result<(), PluginError>;
}

/// Records notifications instead of showing them.
#[derive(Debug, Default)]
pub struct RecordingNotifier {
    shown: Vec<(String, String)>,
}

impl RecordingNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shown(&self) -> &[(String, String)] {
        &self.shown
    }

    pub fn clear(&mut self) {
        self.shown.clear();
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&mut self, title: &str, body: &str) -> Result<(), PluginError> {
        self.shown.push((title.to_string(), body.to_string()));
        Ok(())
    }
}

/// Reads and changes system state.
///
/// One trait for the whole of "the operating system", because every method here
/// is either an OS call or unavailable, and splitting it into five one-method
/// traits would multiply the test doubles without isolating anything.
pub trait SystemBackend: Send + std::fmt::Debug {
    /// 0-100.
    fn volume(&mut self) -> Result<f64, PluginError>;
    fn set_volume(&mut self, percent: f64) -> Result<(), PluginError>;
    fn muted(&mut self) -> Result<bool, PluginError>;
    fn set_muted(&mut self, muted: bool) -> Result<(), PluginError>;
    fn output_device(&mut self) -> Result<String, PluginError>;
    fn set_output_device(&mut self, device: &str) -> Result<(), PluginError>;
    fn output_devices(&mut self) -> Result<Vec<String>, PluginError>;
    /// 0-100. A *rate*, which is why the sampler must keep prior state.
    fn cpu_usage(&mut self) -> Result<f64, PluginError>;
}

/// The honest default for a platform this build has no backend for.
///
/// Every call fails with a reason rather than returning a plausible zero. A
/// volume of `0` would make `volume >= 50` quietly false, and the user would be
/// debugging their automation instead of reading "not available on this system".
#[derive(Debug, Default)]
pub struct UnavailableSystem;

const NO_BACKEND: &str = "this build has no system backend for this platform";

impl SystemBackend for UnavailableSystem {
    fn volume(&mut self) -> Result<f64, PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn set_volume(&mut self, _percent: f64) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn muted(&mut self) -> Result<bool, PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn set_muted(&mut self, _muted: bool) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn output_device(&mut self) -> Result<String, PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn set_output_device(&mut self, _device: &str) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn output_devices(&mut self) -> Result<Vec<String>, PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }

    fn cpu_usage(&mut self) -> Result<f64, PluginError> {
        Err(PluginError::Unavailable(NO_BACKEND.to_string()))
    }
}

/// What a plugin is given to work with. Every part is a capability, not a
/// global, so a test can hand it recorders.
///
/// Deliberately not `Debug`: it holds a `&mut dyn Executor`, and printing the
/// thing that types into a user's desktop is not a diagnostic worth having.
pub struct Ctx<'a> {
    /// Injects input. Plugins that only change system state ignore it.
    pub input: &'a mut dyn Executor,
    /// Live variables. Read freely; `refresh` your own; `set` the user's.
    pub vars: &'a VariableStore,
    pub notifier: &'a mut dyn Notifier,
    /// Settings for the calling plugin, by key, defaults filled in.
    ///
    /// The registry replaces whatever the caller put here with the plugin's own
    /// settings before every call, so a plugin can never read another's.
    pub settings: &'a BTreeMap<String, String>,
    pub now_ms: i64,
}

/// One plugin.
///
/// `Debug` is required rather than optional so the registry can derive it: a
/// `Box<dyn Plugin>` that cannot be printed turns every `{:?}` on the plugin set
/// into a compile error at the worst moment.
pub trait Plugin: Send + std::fmt::Debug {
    /// Who this plugin is and what it offers. Read once, at registration.
    fn manifest(&self) -> abi::ManifestDto;

    /// The variables this plugin owns *right now*.
    ///
    /// Read at registration to declare them, and again whenever the catalogue
    /// is served — an installed plugin may declare more as it learns them (one
    /// per OBS scene, say).
    fn variables(&self) -> Vec<VariableSpec>;

    /// Reject a value this plugin cannot accept, **before anything runs**.
    ///
    /// The registry checks the catalogue: does the action exist, is the argument
    /// present, is a `choice` really one of its choices. Only the plugin can
    /// check the *value* — whether `ctrl+shift+m` parses, whether an output
    /// device exists — so a plugin that can do so cheaply should, and then a
    /// sequence fails as a whole instead of after its first button press.
    fn check(&self, _action: &str, _args: &HashMap<String, String>) -> Result<(), PluginError> {
        Ok(())
    }

    /// The values `action`'s arguments can take right now, for the editor —
    /// OBS's scene names, its inputs. `args` is what is filled in so far, since
    /// one argument's choices can depend on another's value.
    ///
    /// Arguments left out of the answer are typed freely, which is what the
    /// default answers for all of them.
    fn choices(
        &self,
        _action: &str,
        _args: &HashMap<String, String>,
    ) -> Result<abi::ChoicesResponse, PluginError> {
        Ok(abi::ChoicesResponse::default())
    }

    /// A key started using one of this plugin's feeds, with its arguments:
    /// publish its values under `<plugin>.@<instance>.`. Only a plugin whose
    /// manifest lists feeds is asked.
    fn watch_feed(&mut self, _instance: &str, feed: &str, _args: &BTreeMap<String, String>) -> Result<(), PluginError> {
        Err(PluginError::Failed(format!("this plugin has no feed '{feed}'")))
    }

    /// That key is gone: stop publishing for it.
    fn unwatch_feed(&mut self, _instance: &str) -> Result<(), PluginError> {
        Ok(())
    }

    /// Refresh `wanted`, and only `wanted`.
    ///
    /// An empty slice means "nothing is watching you"; returning `Ok(())`
    /// without doing any work is the correct response, not an oversight.
    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError>;

    /// Run one action. Arguments are already checked against the catalogue.
    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError>;

    /// The user changed this plugin's settings. Defaults are filled in.
    fn settings_changed(&mut self, _settings: &BTreeMap<String, String>) {}

    /// `None` when healthy; otherwise why the plugin cannot work right now —
    /// "OBS is not running", "the process crashed: …".
    fn status(&self) -> Option<String> {
        None
    }

    fn origin(&self) -> Origin {
        Origin::BuiltIn
    }

    /// Stop. Called once, when the plugin is unregistered.
    fn shutdown(&mut self) {}
}

const SLOW_WAIT: Duration = Duration::from_millis(200);
const SLOW_SAMPLE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
struct Registered {
    plugin: Arc<Mutex<Box<dyn Plugin>>>,
    /// Captured once at registration.
    manifest: Arc<abi::ManifestDto>,
    origin: Origin,
    delivered: Arc<AtomicU64>,
}

impl Registered {
    fn plugin(&self) -> MutexGuard<'_, Box<dyn Plugin>> {
        let started = Instant::now();
        let guard = self.plugin.lock().unwrap_or_else(PoisonError::into_inner);
        let waited = started.elapsed();
        if waited > SLOW_WAIT {
            tracing::warn!(plugin = %self.manifest.id, ms = waited.as_millis() as u64, "waited for plugin");
        }
        guard
    }
}

/// The plugins this agent has, built in or installed.
#[must_use = "the plugin is only shut down by shut_down"]
#[derive(Debug)]
pub struct Retired {
    entry: Registered,
}

impl Retired {
    pub fn shut_down(self, store: &VariableStore) {
        let mut plugin = self.entry.plugin();
        for spec in plugin.variables() {
            store.undeclare(&spec.name);
        }
        plugin.shutdown();
    }
}

#[must_use = "the plugin is only told by deliver"]
#[derive(Debug)]
pub struct SettingsUpdate {
    entry: Option<Registered>,
    effective: BTreeMap<String, String>,
    generation: u64,
}

impl SettingsUpdate {
    pub fn deliver(self) {
        if let Some(entry) = self.entry {
            let mut plugin = entry.plugin();
            if entry.delivered.load(Ordering::SeqCst) < self.generation {
                entry.delivered.store(self.generation, Ordering::SeqCst);
                plugin.settings_changed(&self.effective);
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Registry {
    plugins: BTreeMap<String, Registered>,
    /// What the user set, by plugin id then key. Kept even while a plugin is
    /// not registered, so reinstalling one keeps its configuration.
    settings: BTreeMap<String, BTreeMap<String, String>>,
    /// Bumped on every register, unregister and settings change, so a client
    /// polling the catalogue can tell it moved without diffing it.
    generation: u64,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a plugin.
    ///
    /// Re-registering an id replaces the plugin, shutting the old one down.
    /// That is what makes a caller able to swap a recording build for a live
    /// one, or an old version for a new one, without a special case.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        let origin = plugin.origin();
        if let Some(old) = self.plugins.remove(&id) {
            old.plugin().shutdown();
        }
        self.plugins.insert(
            id,
            Registered {
                plugin: Arc::new(Mutex::new(plugin)),
                manifest: Arc::new(manifest),
                origin,
                delivered: Arc::new(AtomicU64::new(0)),
            },
        );
        self.generation += 1;
    }

    /// Add a plugin and publish its variables in one step.
    pub fn register_into(&mut self, plugin: Box<dyn Plugin>, store: &VariableStore) {
        store.declare_all(plugin.variables());
        self.register(plugin);
    }

    /// Remove a plugin, shut it down, and forget its variables.
    ///
    /// Returns `false` if nothing was registered under `id`. Widgets bound to
    /// it are left alone: they fail [`Registry::check`] with `UnknownPlugin`,
    /// which the editor shows as "not installed".
    pub fn unregister(&mut self, id: &str, store: &VariableStore) -> bool {
        self.retire(id, store).map(|retired| retired.shut_down(store)).is_some()
    }

    pub fn retire(&mut self, id: &str, store: &VariableStore) -> Option<Retired> {
        let entry = self.plugins.remove(id)?;
        store.undeclare_prefix(&format!("{id}."));
        self.generation += 1;
        Some(Retired { entry })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.plugins.keys().map(String::as_str).collect()
    }

    pub fn manifest(&self, id: &str) -> Option<&abi::ManifestDto> {
        self.plugins.get(id).map(|entry| entry.manifest.as_ref())
    }

    pub fn manifests(&self) -> Vec<&abi::ManifestDto> {
        self.plugins.values().map(|entry| entry.manifest.as_ref()).collect()
    }

    pub fn origin(&self, id: &str) -> Option<Origin> {
        self.plugins.get(id).map(|entry| entry.origin)
    }

    pub fn status(&self, id: &str) -> Option<Option<String>> {
        self.plugins.get(id).map(|entry| entry.plugin().status())
    }

    /// Every variable every plugin declares, ordered by name.
    pub fn variables(&self) -> Vec<VariableSpec> {
        let mut all: Vec<VariableSpec> = self
            .plugins
            .values()
            .flat_map(|entry| entry.plugin().variables())
            .collect();
        all.sort_by(|left, right| left.name.cmp(&right.name));
        all
    }

    /// Publish every plugin's declarations into the store.
    pub fn declare_variables(&self, store: &VariableStore) {
        store.declare_all(self.variables());
    }

    /// The catalogue entry for an action, if this agent offers it.
    pub fn action_spec(&self, plugin: &str, action: &str) -> Option<&abi::ActionDto> {
        self.plugins
            .get(plugin)?
            .manifest
            .actions
            .iter()
            .find(|spec| spec.action == action)
    }

    /// What the user set for `plugin`, *without* defaults filled in.
    pub fn stored_settings(&self, plugin: &str) -> BTreeMap<String, String> {
        self.settings.get(plugin).cloned().unwrap_or_default()
    }

    /// Every plugin's stored settings, including plugins not registered now.
    pub fn all_settings(&self) -> BTreeMap<String, BTreeMap<String, String>> {
        self.settings
            .iter()
            .filter(|(_, values)| !values.is_empty())
            .map(|(id, values)| (id.clone(), values.clone()))
            .collect()
    }

    /// What `plugin` sees: the user's values, defaults filled in.
    pub fn effective_settings(&self, plugin: &str) -> BTreeMap<String, String> {
        let mut effective = BTreeMap::new();
        if let Some(entry) = self.plugins.get(plugin) {
            for setting in &entry.manifest.settings {
                if let Some(default) = &setting.default_value {
                    effective.insert(setting.key.clone(), default.clone());
                }
            }
        }
        if let Some(stored) = self.settings.get(plugin) {
            for (key, value) in stored {
                effective.insert(key.clone(), value.clone());
            }
        }
        effective
    }

    /// Replace the user's settings for `plugin`, and tell the plugin.
    ///
    /// Keys the plugin does not declare are refused rather than stored: a
    /// typo in a settings file should not become a value nobody ever reads.
    pub fn set_settings(
        &mut self,
        plugin: &str,
        values: BTreeMap<String, String>,
    ) -> Result<(), PluginError> {
        self.update_settings(plugin, values).map(SettingsUpdate::deliver)
    }

    pub fn update_settings(
        &mut self,
        plugin: &str,
        values: BTreeMap<String, String>,
    ) -> Result<SettingsUpdate, PluginError> {
        if let Some(entry) = self.plugins.get(plugin) {
            for key in values.keys() {
                if !entry.manifest.settings.iter().any(|s| &s.key == key) {
                    return Err(PluginError::Failed(format!(
                        "the '{plugin}' plugin has no setting called '{key}'"
                    )));
                }
            }
        }
        self.settings.insert(plugin.to_string(), values);
        self.generation += 1;
        Ok(SettingsUpdate {
            entry: self.plugins.get(plugin).cloned(),
            effective: self.effective_settings(plugin),
            generation: self.generation,
        })
    }

    /// Keep values a plugin stored for itself (`settings/store`) on top of what
    /// is there, *without* telling it: it sent them, so it already has them,
    /// and an echo would loop. Undeclared keys are refused as in
    /// [`Registry::set_settings`].
    pub fn merge_settings(
        &mut self,
        plugin: &str,
        values: BTreeMap<String, String>,
    ) -> Result<(), PluginError> {
        let Some(entry) = self.plugins.get(plugin) else {
            return Err(PluginError::UnknownPlugin(plugin.to_string()));
        };
        if let Some(key) = values
            .keys()
            .find(|key| !entry.manifest.settings.iter().any(|s| &&s.key == key))
        {
            return Err(PluginError::Failed(format!(
                "the '{plugin}' plugin has no setting called '{key}'"
            )));
        }
        self.settings
            .entry(plugin.to_string())
            .or_default()
            .extend(values);
        self.generation += 1;
        Ok(())
    }

    /// Restore stored settings without notifying anyone — the startup path,
    /// before any plugin exists to tell.
    pub fn load_settings(&mut self, plugin: &str, values: BTreeMap<String, String>) {
        self.settings.insert(plugin.to_string(), values);
    }

    /// Everything wrong with an action, before anything runs.
    ///
    /// A misspelled plugin, an action that does not exist, a blank argument, or
    /// a value outside a declared choice is a configuration mistake the editor
    /// should show — not a button that silently does nothing when pressed.
    /// What `action`'s arguments can be set to right now, as its plugin sees it.
    ///
    /// Only the action's own arguments come back, whatever the plugin said, so
    /// the editor never offers a field the action does not take.
    /// Ask `plugin` to run its `feed` for a key. Also takes away the key's
    /// old values, so a renamed argument does not leave stale ones.
    pub fn watch_feed(&mut self, plugin: &str, instance: &str, feed: &str, args: &BTreeMap<String, String>) -> Result<(), PluginError> {
        let registered = self
            .plugins
            .get(plugin)
            .ok_or_else(|| PluginError::UnknownPlugin(plugin.to_string()))?;
        if !registered.manifest.feeds.iter().any(|f| f.name == feed) {
            return Err(PluginError::Failed(format!("{plugin} has no feed '{feed}'")));
        }
        registered.plugin().watch_feed(instance, feed, args)
    }

    pub fn unwatch_feed(&mut self, plugin: &str, instance: &str) -> Result<(), PluginError> {
        match self.plugins.get(plugin) {
            Some(registered) => registered.plugin().unwatch_feed(instance),
            None => Ok(()),
        }
    }

    pub fn choices(&self, action: &Action) -> Result<abi::ChoicesResponse, PluginError> {
        let Some(spec) = self.action_spec(&action.plugin, &action.action) else {
            return Err(if self.plugins.contains_key(action.plugin.as_str()) {
                PluginError::UnknownAction {
                    plugin: action.plugin.clone(),
                    action: action.action.clone(),
                }
            } else {
                PluginError::UnknownPlugin(action.plugin.clone())
            });
        };
        let names: Vec<&str> = spec.arguments.iter().map(|a| a.name.as_str()).collect();
        let registered = &self.plugins[action.plugin.as_str()];
        let mut answer = registered.plugin().choices(&action.action, &action.args)?;
        answer
            .arguments
            .retain(|name, _| names.contains(&name.as_str()));
        Ok(answer)
    }

    pub fn check(&self, action: &Action) -> Result<&abi::ActionDto, PluginError> {
        if action.plugin.trim().is_empty() {
            return Err(PluginError::UnknownPlugin(String::new()));
        }

        let Some(spec) = self.action_spec(&action.plugin, &action.action) else {
            return Err(if self.plugins.contains_key(action.plugin.as_str()) {
                PluginError::UnknownAction {
                    plugin: action.plugin.clone(),
                    action: action.action.clone(),
                }
            } else {
                PluginError::UnknownPlugin(action.plugin.clone())
            });
        };

        for argument in &spec.arguments {
            let value = action
                .args
                .get(&argument.name)
                .map(String::as_str)
                .unwrap_or_default();
            if value.trim().is_empty() {
                if argument.optional {
                    continue;
                }
                return Err(PluginError::MissingArgument {
                    action: action.action.clone(),
                    argument: argument.name.clone(),
                });
            }

            // A choice is only worth declaring if a typo is refused here rather
            // than becoming an action that never matches anything.
            if argument.kind == abi::kind::CHOICE && !argument.choices.iter().any(|c| c == value) {
                return Err(PluginError::NotAChoice {
                    name: argument.name.clone(),
                    value: value.to_string(),
                    choices: argument.choices.clone(),
                });
            }
        }

        // The catalogue is checked above; the *value* is the plugin's business.
        let entry = self
            .plugins
            .get(action.plugin.as_str())
            .expect("the action spec proves the plugin is registered");
        entry.plugin().check(&action.action, &action.args)?;

        Ok(spec)
    }

    /// Check and run one action.
    pub fn invoke(&mut self, action: &Action, ctx: &mut Ctx<'_>) -> Result<Performed, PluginError> {
        self.check(action)?;

        let settings = self.effective_settings(&action.plugin);
        let entry = self
            .plugins
            .get(action.plugin.as_str())
            .expect("check() proved the plugin is registered");

        let mut scoped = Ctx {
            input: &mut *ctx.input,
            vars: ctx.vars,
            notifier: &mut *ctx.notifier,
            settings: &settings,
            now_ms: ctx.now_ms,
        };
        let outcome = entry
            .plugin()
            .invoke(&action.action, &action.args, &mut scoped)?;

        Ok(Performed {
            plugin: action.plugin.clone(),
            action: action.action.clone(),
            input: outcome.input,
            state: outcome.state,
        })
    }

    /// Run actions in order, stopping at the first failure.
    ///
    /// Everything is checked *before* anything is performed, so a widget whose
    /// second action is misconfigured runs nothing at all rather than half of
    /// itself — the same rule the core engine already follows.
    pub fn run_sequence(
        &mut self,
        actions: &[Action],
        ctx: &mut Ctx<'_>,
    ) -> Result<Vec<Performed>, PluginError> {
        for action in actions {
            self.check(action)?;
        }

        let mut performed = Vec::with_capacity(actions.len());
        for action in actions {
            performed.push(self.invoke(action, ctx)?);
        }
        Ok(performed)
    }

    /// Refresh the variables somebody is watching.
    ///
    /// A plugin owns every variable it declares *and* everything under its own
    /// `<id>.` namespace, so a variable an installed plugin declared at run time
    /// is still routed to it.
    ///
    /// Returns how many plugins did any work — `0` when nobody is interested,
    /// the observable form of "an idle agent costs nothing". One plugin failing
    /// does not stop the others being sampled; the first error is returned
    /// after all have had their turn.
    pub fn sample(
        &mut self,
        interested: &BTreeSet<String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<usize, PluginError> {
        let mut sampled = 0;
        let mut first_error = None;
        let ids: Vec<String> = self.plugins.keys().cloned().collect();

        for id in ids {
            match self.sample_plugin(&id, interested, ctx) {
                Ok(true) => sampled += 1,
                Ok(false) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                    sampled += 1;
                }
            }
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(sampled),
        }
    }

    pub fn sample_plugin(
        &mut self,
        id: &str,
        interested: &BTreeSet<String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<bool, PluginError> {
        let settings = self.effective_settings(id);
        let Some(entry) = self.plugins.get(id) else {
            return Ok(false);
        };
        let mut plugin = entry.plugin();
        let prefix = format!("{id}.");
        let declared: BTreeSet<String> = plugin
            .variables()
            .into_iter()
            .map(|spec| spec.name)
            .collect();

        let wanted: Vec<&str> = interested
            .iter()
            .filter(|name| declared.contains(*name) || name.starts_with(&prefix))
            .map(String::as_str)
            .collect();

        if wanted.is_empty() {
            return Ok(false);
        }

        let mut scoped = Ctx {
            input: &mut *ctx.input,
            vars: ctx.vars,
            notifier: &mut *ctx.notifier,
            settings: &settings,
            now_ms: ctx.now_ms,
        };
        let started = Instant::now();
        let sampled = plugin.sample(&wanted, &mut scoped);
        let took = started.elapsed();
        if took > SLOW_SAMPLE {
            tracing::warn!(plugin = %id, wanted = wanted.len(), ms = took.as_millis() as u64, "slow sample");
        } else {
            tracing::debug!(plugin = %id, wanted = wanted.len(), ms = took.as_millis() as u64, "sampled");
        }
        sampled?;
        Ok(true)
    }

    /// The catalogue as JSON, for the editor to build its forms from.
    ///
    /// Secret settings are described but their values are never included —
    /// only whether one has been set.
    pub fn catalogue_json(&self) -> serde_json::Value {
        let plugins: Vec<serde_json::Value> = self
            .plugins
            .iter()
            .map(|(id, entry)| {
                let manifest = &entry.manifest;
                let stored = self.settings.get(id);

                let settings: Vec<serde_json::Value> = manifest
                    .settings
                    .iter()
                    .map(|setting| {
                        let value = stored.and_then(|values| values.get(&setting.key));
                        serde_json::json!({
                            "key": setting.key,
                            "description": setting.description,
                            "kind": setting.kind,
                            "choices": setting.choices,
                            "defaultValue": setting.default_value,
                            "secret": setting.secret,
                            "value": if setting.secret { None } else { value },
                            "isSet": value.is_some(),
                        })
                    })
                    .collect();

                let plugin = entry.plugin();
                let variables: Vec<serde_json::Value> = plugin
                    .variables()
                    .iter()
                    .map(|spec| {
                        serde_json::json!({
                            "name": spec.name,
                            "description": spec.description,
                            "kind": kind_name(spec.kind),
                            "unit": spec.unit,
                            "choices": spec.choices,
                            "readOnly": spec.read_only,
                        })
                    })
                    .collect();

                serde_json::json!({
                    "id": manifest.id,
                    "name": manifest.name,
                    "version": manifest.version,
                    "description": manifest.description,
                    "author": manifest.author,
                    "origin": entry.origin.as_str(),
                    "status": plugin.status(),
                    "actions": manifest.actions,
                    "settings": settings,
                    "events": manifest.events,
                    "permissions": manifest.permissions,
                    "variables": variables,
                    "feeds": manifest.feeds,
                    "presets": manifest.presets,
                })
            })
            .collect();

        serde_json::json!({ "generation": self.generation, "plugins": plugins })
    }
}

/// Read an argument the registry has already proved is present and non-blank.
pub(crate) fn argument<'a>(args: &'a HashMap<String, String>, name: &str) -> &'a str {
    args.get(name).map(String::as_str).unwrap_or_default()
}

/// The wire spelling of a [`Kind`].
///
/// One decision in one place, so the plugin catalogue and the variables
/// endpoint cannot invent two spellings for the same type.
pub fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Bool => abi::kind::BOOL,
        Kind::Number => abi::kind::NUMBER,
        Kind::Text => abi::kind::TEXT,
        Kind::Choice => abi::kind::CHOICE,
    }
}

/// The [`Kind`] a wire spelling names.
pub fn kind_from_name(name: &str) -> Result<Kind, abi::AbiError> {
    match name {
        abi::kind::BOOL => Ok(Kind::Bool),
        abi::kind::NUMBER => Ok(Kind::Number),
        abi::kind::TEXT => Ok(Kind::Text),
        abi::kind::CHOICE => Ok(Kind::Choice),
        other => Err(abi::AbiError::Unknown {
            what: "kind",
            value: other.to_string(),
            expected: abi::kind::ALL,
        }),
    }
}

/// A variable declared over the wire, as the store's own type.
pub fn variable_from_dto(dto: &abi::VariableDto) -> Result<VariableSpec, abi::AbiError> {
    let mut spec = VariableSpec::new(&dto.name, kind_from_name(&dto.kind)?, &dto.description);
    spec.unit = dto.unit.clone();
    spec.choices = dto.choices.clone();
    spec.read_only = dto.read_only;
    Ok(spec)
}

/// A value received over the wire, checked against its declared kind.
pub fn value_from_dto(dto: &abi::ValueDto) -> Result<Value, PluginError> {
    let wrong = || PluginError::Failed(format!("a '{}' value cannot be {}", dto.kind, dto.value));
    match kind_from_name(&dto.kind)? {
        Kind::Bool => dto.value.as_bool().map(Value::Bool).ok_or_else(wrong),
        Kind::Number => dto.value.as_f64().map(Value::Number).ok_or_else(wrong),
        Kind::Text | Kind::Choice => dto
            .value
            .as_str()
            .map(|text| Value::Text(text.to_string()))
            .ok_or_else(wrong),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_actions::RecordingExecutor;

    fn ctx<'a>(
        input: &'a mut RecordingExecutor,
        vars: &'a VariableStore,
        notifier: &'a mut RecordingNotifier,
        settings: &'a BTreeMap<String, String>,
    ) -> Ctx<'a> {
        Ctx {
            input,
            vars,
            notifier,
            settings,
            now_ms: 0,
        }
    }

    fn registry() -> Registry {
        let mut registry = Registry::new();
        for plugin in builtin::builtins(builtin::Backends::recording()) {
            registry.register(plugin);
        }
        registry.register(Box::new(example::Example::new()));
        registry
    }

    fn action(plugin: &str, action: &str, args: &[(&str, &str)]) -> Action {
        Action {
            plugin: plugin.to_string(),
            action: action.to_string(),
            args: args
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        }
    }

    #[test]
    fn every_plugin_id_is_unique_and_the_registry_keeps_them_all() {
        let registry = registry();

        assert_eq!(
            registry.ids(),
            vec![
                "core",
                "deck",
                "display",
                "example",
                "launcher",
                "media",
                "notifications",
                "sensors",
                "system",
                "time",
                "timer",
                "vars"
            ]
        );
    }

    #[test]
    fn every_built_in_manifest_passes_the_same_validation_an_installed_one_does() {
        for manifest in registry().manifests() {
            manifest
                .validate()
                .unwrap_or_else(|error| panic!("{}: {error}", manifest.id));
        }
    }

    #[test]
    fn every_catalogued_action_runs_with_its_own_example() {
        // The catalogue drives the editor's form, so its own examples are what a
        // user ends up configuring. If one of them produced an action the agent
        // could not run, the editor would build it happily and the button would
        // do nothing — the exact failure this path exists to prevent.
        let mut registry = registry();
        let vars = VariableStore::new();
        registry.declare_variables(&vars);
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();

        let catalogue: Vec<Action> = registry
            .manifests()
            .iter()
            .flat_map(|manifest| {
                manifest.actions.iter().map(|spec| Action {
                    plugin: manifest.id.clone(),
                    action: spec.action.clone(),
                    args: spec
                        .arguments
                        .iter()
                        .map(|argument| (argument.name.clone(), argument.example.clone()))
                        .collect(),
                })
            })
            .collect();

        assert!(!catalogue.is_empty());

        for configured in &catalogue {
            let mut ctx = ctx(&mut input, &vars, &mut notifier, &settings);
            registry
                .invoke(configured, &mut ctx)
                .unwrap_or_else(|error| panic!("{configured:?} could not run: {error}"));
        }
    }

    #[test]
    fn every_catalogued_choice_example_is_actually_one_of_its_choices() {
        for manifest in registry().manifests() {
            for spec in &manifest.actions {
                for argument in &spec.arguments {
                    if argument.kind == abi::kind::CHOICE {
                        assert!(
                            argument.choices.contains(&argument.example),
                            "{}.{}: example '{}' is not one of {:?}",
                            manifest.id,
                            spec.action,
                            argument.example,
                            argument.choices
                        );
                        assert!(!argument.choices.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn an_unknown_plugin_is_named_as_such() {
        let registry = registry();
        let error = registry
            .check(&action("obs", "scene", &[]))
            .expect_err("obs is not registered");

        assert_eq!(error, PluginError::UnknownPlugin("obs".to_string()));
    }

    #[test]
    fn an_unknown_action_is_named_as_the_action_not_the_plugin() {
        let registry = registry();
        let error = registry
            .check(&action("core", "teleport", &[]))
            .expect_err("no such action");

        assert_eq!(
            error,
            PluginError::UnknownAction {
                plugin: "core".to_string(),
                action: "teleport".to_string(),
            }
        );
    }

    #[test]
    fn a_blank_argument_is_refused_and_the_error_names_a_real_one() {
        let registry = registry();
        let error = registry
            .check(&action("core", "hotkey", &[("keys", "  ")]))
            .expect_err("a blank key is not a key");

        let PluginError::MissingArgument { argument, .. } = error else {
            panic!("expected a missing-argument error");
        };

        let declared = registry
            .action_spec("core", "hotkey")
            .expect("catalogued")
            .arguments
            .iter()
            .any(|spec| spec.name == argument);
        assert!(declared, "'{argument}' is not a declared argument");
    }

    #[test]
    fn a_value_outside_a_declared_choice_is_refused() {
        let registry = registry();
        let error = registry
            .check(&action(
                "example",
                "demo",
                &[("text", "hi"), ("amount", "3"), ("mode", "banana")],
            ))
            .expect_err("banana is not a mode");

        assert!(matches!(error, PluginError::NotAChoice { .. }));
    }

    #[test]
    fn a_misconfigured_action_stops_the_whole_sequence_before_anything_runs() {
        let mut registry = registry();
        let vars = VariableStore::new();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();

        let actions = vec![
            action("core", "text", &[("text", "first")]),
            action("core", "hotkey", &[("keys", "ctrl+banana")]),
        ];

        let mut ctx = ctx(&mut input, &vars, &mut notifier, &settings);
        assert!(registry.run_sequence(&actions, &mut ctx).is_err());

        assert!(
            input.performed().is_empty(),
            "nothing must run when a later action is misconfigured"
        );
    }

    #[test]
    fn nothing_is_sampled_when_nobody_is_interested() {
        let mut registry = registry();
        let vars = VariableStore::new();
        registry.declare_variables(&vars);
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();

        let mut ctx = ctx(&mut input, &vars, &mut notifier, &settings);
        assert_eq!(registry.sample(&BTreeSet::new(), &mut ctx), Ok(0));
    }

    #[test]
    fn only_the_watched_variable_is_refreshed() {
        let mut registry = registry();
        let vars = VariableStore::new();
        registry.declare_variables(&vars);
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();

        let interested: BTreeSet<String> =
            ["system.audio.volume".to_string()].into_iter().collect();
        let mut ctx = ctx(&mut input, &vars, &mut notifier, &settings);
        assert_eq!(registry.sample(&interested, &mut ctx), Ok(1));

        assert!(vars.get("system.audio.volume").is_some());
        assert_eq!(
            vars.get("system.cpu.usage"),
            None,
            "an unwatched variable must not be sampled"
        );
    }

    #[test]
    fn unregistering_removes_the_actions_and_the_variables_and_moves_the_generation() {
        let mut registry = registry();
        let vars = VariableStore::new();
        registry.declare_variables(&vars);
        assert!(vars.spec("example.counter").is_some());
        let before = registry.generation();

        assert!(registry.unregister("example", &vars));

        assert!(registry.generation() > before);
        assert!(!registry.contains("example"));
        assert_eq!(vars.spec("example.counter"), None);
        assert_eq!(
            registry.check(&action("example", "demo", &[])),
            Err(PluginError::UnknownPlugin("example".into()))
        );
        assert!(
            vars.spec("system.audio.volume").is_some(),
            "another plugin's variables must survive"
        );
        assert!(!registry.unregister("example", &vars), "already gone");
    }

    #[test]
    fn a_plugin_sees_its_own_settings_with_defaults_and_never_another_plugins() {
        let mut registry = registry();
        let vars = VariableStore::new();
        registry.declare_variables(&vars);
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        // Whatever the caller passes is replaced by the plugin's own settings.
        let mut leaked = BTreeMap::new();
        leaked.insert(example::GREETING.to_string(), "LEAKED".to_string());

        let demo = action(
            "example",
            "demo",
            &[("text", "hi"), ("amount", "1"), ("mode", "add")],
        );
        {
            let mut ctx = ctx(&mut input, &vars, &mut notifier, &leaked);
            registry.invoke(&demo, &mut ctx).expect("runs");
        }
        assert_eq!(notifier.shown()[0].0, "Hello", "the default applies");

        registry
            .set_settings(
                "example",
                [(example::GREETING.to_string(), "Hey".to_string())].into(),
            )
            .expect("a declared key");
        {
            let mut ctx = ctx(&mut input, &vars, &mut notifier, &leaked);
            registry.invoke(&demo, &mut ctx).expect("runs");
        }
        assert_eq!(notifier.shown()[1].0, "Hey");

        assert!(
            registry
                .set_settings("example", [("nope".to_string(), "x".to_string())].into())
                .is_err(),
            "an undeclared key is refused"
        );
    }

    #[test]
    fn a_secret_setting_is_described_but_its_value_never_leaves() {
        // Built-ins have no secrets yet, so register a plugin that does.
        #[derive(Debug)]
        struct Secretive;
        impl Plugin for Secretive {
            fn manifest(&self) -> abi::ManifestDto {
                abi::ManifestDto {
                    abi: abi::ABI_VERSION,
                    id: "secretive".into(),
                    name: "Secretive".into(),
                    version: "1".into(),
                    description: String::new(),
                    author: String::new(),
                    actions: vec![],
                    settings: vec![abi::SettingDto {
                        key: "password".into(),
                        description: String::new(),
                        kind: abi::kind::TEXT.into(),
                        choices: vec![],
                        default_value: None,
                        secret: true,
                    }],
                    events: vec![],
                    permissions: vec![],
                    uses: Vec::new(),
                    presets: Vec::new(),
                    feeds: Vec::new(),
                }
            }
            fn variables(&self) -> Vec<VariableSpec> {
                Vec::new()
            }
            fn sample(&mut self, _: &[&str], _: &mut Ctx<'_>) -> Result<(), PluginError> {
                Ok(())
            }
            fn invoke(
                &mut self,
                _: &str,
                _: &HashMap<String, String>,
                _: &mut Ctx<'_>,
            ) -> Result<Outcome, PluginError> {
                Ok(Outcome::default())
            }
        }

        let mut registry = Registry::new();
        registry.register(Box::new(Secretive));
        registry
            .set_settings(
                "secretive",
                [("password".to_string(), "hunter2".to_string())].into(),
            )
            .unwrap();

        let text = registry.catalogue_json().to_string();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("\"isSet\":true"), "{text}");
        assert_eq!(
            registry.effective_settings("secretive")["password"],
            "hunter2"
        );
    }

    #[test]
    fn the_catalogue_json_declares_every_plugin_its_actions_and_its_variables() {
        let registry = registry();
        let json = registry.catalogue_json();
        let plugins = json["plugins"].as_array().expect("an array");

        assert_eq!(plugins.len(), registry.len());

        let system = plugins
            .iter()
            .find(|plugin| plugin["id"] == "system")
            .expect("system is catalogued");

        assert_eq!(system["origin"], "builtIn");
        assert!(
            system["actions"]
                .as_array()
                .expect("actions")
                .iter()
                .any(|entry| entry["action"] == "set_output_device"),
            "the device switch the user asked for must be catalogued"
        );
        assert!(
            system["variables"]
                .as_array()
                .expect("variables")
                .iter()
                .any(|entry| entry["name"] == "system.audio.volume"),
            "system.audio.volume must be declared for the editor"
        );
    }

    #[test]
    fn a_wire_value_is_checked_against_its_kind() {
        let number = abi::ValueDto {
            kind: abi::kind::NUMBER.into(),
            value: serde_json::json!(3.5),
        };
        assert_eq!(value_from_dto(&number), Ok(Value::Number(3.5)));
        let lie = abi::ValueDto {
            kind: abi::kind::BOOL.into(),
            value: serde_json::json!("yes"),
        };
        assert!(value_from_dto(&lie).is_err());
    }

    #[test]
    fn a_busy_plugin_does_not_hold_up_a_press_on_another() {
        use std::sync::mpsc;

        #[derive(Debug)]
        struct Gate {
            id: &'static str,
            entered: Option<mpsc::Sender<()>>,
            release: Option<mpsc::Receiver<()>>,
        }
        impl Plugin for Gate {
            fn manifest(&self) -> abi::ManifestDto {
                abi::ManifestDto {
                    abi: abi::ABI_VERSION,
                    id: self.id.into(),
                    name: self.id.into(),
                    version: "1".into(),
                    description: String::new(),
                    author: String::new(),
                    actions: vec![abi::ActionDto {
                        action: "go".into(),
                        description: String::new(),
                        arguments: vec![],
                        slider: None,
                        active: None,
                    }],
                    settings: vec![],
                    events: vec![],
                    permissions: vec![],
                    uses: Vec::new(),
                    presets: Vec::new(),
                    feeds: Vec::new(),
                }
            }
            fn variables(&self) -> Vec<VariableSpec> {
                Vec::new()
            }
            fn sample(&mut self, _: &[&str], _: &mut Ctx<'_>) -> Result<(), PluginError> {
                Ok(())
            }
            fn invoke(
                &mut self,
                _: &str,
                _: &HashMap<String, String>,
                _: &mut Ctx<'_>,
            ) -> Result<Outcome, PluginError> {
                if let (Some(entered), Some(release)) = (&self.entered, &self.release) {
                    entered.send(()).unwrap();
                    release.recv().unwrap();
                }
                Ok(Outcome::default())
            }
        }

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut registry = Registry::new();
        registry.register(Box::new(Gate { id: "slow", entered: Some(entered_tx), release: Some(release_rx) }));
        registry.register(Box::new(Gate { id: "fast", entered: None, release: None }));

        let mut stuck = registry.clone();
        let blocked = std::thread::spawn(move || {
            let vars = VariableStore::new();
            let mut input = RecordingExecutor::new();
            let mut notifier = RecordingNotifier::new();
            let settings = BTreeMap::new();
            let mut ctx = ctx(&mut input, &vars, &mut notifier, &settings);
            stuck.invoke(&action("slow", "go", &[]), &mut ctx).map(|_| ())
        });
        entered_rx.recv().unwrap();

        let vars = VariableStore::new();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = ctx(&mut input, &vars, &mut notifier, &settings);
        registry.invoke(&action("fast", "go", &[]), &mut ctx).expect("fast runs while slow is busy");

        release_tx.send(()).unwrap();
        blocked.join().unwrap().unwrap();
    }

    #[test]
    fn settings_and_retirement_reach_the_plugin_only_when_delivered() {
        #[derive(Debug, Default)]
        struct Seen {
            settings: Mutex<Vec<String>>,
            shutdowns: Mutex<usize>,
        }
        #[derive(Debug)]
        struct Counting(Arc<Seen>);
        impl Plugin for Counting {
            fn manifest(&self) -> abi::ManifestDto {
                abi::ManifestDto {
                    abi: abi::ABI_VERSION,
                    id: "counting".into(),
                    name: "Counting".into(),
                    version: "1".into(),
                    description: String::new(),
                    author: String::new(),
                    actions: vec![],
                    settings: vec![abi::SettingDto {
                        key: "mode".into(),
                        description: String::new(),
                        kind: abi::kind::TEXT.into(),
                        choices: vec![],
                        default_value: None,
                        secret: false,
                    }],
                    events: vec![],
                    permissions: vec![],
                    uses: Vec::new(),
                    presets: Vec::new(),
                    feeds: Vec::new(),
                }
            }
            fn variables(&self) -> Vec<VariableSpec> {
                Vec::new()
            }
            fn sample(&mut self, _: &[&str], _: &mut Ctx<'_>) -> Result<(), PluginError> {
                Ok(())
            }
            fn invoke(
                &mut self,
                _: &str,
                _: &HashMap<String, String>,
                _: &mut Ctx<'_>,
            ) -> Result<Outcome, PluginError> {
                Ok(Outcome::default())
            }
            fn settings_changed(&mut self, settings: &BTreeMap<String, String>) {
                self.0.settings.lock().unwrap().push(settings["mode"].clone());
            }
            fn shutdown(&mut self) {
                *self.0.shutdowns.lock().unwrap() += 1;
            }
        }

        let seen = Arc::new(Seen::default());
        let vars = VariableStore::new();
        let mut registry = Registry::new();
        registry.register(Box::new(Counting(Arc::clone(&seen))));

        let first = registry.update_settings("counting", [("mode".to_string(), "a".to_string())].into()).unwrap();
        let second = registry.update_settings("counting", [("mode".to_string(), "b".to_string())].into()).unwrap();
        assert!(seen.settings.lock().unwrap().is_empty(), "nothing is told before delivery");

        second.deliver();
        first.deliver();
        assert_eq!(*seen.settings.lock().unwrap(), vec!["b".to_string()], "a stale update is dropped");

        let retired = registry.retire("counting", &vars).expect("registered");
        assert!(!registry.contains("counting"));
        assert_eq!(*seen.shutdowns.lock().unwrap(), 0, "not shut down before shut_down");
        retired.shut_down(&vars);
        assert_eq!(*seen.shutdowns.lock().unwrap(), 1);
        assert!(registry.retire("counting", &vars).is_none());
    }
}

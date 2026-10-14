//! The data contract between the flexdeck agent (the *host*) and a plugin.
//!
//! # Why this crate exists separately
//!
//! There are two kinds of plugin and one shape of catalogue. Built-in plugins
//! are compiled into the agent; external plugins are separate processes, built
//! by different toolchains, often not in Rust at all. The one thing every party
//! must agree on is the *shape of the messages*. Putting that shape in a leaf
//! crate means there is exactly one definition of it rather than two that drift
//! — and the registry uses these same types as its owned, runtime catalogue, so
//! a built-in and an installed plugin are indistinguishable once loaded.
//!
//! # The transport
//!
//! An external plugin is a child process speaking **JSON-RPC 2.0**, one message
//! per line, over its stdin/stdout. Stderr is free text and is captured as the
//! plugin's log. JSON rather than something compact for the reason the rest of
//! flexdeck uses JSON: a plugin author can debug it by eye, and the cost is paid
//! per action rather than per frame.
//!
//! The host calls the methods in [`method`]; the plugin may at any time send the
//! notifications in [`notify`] — that is how a plugin pushes a variable or an
//! event the moment it happens instead of waiting to be polled.
//!
//! # Versioning — the part that is easy to skip and expensive to skip
//!
//! [`ABI_VERSION`] is negotiated in `initialize` *before* any manifest is
//! trusted. Refuse a value you do not understand rather than guessing: a version
//! mismatch reported with both numbers is a sentence a person can act on, while
//! a plugin that silently does nothing is not.
//!
//! # Unknown fields are *rejected*, not ignored
//!
//! A field the host does not read is a version mismatch worth stopping on, not
//! worth quietly dropping. The JSON-RPC *envelope* is the one exception, because
//! it is a public standard with optional members the host does not control.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod repo;

/// The version of this contract.
///
/// Bump on **any** change to a message below, including an added field, because
/// unknown fields are refused (see the module docs).
///
/// History: `1` was the WebAssembly export contract; `2` is the sidecar
/// JSON-RPC contract and adds events, permissions and secret settings; `3` adds
/// slider metadata on actions, a look reported by `invoke`, and the
/// `settings/store`, `host/open_url` and `images/put` notifications.
pub const ABI_VERSION: u32 = 4;

/// Every ABI this host still speaks.
///
/// A package names its ABI in `plugin.json`, so the host knows which one to
/// speak *before* it starts the process. Every change from 2 to 3 is a field or
/// notification a plugin *may* send, never one the host sends, so a v2 plugin is
/// spoken to exactly as before and keeps working.
pub const SUPPORTED_ABIS: &[u32] = &[2, 3, 4];

/// What a host offers and a plugin may use, from ABI 4. New features arrive
/// as capabilities rather than as new ABIs: a host lists what it offers in
/// `initialize`, a plugin lists what it uses in its manifest, and something
/// only one side names is never used — so either side can be older.
pub mod capability {
    /// `series/set`: a whole history for a graph (a day of prices).
    pub const SERIES: &str = "series";
    /// Manifest `presets`: ready-made keys for the editor's gallery.
    pub const PRESETS: &str = "presets";
    /// Manifest `feeds` and `feed/watch`: values per key, with that key's
    /// arguments (a weather key per city).
    pub const FEEDS: &str = "feeds";
    /// An action's `active` may name a fixed value (`is`) rather than an
    /// argument, and its variable may hold `{{argument}}` placeholders: a
    /// toggle lit while `discord.voice.mute` is true.
    pub const TOGGLES: &str = "toggles";
    pub const ALL: &[&str] = &[SERIES, PRESETS, FEEDS, TOGGLES];
}

/// The most points a `series/set` may carry.
pub const MAX_SERIES_POINTS: usize = 600;

/// Whether this host can load a plugin built against `abi`.
pub fn supports_abi(abi: u32) -> bool {
    SUPPORTED_ABIS.contains(&abi)
}

/// Methods the host calls on a plugin.
pub mod method {
    /// `InitializeParams -> InitializeResult`. Always first.
    pub const INITIALIZE: &str = "initialize";
    /// `() -> Vec<VariableDto>` — the live values the plugin owns right now.
    pub const VARIABLES: &str = "variables";
    /// `CheckRequest -> CheckResponse` — is this action callable with these args?
    pub const CHECK: &str = "check";
    /// `InvokeRequest -> InvokeResponse` — run one action.
    pub const INVOKE: &str = "invoke";
    /// `SampleRequest -> SampleResponse` — refresh the named variables.
    pub const SAMPLE: &str = "sample";
    /// `SettingsChanged -> {}` — the user changed this plugin's settings.
    pub const SETTINGS_CHANGED: &str = "settings/changed";
    /// `() -> {}` — exit now. The host kills the process after a grace period.
    pub const SHUTDOWN: &str = "shutdown";
    /// `FeedWatch -> {}` — a key started using one of this plugin's feeds:
    /// publish its values as `<plugin>.@<instance>.<field>`. ABI 4, `feeds`.
    pub const FEED_WATCH: &str = "feed/watch";
    /// `FeedUnwatch -> {}` — that key is gone; stop. ABI 4, `feeds`.
    pub const FEED_UNWATCH: &str = "feed/unwatch";
    /// `ChoicesRequest -> ChoicesResponse` — the values an action's arguments
    /// can take right now (OBS's scenes, its inputs), for the editor.
    ///
    /// Optional: a plugin that does not know the method answers "no method",
    /// and the host reads that as "nothing to suggest". So it was added without
    /// a new [`ABI_VERSION`] — no existing message changed shape.
    pub const CHOICES: &str = "choices";
}

/// Notifications a plugin may send to the host, unprompted.
pub mod notify {
    /// `VarsSet` — publish values for variables this plugin owns.
    pub const VARS_SET: &str = "vars/set";
    /// `VarsDeclare` — declare (or replace) variables at run time, e.g. one per
    /// OBS scene, which cannot be known before the plugin has connected.
    pub const VARS_DECLARE: &str = "vars/declare";
    /// `EventEmit` — something happened; automations may trigger on it.
    pub const EVENT_EMIT: &str = "event/emit";
    /// `LogLine` — a line for the plugin's log.
    pub const LOG: &str = "log";
    /// `NotifyRequest` — show a notification to the person at the machine.
    pub const NOTIFY: &str = "notify";
    /// `SettingsStore` — persist values for settings this plugin declared, such
    /// as a refresh token it was just issued. ABI 3.
    pub const SETTINGS_STORE: &str = "settings/store";
    /// `OpenUrl` — open an http(s) address in the person's browser, for an
    /// OAuth consent page. Needs the `network` permission. ABI 3.
    pub const OPEN_URL: &str = "host/open_url";
    /// `ImagePut` — hand the host an image (album art, an avatar) that a widget
    /// can show as `img:<sha256>`. ABI 3.
    pub const IMAGES_PUT: &str = "images/put";
    /// `SeriesSet` — a variable's whole recent history, for a graph: a day of
    /// prices, the hours of a forecast. ABI 4, `series`.
    pub const SERIES_SET: &str = "series/set";
}

/// `series/set`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesSet {
    /// A variable this plugin owns.
    pub name: String,
    /// Evenly spaced, oldest first; at most [`MAX_SERIES_POINTS`].
    pub points: Vec<f64>,
}

/// `feed/watch`: a key's own copy of a feed, with its arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedWatch {
    /// Who is watching: the key's id. Values go under `<plugin>.@<instance>.`.
    pub instance: String,
    pub feed: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

/// `feed/unwatch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedUnwatch {
    pub instance: String,
}

/// A feed a plugin offers: what a key gives it, and what it publishes back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedDto {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<ArgumentDto>,
    /// The values it publishes per key, by their short names: `temp`, `sky`.
    #[serde(default)]
    pub fields: Vec<FeedField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedField {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub description: String,
}

/// A ready-made key for the editor's gallery. `widget` is a widget in the
/// configuration's JSON shape, without its id or place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresetDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "one")]
    pub w: u32,
    #[serde(default = "one")]
    pub h: u32,
    pub widget: serde_json::Value,
}

fn one() -> u32 {
    1
}

/// The largest image a plugin may hand over with `images/put`, decoded.
pub const MAX_IMAGE_BYTES: usize = 256 * 1024;

/// The wire spelling of a value's kind.
///
/// One spelling, defined once, so a plugin writes `kind::TEXT` rather than a
/// bare string literal that could be mistyped into a value the host refuses.
pub mod kind {
    pub const TEXT: &str = "text";
    pub const NUMBER: &str = "number";
    pub const BOOL: &str = "bool";
    pub const CHOICE: &str = "choice";

    pub const ALL: &[&str] = &[TEXT, NUMBER, BOOL, CHOICE];
}

/// Permissions a plugin may declare. Shown to the person before install.
///
/// External plugins are processes, not sandboxed code, so most of these are a
/// *declaration the user consents to*, not something the operating system
/// enforces. The host enforces what it mediates (variables, events, notices).
pub mod permission {
    /// Talk to the local network or the internet.
    pub const NETWORK: &str = "network";
    /// Show desktop notifications.
    pub const NOTIFICATIONS: &str = "notifications";
    /// Start other programs.
    pub const PROCESSES: &str = "processes";
    /// Read or write files outside its own directory.
    pub const FILESYSTEM: &str = "filesystem";

    pub const ALL: &[&str] = &[NETWORK, NOTIFICATIONS, PROCESSES, FILESYSTEM];
}

/// Why a plugin's declaration was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AbiError {
    /// The plugin speaks a contract this host does not.
    #[error(
        "this plugin speaks plugin ABI {theirs}, but this agent speaks {ours} \
         (update one of them)"
    )]
    Version { theirs: u32, ours: String },

    /// A value the plugin supplied is not one this contract defines.
    #[error("'{value}' is not a {what} this agent understands ({expected:?})")]
    Unknown {
        what: &'static str,
        value: String,
        expected: &'static [&'static str],
    },

    /// An id that cannot be used as a namespace.
    #[error("'{0}' is not a usable plugin id (lowercase letters, digits, '-' and '_', starting with a letter)")]
    BadId(String),

    /// A variable outside the plugin's own namespace.
    #[error("variable '{name}' must start with '{prefix}'")]
    Namespace { name: String, prefix: String },

    /// The same name declared twice.
    #[error("'{0}' is declared more than once")]
    Duplicate(String),

    /// A version that is not plain semver-shaped text.
    #[error("'{0}' is not a usable plugin version (digits, letters, '.', '-' and '+', starting with a digit)")]
    BadVersion(String),

    /// Something in the manifest that cannot work, in words.
    #[error("{0}")]
    Invalid(String),
}

/// One argument an action takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArgumentDto {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub example: String,
    /// One of [`kind`].
    pub kind: String,
    #[serde(default)]
    pub choices: Vec<String>,
    /// May be left blank.
    #[serde(default)]
    pub optional: bool,
}

/// One action the plugin offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionDto {
    /// The action id used in an `Action { action }` binding.
    pub action: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<ArgumentDto>,
    /// Set when a slider can drive this action. ABI 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slider: Option<SliderDto>,
    /// Set when what this action picks can be seen to be picked: a key that
    /// runs it lights up while it is. ABI 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<ActiveDto>,
}

/// When a key running an action shows as active: while `variable` holds the
/// value of the action's `equals` argument — OBS's `set_scene` is active while
/// `obs.scene` is the scene it switches to — or, with [`capability::TOGGLES`],
/// the fixed value `is` (`"true"` when neither is given): Discord's
/// `toggle_mute` is active while `discord.voice.mute` is true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActiveDto {
    /// May hold `{{argument}}` placeholders under [`capability::TOGGLES`].
    pub variable: String,
    /// One of the action's own arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is: Option<String>,
}

impl ActiveDto {
    /// The ABI 4 form every host reads: a plain variable of the plugin's own
    /// (under `prefix`) matched to an argument. Anything else needs
    /// [`capability::TOGGLES`], under which a light may also watch another
    /// plugin's variable — a light only reads.
    pub fn is_plain(&self, prefix: &str) -> bool {
        self.equals.is_some()
            && self.is.is_none()
            && !self.variable.contains("{{")
            && self.variable.starts_with(prefix)
    }

    /// The names inside the variable's `{{…}}` placeholders.
    pub fn placeholders(&self) -> Vec<&str> {
        self.variable
            .split("{{")
            .skip(1)
            .filter_map(|rest| rest.split_once("}}").map(|(name, _)| name.trim()))
            .collect()
    }
}

/// How a slider drives an action: which argument takes its value, over what
/// range, and which variable reports the live value back so the thumb follows
/// changes made elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SliderDto {
    /// The argument the slider's value is written into.
    pub argument: String,
    #[serde(default)]
    pub min: i64,
    #[serde(default = "default_slider_max")]
    pub max: i64,
    #[serde(default = "default_slider_step")]
    pub step: i64,
    /// A variable name, possibly with `{{argument}}` placeholders filled from
    /// the action's other arguments: `system.audio.app.{{app}}.volume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// How the same channel is muted, so a slider tile can mute from its
    /// header the way a mixer strip does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mute: Option<SliderMuteDto>,
}

/// A slider's mute: an action of the same plugin that toggles it, run with the
/// slider action's other arguments, and the variable saying whether it is
/// muted — a template like [`SliderDto::feedback`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SliderMuteDto {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

fn default_slider_max() -> i64 {
    100
}

fn default_slider_step() -> i64 {
    1
}

/// A value the plugin needs before it can work — an OBS host and password, say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingDto {
    pub key: String,
    #[serde(default)]
    pub description: String,
    /// One of [`kind`].
    pub kind: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub default_value: Option<String>,
    /// A password or token: stored, passed to the plugin, never sent back out
    /// over the API.
    #[serde(default)]
    pub secret: bool,
}

/// A live value the plugin owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VariableDto {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// One of [`kind`].
    pub kind: String,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub read_only: bool,
}

/// Something a plugin can announce, which an automation can trigger on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventDto {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The payload keys this event carries, so the editor can offer a filter.
    #[serde(default)]
    pub fields: Vec<String>,
}

/// Who a plugin is and what it offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestDto {
    /// The plugin ABI this plugin was built against. Checked, not assumed.
    pub abi: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub actions: Vec<ActionDto>,
    #[serde(default)]
    pub settings: Vec<SettingDto>,
    #[serde(default)]
    pub events: Vec<EventDto>,
    /// From [`permission`].
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The [`capability`]s it uses. ABI 4.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<String>,
    /// Ready-made keys for the gallery. ABI 4, `presets`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<PresetDto>,
    /// Values per key. ABI 4, `feeds`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feeds: Vec<FeedDto>,
}

impl ManifestDto {
    /// Whether it uses `capability` — and so may send or answer what that adds.
    pub fn uses(&self, capability: &str) -> bool {
        self.abi >= 4 && self.uses.iter().any(|used| used == capability)
    }
    /// Refuse this manifest unless the plugin speaks our contract and every
    /// value in it is one we understand.
    ///
    /// Run *once, at load*, rather than letting a bad kind surface on the first
    /// press. A manifest that half-parses is worse than one that is refused: the
    /// plugin appears and then fails the moment a person uses it.
    pub fn validate(&self) -> Result<(), AbiError> {
        check_abi(self.abi)?;
        validate_id(&self.id)?;

        let mut actions = std::collections::BTreeSet::new();
        for action in &self.actions {
            if !actions.insert(action.action.as_str()) {
                return Err(AbiError::Duplicate(action.action.clone()));
            }
            for argument in &action.arguments {
                check_one("kind", &argument.kind, kind::ALL)?;
            }
            if let Some(slider) = &action.slider {
                slider.validate(self.abi, action)?;
            }
            if let Some(active) = &action.active {
                if self.abi < 4 {
                    return Err(AbiError::Invalid(format!("{}.active needs plugin ABI 4", action.action)));
                }
                let toggles = self.uses(capability::TOGGLES);
                if !active.is_plain(&self.variable_prefix()) && !toggles && active.variable.starts_with(&self.variable_prefix()) {
                    return Err(AbiError::Invalid(format!(
                        "{}.active with `is` or placeholders needs `uses: [\"toggles\"]`",
                        action.action
                    )));
                }
                if active.equals.is_some() && active.is.is_some() {
                    return Err(AbiError::Invalid(format!(
                        "{}.active takes `equals` or `is`, not both",
                        action.action
                    )));
                }
                let named = active.equals.as_deref().into_iter().chain(active.placeholders());
                for argument in named {
                    if !action.arguments.iter().any(|a| a.name == argument) {
                        return Err(AbiError::Unknown {
                            what: "active argument",
                            value: argument.to_string(),
                            expected: &["one of the action's own arguments"],
                        });
                    }
                }
                if !toggles && !active.variable.starts_with(&self.variable_prefix()) {
                    return Err(AbiError::Namespace {
                        name: active.variable.clone(),
                        prefix: self.variable_prefix(),
                    });
                }
            }
        }
        for setting in &self.settings {
            check_one("kind", &setting.kind, kind::ALL)?;
        }
        for permission in &self.permissions {
            check_one("permission", permission, permission::ALL)?;
        }
        if self.abi < 4 && !(self.uses.is_empty() && self.presets.is_empty() && self.feeds.is_empty()) {
            return Err(AbiError::Invalid("uses, presets and feeds need plugin ABI 4".into()));
        }
        for used in &self.uses {
            check_one("capability", used, capability::ALL)?;
        }
        if !self.presets.is_empty() && !self.uses(capability::PRESETS) {
            return Err(AbiError::Invalid("presets need `uses: [\"presets\"]`".into()));
        }
        if !self.feeds.is_empty() && !self.uses(capability::FEEDS) {
            return Err(AbiError::Invalid("feeds need `uses: [\"feeds\"]`".into()));
        }
        let mut presets = std::collections::BTreeSet::new();
        for preset in &self.presets {
            if !presets.insert(preset.id.as_str()) {
                return Err(AbiError::Duplicate(preset.id.clone()));
            }
            if !preset.widget.is_object() || preset.w == 0 || preset.h == 0 || preset.w > 8 || preset.h > 8 {
                return Err(AbiError::Invalid(format!("preset '{}' is not a widget of 1 to 8 cells a side", preset.id)));
            }
        }
        let mut feeds = std::collections::BTreeSet::new();
        for feed in &self.feeds {
            if !feeds.insert(feed.name.as_str()) {
                return Err(AbiError::Duplicate(feed.name.clone()));
            }
            for argument in &feed.arguments {
                check_one("kind", &argument.kind, kind::ALL)?;
            }
            for field in &feed.fields {
                check_one("kind", &field.kind, kind::ALL)?;
            }
        }
        Ok(())
    }

    /// The namespace every variable this plugin owns must live under.
    pub fn variable_prefix(&self) -> String {
        format!("{}.", self.id)
    }
}

/// Check that a variable belongs to `plugin` and has a kind we understand.
pub fn validate_variable(plugin: &str, variable: &VariableDto) -> Result<(), AbiError> {
    let prefix = format!("{plugin}.");
    if !variable.name.starts_with(&prefix) || variable.name.len() == prefix.len() {
        return Err(AbiError::Namespace {
            name: variable.name.clone(),
            prefix,
        });
    }
    check_one("kind", &variable.kind, kind::ALL)
}

/// Plugin ids become variable namespaces, directory names and URL segments, so
/// they are held to the narrowest spelling that is safe in all three.
pub fn validate_id(id: &str) -> Result<(), AbiError> {
    let mut chars = id.chars();
    let ok = matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && id.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AbiError::BadId(id.to_string()))
    }
}

/// A version, checked as text before it becomes part of an install path.
///
/// The installer names a plugin's directory after its version, so a version of
/// `../../x` would unpack — and later delete — outside the plugin root. Only the
/// characters semver uses are allowed, and never two dots in a row.
pub fn validate_version(version: &str) -> Result<(), AbiError> {
    let ok = version.len() <= 64
        && version.starts_with(|c: char| c.is_ascii_digit())
        && !version.contains("..")
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if ok {
        Ok(())
    } else {
        Err(AbiError::BadVersion(version.to_string()))
    }
}

impl SliderDto {
    /// A slider must name one of its action's own arguments and a real range,
    /// and only an ABI 3 plugin may send one.
    pub fn validate(&self, abi: u32, action: &ActionDto) -> Result<(), AbiError> {
        if abi < 3 {
            return Err(AbiError::Unknown {
                what: "field under ABI 2",
                value: format!("{}.slider", action.action),
                expected: &["ABI 3 or later"],
            });
        }
        if !action.arguments.iter().any(|a| a.name == self.argument) {
            return Err(AbiError::Unknown {
                what: "slider argument",
                value: self.argument.clone(),
                expected: &["one of the action's own arguments"],
            });
        }
        if self.min >= self.max || self.step <= 0 {
            return Err(AbiError::Unknown {
                what: "slider range",
                value: format!("{}..{} step {}", self.min, self.max, self.step),
                expected: &["min < max and step > 0"],
            });
        }
        Ok(())
    }
}

/// Refuse an ABI this host does not speak, naming every one it does.
pub fn check_abi(abi: u32) -> Result<(), AbiError> {
    if supports_abi(abi) {
        Ok(())
    } else {
        Err(AbiError::Version {
            theirs: abi,
            ours: SUPPORTED_ABIS
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" or "),
        })
    }
}

fn check_one(
    what: &'static str,
    value: &str,
    expected: &'static [&'static str],
) -> Result<(), AbiError> {
    if expected.contains(&value) {
        Ok(())
    } else {
        Err(AbiError::Unknown {
            what,
            value: value.to_string(),
            expected,
        })
    }
}

/// The file name of a package's manifest, at the root of the package.
pub const PACKAGE_MANIFEST: &str = "plugin.json";

/// How a package's entry point is started.
pub mod runtime {
    /// `entry` is a JavaScript file run by the Node runtime flexdeck ships.
    pub const NODE: &str = "node";
    /// `entries` maps a platform (`windows`, `macos`, `linux`) to an executable.
    pub const NATIVE: &str = "native";
    /// No program: `recipe` names a JSON file of web data sources and
    /// gallery presets that the agent runs itself. Made without code.
    pub const RECIPE: &str = "recipe";
    /// `entry` is a Python file, run by the Python on this computer (3.10 or
    /// later). The package carries the SDK it was built with: no pip.
    pub const PYTHON: &str = "python";

    pub const ALL: &[&str] = &[NODE, NATIVE, RECIPE, PYTHON];
}

/// `plugin.json`: what a package *is*, readable without running anything.
///
/// The installer shows this — name, author, permissions — before the person
/// agrees to anything, which is why it lives in the package rather than only in
/// the running plugin's `initialize` answer. The two are then compared at load,
/// and a plugin that claims more than its package declared is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageManifest {
    pub abi: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// One of [`runtime`].
    pub runtime: String,
    /// For [`runtime::NODE`]: the script, relative to the package root.
    #[serde(default)]
    pub entry: Option<String>,
    /// For [`runtime::NATIVE`]: platform -> executable, relative to the root.
    #[serde(default)]
    pub entries: BTreeMap<String, String>,
    /// From [`permission`].
    #[serde(default)]
    pub permissions: Vec<String>,
    /// For [`runtime::RECIPE`]: the recipe file, relative to the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
}

impl PackageManifest {
    /// Refuse a package that could not be started, or that names a file
    /// outside itself.
    pub fn validate(&self) -> Result<(), AbiError> {
        check_abi(self.abi)?;
        validate_id(&self.id)?;
        validate_version(&self.version)?;
        check_one("runtime", &self.runtime, runtime::ALL)?;
        for permission in &self.permissions {
            check_one("permission", permission, permission::ALL)?;
        }
        let paths: Vec<&String> = match self.runtime.as_str() {
            runtime::NODE | runtime::PYTHON => self.entry.iter().collect(),
            runtime::RECIPE => self.recipe.iter().collect(),
            _ => self.entries.values().collect(),
        };
        if paths.is_empty() {
            return Err(AbiError::Unknown {
                what: "entry point",
                value: String::new(),
                expected: &["entry (node, python), entries (native) or recipe (recipe)"],
            });
        }
        if self.runtime == runtime::RECIPE && self.abi < 4 {
            return Err(AbiError::Invalid("a recipe needs plugin ABI 4".into()));
        }
        if self.runtime == runtime::PYTHON && self.abi < 4 {
            return Err(AbiError::Invalid("a Python plugin needs plugin ABI 4".into()));
        }
        for path in paths {
            if !is_contained_path(path) {
                return Err(AbiError::Unknown {
                    what: "entry path",
                    value: path.clone(),
                    expected: &["a relative path inside the package"],
                });
            }
        }
        Ok(())
    }

    /// The entry point for this platform, if the package has one.
    pub fn entry_for(&self, platform: &str) -> Option<&str> {
        match self.runtime.as_str() {
            runtime::NODE | runtime::PYTHON => self.entry.as_deref(),
            _ => self.entries.get(platform).map(String::as_str),
        }
    }
}

/// The platform name used in [`PackageManifest::entries`].
pub fn current_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A relative path that cannot climb out of the directory it is joined to.
///
/// Checked as text, before anything touches the disk, so an archive entry like
/// `../../startup/evil.exe` is refused by name rather than discovered by
/// canonicalising after it has been written.
pub fn is_contained_path(path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    !normalised.is_empty()
        && !normalised.starts_with('/')
        && !normalised.contains(':')
        && normalised
            .split('/')
            .all(|part| !part.is_empty() && part != ".." && part != ".")
}

/// Reading a package's `plugin.json` without unpacking anything else.
#[cfg(feature = "package")]
pub mod package {
    use std::io::Read;

    use super::{AbiError, PackageManifest, PACKAGE_MANIFEST};

    #[derive(Debug, thiserror::Error)]
    pub enum PackageError {
        #[error("the package is not a readable plugin package: {0}")]
        Unreadable(String),
        #[error(transparent)]
        Abi(#[from] AbiError),
    }

    /// The validated `plugin.json` inside a `.fdpkg`.
    pub fn read_manifest(bytes: &[u8]) -> Result<PackageManifest, PackageError> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|error| PackageError::Unreadable(error.to_string()))?;
        let file = archive.by_name(PACKAGE_MANIFEST).map_err(|_| {
            PackageError::Unreadable(format!("there is no {PACKAGE_MANIFEST} at its root"))
        })?;
        let mut text = String::new();
        file.take(1024 * 1024)
            .read_to_string(&mut text)
            .map_err(|error| PackageError::Unreadable(error.to_string()))?;
        let manifest: PackageManifest = serde_json::from_str(&text).map_err(|error| {
            PackageError::Unreadable(format!("{PACKAGE_MANIFEST} is not valid: {error}"))
        })?;
        manifest.validate()?;
        Ok(manifest)
    }
}

/// The first message the host sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeParams {
    /// The host's [`ABI_VERSION`]. A plugin may refuse to continue.
    pub abi: u32,
    /// This plugin's settings, with defaults filled in.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    /// Where the plugin may keep its own files.
    #[serde(default)]
    pub data_dir: Option<String>,
    /// The [`capability`]s this host offers. Sent only to an ABI 4 plugin, so
    /// an older one never sees a field it would refuse.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_capabilities: Vec<String>,
}

/// The plugin's answer to `initialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeResult {
    pub manifest: ManifestDto,
    #[serde(default)]
    pub variables: Vec<VariableDto>,
}

/// A request to check an action *and its arguments* before anything runs.
///
/// Validation happens on the host side of the catalogue first; this is for the
/// half only the plugin can answer — a hotkey that does not parse, a scene that
/// does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckRequest {
    pub action: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

/// The plugin's answer to [`CheckRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What the editor asks while someone is choosing an action's arguments.
///
/// `args` holds what is filled in so far, because one argument's choices can
/// depend on another's value: the sources *in the chosen scene*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChoicesRequest {
    pub action: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

/// One value an argument can take, and how to show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChoiceDto {
    pub value: String,
    /// Shown instead of `value` when given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The plugin's answer to [`ChoicesRequest`].
///
/// Only arguments the plugin has suggestions for appear in `arguments`; one
/// that is absent is typed freely. `error` says why there are none when there
/// should have been — "OBS is not running".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChoicesResponse {
    #[serde(default)]
    pub arguments: BTreeMap<String, Vec<ChoiceDto>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A request to run one action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvokeRequest {
    pub action: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

/// The plugin's answer to [`InvokeRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvokeResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The look a widget should now show, for a widget whose states come from
    /// this action ("provider" looks). ABI 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// A request to refresh specific variables — never all of them.
///
/// The host passes only the names someone actually watches, so an unwatched
/// plugin costs nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SampleRequest {
    #[serde(default)]
    pub wanted: Vec<String>,
}

/// The values a plugin refreshed, keyed by variable name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SampleResponse {
    #[serde(default)]
    pub values: BTreeMap<String, ValueDto>,
}

/// Sent when the user changes this plugin's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsChanged {
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

/// `vars/set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VarsSet {
    #[serde(default)]
    pub values: BTreeMap<String, ValueDto>,
}

/// `vars/declare`. `replace` drops this plugin's other run-time declarations,
/// which is how a plugin forgets a scene that was deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VarsDeclare {
    #[serde(default)]
    pub variables: Vec<VariableDto>,
    #[serde(default)]
    pub replace: bool,
}

/// `event/emit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventEmit {
    pub event: String,
    #[serde(default)]
    pub payload: BTreeMap<String, String>,
}

/// `log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogLine {
    #[serde(default = "default_level")]
    pub level: String,
    pub message: String,
}

fn default_level() -> String {
    "info".to_string()
}

/// `notify`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotifyRequest {
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// `settings/store`. Only keys the plugin's manifest declares are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsStore {
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

/// `host/open_url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenUrl {
    pub url: String,
}

impl OpenUrl {
    /// Only web addresses: a plugin must not be able to ask the host to open
    /// `file:` paths or arbitrary protocol handlers.
    pub fn is_allowed(&self) -> bool {
        let lower = self.url.to_ascii_lowercase();
        (lower.starts_with("https://") || lower.starts_with("http://"))
            && !self.url.chars().any(|c| c.is_whitespace() || c.is_control())
            && self.url.len() <= 4096
    }
}

/// `images/put`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImagePut {
    /// Lowercase hex sha256 of the decoded bytes. The host checks it.
    pub sha256: String,
    /// One of [`IMAGE_MIMES`].
    pub mime: String,
    /// The image, base64 (standard alphabet, padded).
    pub data: String,
}

/// The image types a plugin may hand over.
pub const IMAGE_MIMES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

/// A value in transit.
///
/// Always `{kind, value}` — never a bare scalar — so the consumer cannot guess
/// the type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValueDto {
    /// One of [`kind`].
    pub kind: String,
    pub value: serde_json::Value,
}

/// The JSON-RPC 2.0 envelope, in one shape for all three message types.
///
/// A request has `id` and `method`; a response has `id` and `result` or
/// `error`; a notification has `method` and no `id`. One struct rather than an
/// untagged enum, because an untagged enum's error message for a malformed line
/// is "data did not match any variant", which names nothing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Message {
    #[serde(default = "jsonrpc")]
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

fn jsonrpc() -> String {
    "2.0".to_string()
}

impl Message {
    pub fn request(id: u64, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: jsonrpc(),
            id: Some(id),
            method: Some(method.to_string()),
            params: Some(params),
            ..Self::default()
        }
    }

    pub fn notification(method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: jsonrpc(),
            method: Some(method.to_string()),
            params: Some(params),
            ..Self::default()
        }
    }

    pub fn success(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: jsonrpc(),
            id: Some(id),
            result: Some(result),
            ..Self::default()
        }
    }

    pub fn failure(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: jsonrpc(),
            id: Some(id),
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
            ..Self::default()
        }
    }

    pub fn is_response(&self) -> bool {
        self.id.is_some() && self.method.is_none()
    }

    /// One line of the wire. Never contains a newline, because serde_json
    /// escapes every newline inside a string.
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_else(|error| {
            format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"{error}"}}}}"#)
        });
        line.push('\n');
        line
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ManifestDto {
        ManifestDto {
            abi: ABI_VERSION,
            id: "obs".into(),
            name: "OBS Studio".into(),
            version: "0.1.0".into(),
            description: "Control OBS".into(),
            author: "flexdeck".into(),
            actions: vec![ActionDto {
                action: "start_recording".into(),
                description: "Start recording".into(),
                arguments: vec![],
                slider: None,
                active: None,
            }],
            settings: vec![SettingDto {
                key: "port".into(),
                description: "obs-websocket port".into(),
                kind: kind::NUMBER.into(),
                choices: vec![],
                default_value: Some("4455".into()),
                secret: false,
            }],
            events: vec![EventDto {
                name: "scene_changed".into(),
                description: "The program scene changed".into(),
                fields: vec!["scene".into()],
            }],
            permissions: vec![permission::NETWORK.into()],
            uses: Vec::new(),
            presets: Vec::new(),
            feeds: Vec::new(),
        }
    }

    #[test]
    fn a_manifest_round_trips_through_json() {
        let manifest = sample_manifest();
        let text = serde_json::to_string(&manifest).expect("serialise");
        let back: ManifestDto = serde_json::from_str(&text).expect("deserialise");
        assert_eq!(manifest, back);
    }

    #[test]
    fn the_wire_uses_camel_case_like_the_rest_of_flexdeck() {
        let text = serde_json::to_string(&sample_manifest()).expect("serialise");
        assert!(text.contains("\"defaultValue\""), "{text}");
        assert!(!text.contains("default_value"), "{text}");
    }

    #[test]
    fn a_plugin_built_against_another_abi_is_refused_with_both_numbers() {
        let mut manifest = sample_manifest();
        manifest.abi = ABI_VERSION + 1;
        let err = manifest.validate().expect_err("must refuse");
        assert_eq!(
            err,
            AbiError::Version {
                theirs: ABI_VERSION + 1,
                ours: "2 or 3 or 4".into(),
            }
        );
        let text = err.to_string();
        assert!(text.contains(&(ABI_VERSION + 1).to_string()), "{text}");
        assert!(text.contains(&ABI_VERSION.to_string()), "{text}");
        manifest.abi = 1;
        assert!(manifest.validate().is_err(), "ABI 1 is gone");
    }

    #[test]
    fn an_abi_2_plugin_still_loads() {
        let mut manifest = sample_manifest();
        manifest.abi = 2;
        manifest.validate().expect("v2 is still spoken");
        let mut package = package();
        package.abi = 2;
        package.validate().expect("v2 packages still install");
    }

    fn active_manifest(active: ActiveDto, uses: &[&str]) -> ManifestDto {
        let mut manifest = sample_manifest();
        manifest.actions = vec![ActionDto {
            action: "toggle_mute".into(),
            description: String::new(),
            arguments: vec![ArgumentDto {
                name: "input".into(),
                description: String::new(),
                example: "Mic".into(),
                kind: kind::TEXT.into(),
                choices: vec![],
                optional: false,
            }],
            slider: None,
            active: Some(active),
        }];
        manifest.uses = uses.iter().map(|used| used.to_string()).collect();
        manifest
    }

    fn active(variable: &str, equals: Option<&str>, is: Option<&str>) -> ActiveDto {
        ActiveDto {
            variable: variable.into(),
            equals: equals.map(str::to_string),
            is: is.map(str::to_string),
        }
    }

    #[test]
    fn a_plain_active_needs_no_capability() {
        active_manifest(active("obs.scene", Some("input"), None), &[]).validate().expect("plain");
        let text = serde_json::to_string(&active("obs.scene", Some("input"), None)).expect("serialise");
        assert_eq!(text, r#"{"variable":"obs.scene","equals":"input"}"#, "what an older host reads");
    }

    #[test]
    fn a_toggle_active_needs_the_toggles_capability() {
        let toggle = active("obs.muted", None, None);
        assert!(active_manifest(toggle.clone(), &[]).validate().is_err());
        active_manifest(toggle, &[capability::TOGGLES]).validate().expect("with toggles");
        active_manifest(active("obs.mode", None, Some("ptt")), &[capability::TOGGLES]).validate().expect("is");
        let templated = active("obs.input.{{input}}.muted", None, None);
        assert!(active_manifest(templated.clone(), &[]).validate().is_err());
        active_manifest(templated, &[capability::TOGGLES]).validate().expect("placeholder of an argument");
    }

    #[test]
    fn a_toggle_active_is_refused_when_it_names_what_is_not_there() {
        let both = active("obs.muted", Some("input"), Some("true"));
        assert!(active_manifest(both, &[capability::TOGGLES]).validate().is_err());
        let stray = active("obs.input.{{device}}.muted", None, None);
        assert!(matches!(
            active_manifest(stray, &[capability::TOGGLES]).validate(),
            Err(AbiError::Unknown { .. })
        ));
        let elsewhere = active("system.audio.muted", None, None);
        assert!(matches!(active_manifest(elsewhere.clone(), &[]).validate(), Err(AbiError::Namespace { .. })));
        active_manifest(elsewhere, &[capability::TOGGLES]).validate().expect("a light may watch another plugin's value");
    }

    fn slider_action() -> ActionDto {
        ActionDto {
            action: "set_volume".into(),
            description: String::new(),
            arguments: vec![ArgumentDto {
                name: "percent".into(),
                description: String::new(),
                example: "50".into(),
                kind: kind::NUMBER.into(),
                choices: vec![],
                optional: false,
            }],
            slider: Some(SliderDto {
                argument: "percent".into(),
                min: 0,
                max: 100,
                step: 1,
                feedback: Some("obs.volume".into()),
                mute: None,
            }),
            active: None,
        }
    }

    #[test]
    fn a_slider_must_name_its_own_argument_and_a_real_range() {
        let mut manifest = sample_manifest();
        manifest.actions.push(slider_action());
        manifest.validate().expect("valid slider");

        let mut wrong = manifest.clone();
        wrong.actions[1].slider.as_mut().unwrap().argument = "level".into();
        assert!(wrong.validate().is_err(), "unknown argument");

        let mut empty = manifest.clone();
        empty.actions[1].slider.as_mut().unwrap().max = 0;
        assert!(empty.validate().is_err(), "min == max");

        let mut old = manifest;
        old.abi = 2;
        assert!(old.validate().is_err(), "sliders are ABI 3");
    }

    #[test]
    fn a_v2_answer_without_state_still_parses_and_a_v3_one_carries_it() {
        let v2: InvokeResponse = serde_json::from_str(r#"{"ok":true}"#).expect("v2");
        assert_eq!(v2.state, None);
        let v3: InvokeResponse = serde_json::from_str(r#"{"ok":true,"state":"on"}"#).expect("v3");
        assert_eq!(v3.state.as_deref(), Some("on"));
        // A response without a state serialises exactly as ABI 2 did.
        let text = serde_json::to_string(&v2).expect("serialise");
        assert_eq!(text, r#"{"ok":true}"#);
    }

    #[test]
    fn only_web_addresses_may_be_opened() {
        for good in ["https://accounts.spotify.com/authorize?x=1", "http://127.0.0.1:47823/cb"] {
            assert!(OpenUrl { url: good.into() }.is_allowed(), "{good}");
        }
        for bad in [
            "file:///C:/Windows/System32/cmd.exe",
            "ms-settings:privacy",
            "javascript:alert(1)",
            "https://x.com/a b",
            "https://x.com/\nb",
            "",
        ] {
            assert!(!OpenUrl { url: bad.into() }.is_allowed(), "{bad}");
        }
    }

    #[test]
    fn an_unknown_argument_kind_is_refused_at_load_not_at_press() {
        let mut manifest = sample_manifest();
        manifest.actions[0].arguments.push(ArgumentDto {
            name: "level".into(),
            description: "A slider".into(),
            example: "50".into(),
            kind: "slider".into(),
            choices: vec![],
            optional: false,
        });
        let err = manifest.validate().expect_err("must refuse");
        assert!(
            matches!(err, AbiError::Unknown { what: "kind", .. }),
            "{err}"
        );
    }

    #[test]
    fn an_unknown_permission_is_refused() {
        let mut manifest = sample_manifest();
        manifest.permissions.push("root".into());
        let err = manifest.validate().expect_err("must refuse");
        assert!(matches!(
            err,
            AbiError::Unknown {
                what: "permission",
                ..
            }
        ));
    }

    #[test]
    fn ids_are_held_to_a_namespace_safe_spelling() {
        for good in ["obs", "my-plugin", "a1_b"] {
            validate_id(good).unwrap_or_else(|e| panic!("{good}: {e}"));
        }
        for bad in ["", "OBS", "1obs", "o.bs", "../x", "o bs"] {
            assert!(validate_id(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn a_duplicate_action_is_refused() {
        let mut manifest = sample_manifest();
        manifest.actions.push(manifest.actions[0].clone());
        assert!(matches!(
            manifest.validate(),
            Err(AbiError::Duplicate(name)) if name == "start_recording"
        ));
    }

    #[test]
    fn a_variable_outside_the_plugins_namespace_is_refused() {
        let variable = VariableDto {
            name: "system.audio.volume".into(),
            description: String::new(),
            kind: kind::NUMBER.into(),
            unit: None,
            choices: vec![],
            read_only: true,
        };
        assert!(matches!(
            validate_variable("obs", &variable),
            Err(AbiError::Namespace { .. })
        ));
        let own = VariableDto {
            name: "obs.scene".into(),
            ..variable
        };
        validate_variable("obs", &own).expect("own namespace");
    }

    #[test]
    fn unknown_fields_are_rejected_rather_than_ignored() {
        let json = r#"{"abi":2,"id":"x","name":"X","version":"0","future":true}"#;
        let err = serde_json::from_str::<ManifestDto>(json).expect_err("must reject");
        assert!(err.to_string().contains("future"), "{err}");
    }

    #[test]
    fn a_valid_manifest_passes_and_a_response_may_omit_error() {
        sample_manifest().validate().expect("valid");
        let response: CheckResponse = serde_json::from_str(r#"{"ok":true}"#).expect("parse");
        assert!(response.ok);
        assert!(response.error.is_none());
    }

    fn package() -> PackageManifest {
        PackageManifest {
            abi: ABI_VERSION,
            id: "obs".into(),
            name: "OBS".into(),
            version: "0.1.0".into(),
            description: String::new(),
            author: String::new(),
            homepage: None,
            runtime: runtime::NODE.into(),
            entry: Some("dist/index.js".into()),
            entries: BTreeMap::new(),
            permissions: vec![permission::NETWORK.into()],
            recipe: None,
        }
    }

    #[test]
    fn a_package_with_an_entry_inside_itself_is_accepted() {
        package().validate().expect("valid");
        assert_eq!(package().entry_for("windows"), Some("dist/index.js"));
    }

    #[test]
    fn a_package_entry_that_climbs_out_is_refused() {
        for bad in [
            "../x.js",
            "/etc/x",
            "C:/x.exe",
            "a/../../b",
            r"a\..\b",
            "",
            "./x",
        ] {
            let mut package = package();
            package.entry = Some(bad.into());
            assert!(package.validate().is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn a_package_version_that_could_name_another_directory_is_refused() {
        for good in ["0.1.0", "1.2.3-beta.1", "2.0.0+build.7"] {
            let mut package = package();
            package.version = good.into();
            package.validate().expect(good);
        }
        for bad in [
            "../../x",
            "1.0/../..",
            r"1.0\x",
            "",
            "v1.0.0",
            "1..0",
            "1.0.0 ",
        ] {
            let mut package = package();
            package.version = bad.into();
            assert!(package.validate().is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn a_native_package_names_an_executable_per_platform() {
        let mut package = package();
        package.runtime = runtime::NATIVE.into();
        package.entry = None;
        assert!(package.validate().is_err(), "no entries at all");
        package
            .entries
            .insert("windows".into(), "bin/obs.exe".into());
        package.validate().expect("valid");
        assert_eq!(package.entry_for("windows"), Some("bin/obs.exe"));
        assert_eq!(package.entry_for("linux"), None);
    }

    #[test]
    fn what_the_typescript_sdk_sends_is_exactly_what_this_crate_reads() {
        // Written by `packages/plugin-sdk`'s tests from a real definition. Every
        // DTO here refuses unknown fields, so a field the SDK added and this
        // crate did not — or a spelling that drifted — fails right here.
        let text = include_str!("../../../contract/fixtures/plugin/initialize.json");
        let result: InitializeResult = serde_json::from_str(text).expect("the SDK's answer parses");
        result.manifest.validate().expect("and is valid");
        assert_eq!(result.manifest.abi, 2, "the frozen ABI 2 fixture");
        assert!(result.manifest.settings.iter().any(|s| s.secret));
        for variable in &result.variables {
            validate_variable(&result.manifest.id, variable).expect("namespaced");
        }
    }

    #[test]
    fn what_the_typescript_sdk_sends_at_abi_4_is_exactly_what_this_crate_reads() {
        let text = include_str!("../../../contract/fixtures/plugin/initialize.v4.json");
        let result: InitializeResult = serde_json::from_str(text).expect("the SDK's answer parses");
        result.manifest.validate().expect("and is valid");
        assert_eq!(result.manifest.abi, ABI_VERSION, "the SDK and this crate agree on the version");
        assert!(result.manifest.uses(capability::FEEDS) && result.manifest.uses(capability::PRESETS));
        assert_eq!(result.manifest.feeds[0].fields[0].name, "temp");
        assert_eq!(result.manifest.presets[0].widget["face"]["layout"], "FACE_LAYOUT_DIAL");
    }

    #[test]
    fn abi_4_additions_need_abi_4_and_the_capability_named() {
        let mut manifest = sample_manifest();
        manifest.abi = 3;
        manifest.uses = vec![capability::SERIES.into()];
        assert!(manifest.validate().is_err(), "uses under 3");
        manifest.abi = 4;
        manifest.validate().expect("series used at 4");
        manifest.feeds = vec![FeedDto { name: "room".into(), description: String::new(), arguments: vec![], fields: vec![] }];
        assert!(manifest.validate().unwrap_err().to_string().contains("feeds need"));
        manifest.uses.push("teleport".into());
        assert!(manifest.validate().is_err(), "an unknown capability");
    }

    #[test]
    fn what_the_typescript_sdk_sends_at_abi_3_is_exactly_what_this_crate_reads() {
        let text = include_str!("../../../contract/fixtures/plugin/initialize.v3.json");
        let result: InitializeResult = serde_json::from_str(text).expect("the SDK's answer parses");
        result.manifest.validate().expect("and is valid");
        assert_eq!(result.manifest.abi, 3, "the frozen ABI 3 fixture");
        assert!(
            result.manifest.actions.iter().any(|a| a.slider.is_some()),
            "the fixture exercises slider metadata"
        );
        assert!(result.manifest.settings.iter().any(|s| s.secret));
        for variable in &result.variables {
            validate_variable(&result.manifest.id, variable).expect("namespaced");
        }
    }

    #[test]
    fn a_message_is_exactly_one_line_even_with_newlines_in_strings() {
        let message =
            Message::notification(notify::LOG, serde_json::json!({"message": "two\nlines"}));
        let line = message.to_line();
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        let back: Message = serde_json::from_str(line.trim()).expect("parse");
        assert_eq!(back, message);
        assert!(!back.is_response());
        assert!(Message::success(1, serde_json::json!({})).is_response());
    }

    #[test]
    fn a_python_package_names_its_entry_and_needs_abi_4() {
        let package = |abi: u32, entry: Option<&str>| PackageManifest {
            abi,
            id: "lights".into(),
            name: "Lights".into(),
            version: "0.1.0".into(),
            description: String::new(),
            author: String::new(),
            homepage: None,
            runtime: runtime::PYTHON.into(),
            entry: entry.map(str::to_string),
            entries: BTreeMap::new(),
            permissions: vec![],
            recipe: None,
        };
        package(4, Some("plugin.py")).validate().expect("a Python package");
        assert_eq!(package(4, Some("plugin.py")).entry_for("windows"), Some("plugin.py"));
        assert!(package(3, Some("plugin.py")).validate().is_err(), "ABI 4 or later");
        assert!(package(4, None).validate().is_err(), "an entry file");
        assert!(package(4, Some("../escape.py")).validate().is_err(), "inside the package");
    }
}

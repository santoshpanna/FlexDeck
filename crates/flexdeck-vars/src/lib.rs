//! Typed, namespaced variables for the flexdeck agent.
//!
//! A variable is a *named, typed, live value* that a plugin owns and that an
//! automation can read. `system.audio.volume` is a number, `obs.streaming` is a
//! boolean, `vars.stream_live` is one the **user** owns.
//!
//! # Why this is its own crate
//!
//! Two separate concerns need the same vocabulary and must agree on it exactly:
//!
//! * a plugin *declares* the variables it can read and *refreshes* their values;
//! * the automation engine *reads* them to evaluate conditions, and *watches* a
//!   subset so it can react to a change.
//!
//! If those two ever disagreed about a type, `volume >= 50` could silently
//! compare a number against text and always be false — a bug with no error and
//! no symptom other than "my automation does not fire". So the type system is
//! shared, not duplicated.
//!
//! # Why sampling is lazy, and how that is made true
//!
//! The user's constraint is explicit: an idle agent must be *truly* idle. A
//! variable that nothing looks at must cost **zero wake-ups**, so a plugin only
//! samples the variables somebody is interested in.
//!
//! "Somebody" is two different parties, and they are tracked differently:
//!
//! * the **engine** holds an [`InterestGuard`] for as long as an enabled
//!   automation references the variable — an RAII guard, so a deleted automation
//!   cannot leak interest;
//! * a **UI page** cannot hold a guard, because the agent does not know when a
//!   browser tab closes. It [`lease`](VariableStore::lease)s instead: every poll
//!   refreshes a lease with a TTL, and when the polls stop the lease expires.
//!
//! [`interested`](VariableStore::interested) then answers "who should be sampled
//! right now", taking an injected `now` so the expiry boundary is testable
//! without sleeping — the same discipline the pairing windows use.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// How many change events a slow subscriber may fall behind before it starts
/// losing them. Closing a live agent's config broadcast uses the same idea.
///
/// Losing an event is deliberately *not* fatal: a consumer that only needs the
/// current value re-reads it, and a consumer that needs every edge must not rely
/// on a queue that can drop.
pub const CHANGE_BUFFER: usize = 64;

/// The person's own variables: made by setting them, kept across restarts.
pub const USER_PREFIX: &str = "vars.";

/// Whether `name` is one the person may make: `vars.` and a name of letters,
/// digits, `_` and `-`, dotted.
pub fn is_user_name(name: &str) -> bool {
    name.strip_prefix(USER_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= 64
            && rest.split('.').all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
    })
}

/// The type of a variable, and therefore the type of any literal it may be
/// compared against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Bool,
    Number,
    /// Free text.
    Text,
    /// One of a fixed set of strings. A separate kind rather than a plain
    /// `Text` so the editor can offer the list instead of a free-text box, and
    /// so a typo is refused at the write rather than silently never matching.
    Choice,
}

/// A variable's live value.
///
/// Serialised as `{"kind": "...", "value": ...}` rather than as a bare scalar,
/// because a client that has to *guess* whether `1` is a number or the string
/// `"1"` will eventually guess wrong, and the tunnel and the REST surface
/// disagree about int64 representation on purpose (see the tunnel module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    pub fn kind(&self) -> Kind {
        match self {
            Value::Bool(_) => Kind::Bool,
            Value::Number(_) => Kind::Number,
            Value::Text(_) => Kind::Text,
        }
    }

    /// The value as the comparison side of an automation sees it.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// Everything anyone needs to know about a variable *without* its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableSpec {
    pub name: String,
    pub description: String,
    pub kind: Kind,
    /// A display suffix, e.g. `%`. Never interpreted.
    pub unit: Option<String>,
    /// The permitted values when [`Kind::Choice`], empty otherwise.
    pub choices: Vec<String>,
    /// Whether the plugin owns this value. A read-only variable cannot be set by
    /// an automation, only refreshed by its own plugin.
    pub read_only: bool,
    /// For numbers: a change smaller than this is stored but **not** published,
    /// so a jittering value cannot wake the engine on every sample.
    pub threshold: Option<f64>,
}

impl VariableSpec {
    pub fn new(name: impl Into<String>, kind: Kind, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            kind,
            unit: None,
            choices: Vec::new(),
            read_only: false,
            threshold: None,
        }
    }

    /// The plugin owns this value; only [`VariableStore::refresh`] may change it.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn choices(mut self, choices: &[&str]) -> Self {
        self.choices = choices.iter().map(|choice| (*choice).to_string()).collect();
        self
    }

    pub fn threshold(mut self, delta: f64) -> Self {
        self.threshold = Some(delta);
        self
    }

    /// Whether `value` is a legal value for this variable.
    ///
    /// Refusing a mismatched write is the whole point of declaring a kind: a
    /// boolean written into a number would otherwise make every comparison
    /// against it quietly false.
    pub fn accepts(&self, value: &Value) -> Result<(), VarError> {
        let actual = value.kind();
        let matches = match self.kind {
            Kind::Bool => actual == Kind::Bool,
            Kind::Number => actual == Kind::Number,
            Kind::Text => actual == Kind::Text,
            Kind::Choice => actual == Kind::Text,
        };
        if !matches {
            return Err(VarError::BadKind {
                name: self.name.clone(),
                expected: self.kind,
                actual,
            });
        }

        if self.kind == Kind::Choice {
            let chosen = value.as_text().unwrap_or_default();
            if !self.choices.iter().any(|choice| choice == chosen) {
                return Err(VarError::NotAChoice {
                    name: self.name.clone(),
                    value: chosen.to_string(),
                    choices: self.choices.clone(),
                });
            }
        }

        Ok(())
    }
}

/// A published change. Only real changes are published.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub name: String,
    pub value: Value,
    /// `None` means the variable had no value before this one.
    pub previous: Option<Value>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VarError {
    #[error("no variable called '{0}' is declared")]
    Unknown(String),

    #[error("'{0}' is read-only: only its own plugin may change it")]
    ReadOnly(String),

    #[error("'{name}' is {expected:?}, but a {actual:?} was written")]
    BadKind {
        name: String,
        expected: Kind,
        actual: Kind,
    },

    #[error("'{name}' must be one of {choices:?}, not '{value}'")]
    NotAChoice {
        name: String,
        value: String,
        choices: Vec<String>,
    },
}

/// Whether a write actually changed anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    Yes,
    No,
}

#[derive(Debug, Default, Clone, Copy)]
struct Interest {
    guards: u32,
    /// `None` when no lease is held. Leases expire; guards do not.
    lease_until: Option<i64>,
}

#[derive(Debug, Default)]
struct State {
    values: BTreeMap<String, Value>,
    interest: BTreeMap<String, Interest>,
}

/// Holds interest in one variable for as long as it lives.
///
/// Drop it and the interest is released, which is what stops a deleted
/// automation from keeping a plugin sampling forever.
#[derive(Debug)]
pub struct InterestGuard {
    store: Arc<VariableStore>,
    name: String,
}

impl InterestGuard {
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for InterestGuard {
    fn drop(&mut self) {
        self.store.release_guard(&self.name);
    }
}

/// Live values, their declarations, and who is currently interested.
#[derive(Debug)]
pub struct VariableStore {
    specs: RwLock<BTreeMap<String, VariableSpec>>,
    state: Mutex<State>,
    changes: broadcast::Sender<Change>,
}

impl Default for VariableStore {
    fn default() -> Self {
        Self::new()
    }
}

impl VariableStore {
    pub fn new() -> Self {
        let (changes, _) = broadcast::channel(CHANGE_BUFFER);
        Self {
            specs: RwLock::new(BTreeMap::new()),
            state: Mutex::new(State::default()),
            changes,
        }
    }

    /// Declare a variable. Redeclaring a name replaces its spec but keeps its
    /// value, because a plugin re-registering must not blank a live reading.
    pub fn declare(&self, spec: VariableSpec) {
        self.specs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(spec.name.clone(), spec);
    }

    pub fn declare_all(&self, specs: impl IntoIterator<Item = VariableSpec>) {
        for spec in specs {
            self.declare(spec);
        }
    }

    /// Forget a variable and its value. Returns whether it was declared.
    ///
    /// The path a plugin's removal takes: an uninstalled plugin's readings must
    /// not linger as values nothing will ever refresh again.
    pub fn undeclare(&self, name: &str) -> bool {
        let removed = self
            .specs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name)
            .is_some();
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values
            .remove(name);
        removed
    }

    /// Forget every variable whose name starts with `prefix`. Returns how many.
    pub fn undeclare_prefix(&self, prefix: &str) -> usize {
        let names: Vec<String> = self
            .specs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect();
        names.iter().filter(|name| self.undeclare(name)).count()
    }

    /// Every declaration, ordered by name so a catalogue does not shuffle.
    pub fn specs(&self) -> Vec<VariableSpec> {
        self.specs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    pub fn spec(&self, name: &str) -> Option<VariableSpec> {
        self.specs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values
            .get(name)
            .cloned()
    }

    /// Every declared variable with its current value, ordered by name.
    ///
    /// `None` means "declared but never sampled", which is a real state under
    /// lazy sampling and must be reportable rather than invented as `0`/`false`.
    pub fn catalogue(&self) -> Vec<(VariableSpec, Option<Value>)> {
        // Take the specs first and let that lock go, so only one lock is ever
        // held at a time and there is no lock-ordering rule to remember.
        let specs = self.specs();
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        specs
            .into_iter()
            .map(|spec| {
                let value = state.values.get(&spec.name).cloned();
                (spec, value)
            })
            .collect()
    }

    /// Change a value on behalf of a user or an automation.
    ///
    /// Refuses an undeclared name and refuses a read-only variable: the first is
    /// a typo that would otherwise become a silently inert automation, and the
    /// second would let an automation fight its own source.
    ///
    /// The one exception is the person's own namespace, [`USER_PREFIX`]: a
    /// `vars.` name is theirs to make, and the first write declares it, of the
    /// kind written. After that it is held to that kind like any other.
    pub fn set(&self, name: &str, value: Value) -> Result<Changed, VarError> {
        if self.spec(name).is_none() && is_user_name(name) {
            self.declare(VariableSpec::new(name, value.kind(), "A value of your own"));
        }
        let spec = self
            .spec(name)
            .ok_or_else(|| VarError::Unknown(name.to_string()))?;
        if spec.read_only {
            return Err(VarError::ReadOnly(name.to_string()));
        }
        self.write(&spec, value)
    }

    /// Change a value on behalf of the plugin that owns it.
    ///
    /// This is the *sampler* path and deliberately bypasses `read_only`, which
    /// exists to stop anyone **else** writing it. A plugin refusing to update
    /// its own reading would make the flag useless in the other direction.
    pub fn refresh(&self, name: &str, value: Value) -> Result<Changed, VarError> {
        let spec = self
            .spec(name)
            .ok_or_else(|| VarError::Unknown(name.to_string()))?;
        self.write(&spec, value)
    }

    fn write(&self, spec: &VariableSpec, value: Value) -> Result<Changed, VarError> {
        spec.accepts(&value)?;

        let previous = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.values.insert(spec.name.clone(), value.clone())
        };

        // A jittering number must not wake the engine on every sample: inside
        // the threshold the value is stored but nothing is published.
        if let (Some(threshold), Value::Number(new), Some(Value::Number(old))) =
            (spec.threshold, &value, &previous)
        {
            if (new - old).abs() < threshold {
                return Ok(Changed::No);
            }
        }

        if previous.as_ref() == Some(&value) {
            return Ok(Changed::No);
        }

        // A send with no subscribers is normal, not an error.
        let _ = self.changes.send(Change {
            name: spec.name.clone(),
            value,
            previous,
        });
        Ok(Changed::Yes)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }

    /// Hold interest in a variable for as long as the guard lives.
    ///
    /// This is the engine's path: an automation that references a variable keeps
    /// a guard, and deleting the automation drops it with no bookkeeping.
    pub fn interest(self: &Arc<Self>, name: impl Into<String>) -> InterestGuard {
        let name = name.into();
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.interest.entry(name.clone()).or_default().guards += 1;
        }
        InterestGuard {
            store: Arc::clone(self),
            name,
        }
    }

    /// Interest with an expiry, for a caller that cannot hold a guard — a UI
    /// page. Each poll refreshes the lease; when the polls stop it lapses.
    pub fn lease(&self, name: &str, now_ms: i64, ttl_ms: i64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .interest
            .entry(name.to_string())
            .or_default()
            .lease_until = Some(now_ms.saturating_add(ttl_ms));
    }

    /// The variables somebody is interested in right now, ordered by name.
    ///
    /// `now` is passed in rather than read here, so the lease boundary is
    /// testable at and just past expiry with no sleeping.
    pub fn interested(&self, now_ms: i64) -> BTreeSet<String> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .interest
            .iter()
            .filter(|(_, interest)| {
                interest.guards > 0 || interest.lease_until.is_some_and(|until| until > now_ms)
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Drop entries nobody is interested in any more. Returns how many went.
    pub fn prune(&self, now_ms: i64) -> usize {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let before = state.interest.len();
        state.interest.retain(|_, interest| {
            interest.guards > 0 || interest.lease_until.is_some_and(|until| until > now_ms)
        });
        before - state.interest.len()
    }

    fn release_guard(&self, name: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(interest) = state.interest.get_mut(name) {
            interest.guards = interest.guards.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume() -> VariableSpec {
        VariableSpec::new("system.audio.volume", Kind::Number, "Output volume")
            .unit("%")
            .read_only()
    }

    fn store_with_volume() -> Arc<VariableStore> {
        let store = Arc::new(VariableStore::new());
        store.declare(volume());
        store
    }

    #[test]
    fn a_declared_variable_is_reported_before_it_is_sampled() {
        // `None` is "declared but never read", which is a real state under lazy
        // sampling. Inventing `0` here would make the editor show a number that
        // no plugin ever measured.
        let store = store_with_volume();
        let catalogue = store.catalogue();

        assert_eq!(catalogue.len(), 1);
        assert_eq!(catalogue[0].0.name, "system.audio.volume");
        assert_eq!(catalogue[0].1, None);
    }

    #[test]
    fn a_plugin_can_refresh_a_read_only_variable_but_an_automation_cannot() {
        let store = store_with_volume();

        assert_eq!(
            store.refresh("system.audio.volume", Value::Number(72.0)),
            Ok(Changed::Yes)
        );
        assert_eq!(
            store.set("system.audio.volume", Value::Number(10.0)),
            Err(VarError::ReadOnly("system.audio.volume".to_string()))
        );
    }

    #[test]
    fn a_write_of_the_same_value_publishes_nothing() {
        let store = store_with_volume();
        let mut changes = store.subscribe();

        store
            .refresh("system.audio.volume", Value::Number(50.0))
            .unwrap();
        assert_eq!(
            store.refresh("system.audio.volume", Value::Number(50.0)),
            Ok(Changed::No)
        );

        let change = changes.try_recv().expect("one change was published");
        assert_eq!(change.value, Value::Number(50.0));
        assert!(
            changes.try_recv().is_err(),
            "the repeat must not be published"
        );
    }

    #[test]
    fn a_jitter_inside_the_threshold_is_stored_but_not_published() {
        let store = Arc::new(VariableStore::new());
        store.declare(
            VariableSpec::new("system.cpu.usage", Kind::Number, "CPU")
                .read_only()
                .threshold(5.0),
        );
        let mut changes = store.subscribe();

        store
            .refresh("system.cpu.usage", Value::Number(40.0))
            .unwrap();
        assert_eq!(
            store.refresh("system.cpu.usage", Value::Number(43.0)),
            Ok(Changed::No)
        );

        assert!(changes.try_recv().is_ok(), "the first sample publishes");
        assert!(
            changes.try_recv().is_err(),
            "43 is within 5 of 40, so the engine must not be woken"
        );
        assert_eq!(store.get("system.cpu.usage"), Some(Value::Number(43.0)));
    }

    #[test]
    fn a_wrong_type_is_refused_rather_than_coerced() {
        let store = store_with_volume();

        assert_eq!(
            store.refresh("system.audio.volume", Value::Bool(true)),
            Err(VarError::BadKind {
                name: "system.audio.volume".to_string(),
                expected: Kind::Number,
                actual: Kind::Bool,
            })
        );
    }

    #[test]
    fn an_undeclared_name_is_refused_rather_than_created() {
        let store = store_with_volume();

        // Outside the person's own `vars.` namespace, a name nobody declared is a typo.
        assert_eq!(
            store.set("system.audio.volum", Value::Bool(true)),
            Err(VarError::Unknown("system.audio.volum".to_string()))
        );
    }

    #[test]
    fn a_choice_outside_the_declared_set_is_refused() {
        let store = Arc::new(VariableStore::new());
        store.declare(
            VariableSpec::new("system.audio.output_device", Kind::Choice, "Output")
                .choices(&["speaker", "headphones"]),
        );

        assert_eq!(
            store.set(
                "system.audio.output_device",
                Value::Text("speaker".to_string())
            ),
            Ok(Changed::Yes)
        );
        assert!(matches!(
            store.set(
                "system.audio.output_device",
                Value::Text("monitor".to_string())
            ),
            Err(VarError::NotAChoice { .. })
        ));
    }

    #[test]
    fn interest_is_held_by_a_guard_and_released_when_it_drops() {
        let store = store_with_volume();

        {
            let _guard = store.interest("system.audio.volume");
            assert_eq!(
                store.interested(0).into_iter().collect::<Vec<_>>(),
                vec!["system.audio.volume".to_string()]
            );
        }

        assert!(
            store.interested(0).is_empty(),
            "dropping the guard releases it"
        );
    }

    #[test]
    fn a_lease_lapses_at_its_expiry_and_not_before() {
        // The boundary follows the repo's expiry rule (as the pairing windows
        // do): the instant itself is CLOSED, so a lease written for
        // now + ttl is alive one millisecond before it and gone exactly at it.
        let store = store_with_volume();
        store.lease("system.audio.volume", 1_000, 500);

        assert!(!store.interested(1_499).is_empty(), "alive just before");
        assert!(store.interested(1_500).is_empty(), "lapsed at the boundary");
    }

    #[test]
    fn polling_again_refreshes_a_lease() {
        let store = store_with_volume();
        store.lease("system.audio.volume", 0, 1_000);

        assert!(store.interested(1_500).is_empty());
        store.lease("system.audio.volume", 1_500, 1_000);

        assert!(
            !store.interested(2_000).is_empty(),
            "the second poll renewed it"
        );
    }

    #[test]
    fn pruning_forgets_lapsed_leases_only() {
        let store = store_with_volume();
        store.lease("system.audio.volume", 0, 100);
        let _guard = store.interest("vars.kept");

        assert_eq!(store.prune(10_000), 1);
        assert_eq!(
            store.interested(10_000).into_iter().collect::<Vec<_>>(),
            vec!["vars.kept".to_string()]
        );
    }

    #[test]
    fn a_value_serialises_with_its_kind_so_a_client_never_guesses() {
        let json = serde_json::to_value(Value::Number(72.0)).expect("serialises");
        assert_eq!(json, serde_json::json!({ "kind": "number", "value": 72.0 }));

        let json = serde_json::to_value(Value::Bool(true)).expect("serialises");
        assert_eq!(json, serde_json::json!({ "kind": "bool", "value": true }));
    }

    #[test]
    fn a_users_own_value_is_made_by_setting_it_and_then_keeps_its_kind() {
        let store = VariableStore::new();
        assert_eq!(store.set("vars.temp", Value::Number(21.5)).unwrap(), Changed::Yes);
        assert_eq!(store.spec("vars.temp").unwrap().kind, Kind::Number);
        assert!(!store.spec("vars.temp").unwrap().read_only);
        assert!(store.set("vars.temp", Value::Text("warm".into())).is_err(), "the kind it was made with");
        assert!(matches!(store.set("plugin.thing", Value::Bool(true)), Err(VarError::Unknown(_))), "only vars. is the person's");
        for bad in ["vars.", "vars..x", "vars.a b", "vars.$"] {
            assert!(!is_user_name(bad), "{bad}");
        }
        assert!(is_user_name("vars.room-1.temp_c"));
    }
}

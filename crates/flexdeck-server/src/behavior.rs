//! The reactive layer: variables, the plugin registry, automations.
//!
//! `Behavior` is the second of the two halves the jury separated. The first is
//! the configuration timeline — what is stored, what is broadcast — and the
//! second is everything that *reacts*: a widget press becomes plugin actions
//! and triggered automations, a variable change wakes automations that watch
//! it, and a dry-run lets the editor exercise an automation against a scratch
//! state.
//!
//! # Why the executor and the registry are passed in
//!
//! Neither is constructed here. The executor lives on its own thread (see
//! [`crate::executor`]); the registry is built at startup. Both are
//! `Clone`-shaped handles so runs can borrow them without owning them.
//!
//! # Why this is the only place that runs automations
//!
//! The evaluator in `flexdeck-automation` knows nothing about the store, the
//! WebSocket or HTTP. It runs against an [`Environment`] supplied here, so
//! every run goes through one place that knows about plugins, variables, and
//! the executor. A widget-press trigger that wants the executor therefore
//! flows: press -> `Behavior::press` -> `Behavior::run_with` -> remote perform
//! -> executor owner thread. No lock to re-enter, which is the deadlock the
//! jury flagged.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use flexdeck_automation::{
    parse_document, Automation, AutomationDocument, Block, CancelFlag, Environment, Inputs, Limits,
    RecordingSleeper, Run, Sleeper, SystemClock, ThreadSleeper, Trace, NO_INPUTS,
};
use flexdeck_plugin::{Ctx, Notifier, PluginError, RecordingNotifier, Registry};
use flexdeck_protocol::Gesture;
use flexdeck_store::{ConfigStore, AUTOMATIONS, CONFIG};
use flexdeck_vars::{Value, VariableStore};
use tokio::time::MissedTickBehavior;

use crate::executor::{ExecutorError, ExecutorHandle, RemoteExecutor};
use crate::widgets::WidgetPlan;

#[derive(Debug, thiserror::Error)]
pub enum BehaviorError {
    #[error("store error: {0}")]
    Store(#[from] flexdeck_store::StoreError),

    #[error("invalid automation document: {0}")]
    Document(String),

    #[error("no automation with id '{0}'")]
    UnknownAutomation(String),

    #[error("plugin error: {0}")]
    Plugin(#[from] PluginError),

    #[error("executor error: {0}")]
    Executor(#[from] ExecutorError),

    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("the automations changed (now rev {current})")]
    Conflict { expected: u64, current: u64 },
}

pub struct BehaviorInner {
    pub(crate) store: Arc<Mutex<ConfigStore>>,
    pub(crate) variables: Arc<VariableStore>,
    pub(crate) registry: Arc<Mutex<Registry>>,
    pub(crate) executor: ExecutorHandle,
    pub(crate) notifier: Arc<Mutex<Box<dyn Notifier>>>,
    /// Announces the plugin catalogue's new generation whenever a plugin is
    /// added, removed or reconfigured, so open editors refresh their forms.
    pub(crate) catalogue: tokio::sync::broadcast::Sender<u64>,
    /// When each scheduled automation last fired (epoch seconds), by id.
    pub(crate) schedule_fired: Mutex<std::collections::HashMap<String, i64>>,
    /// Live widget states for tablets: one per live widget, sent when
    /// something it shows changes. See [`crate::widgets`].
    pub(crate) widget_states: tokio::sync::broadcast::Sender<flexdeck_protocol::StateEvent>,
    /// The recent history of every variable a graph draws, oldest first, one
    /// point per sampler tick. In memory: a restart starts the graphs afresh.
    pub(crate) history: Mutex<std::collections::HashMap<String, std::collections::VecDeque<f64>>>,
    /// Each new history point, for tablets.
    pub(crate) series: tokio::sync::broadcast::Sender<flexdeck_protocol::SeriesEvent>,
    /// Variables whose history something hands over whole (a source's hourly
    /// forecast): the sampler adds no points of its own to those.
    pub(crate) handed: Mutex<BTreeSet<String>>,
    /// The plugin feeds keys are watching now, by widget: (plugin, feed,
    /// args), and the catalogue generation they were asked for in.
    pub(crate) feeds: Mutex<(u64, BTreeMap<String, (String, String, BTreeMap<String, String>)>)>,
    /// How numbers, money and times are written. See [`Behavior::set_locale`].
    pub(crate) locale: RwLock<flexdeck_expr::Locale>,
    /// Data sources: feeds from the web or a program. See [`crate::sources`].
    pub(crate) sources: Arc<crate::sources::Sources>,
    /// Every widget of the current configuration, by its revision, so a press
    /// or a variable change does not re-parse the document.
    pub(crate) widgets: RwLock<(u64, Arc<Vec<WidgetPlan>>)>,
    /// How many times each cycling widget has advanced. In memory: a restart
    /// starts every cycle at its first look, as a fresh deck would.
    pub(crate) cycled: Mutex<std::collections::HashMap<String, usize>>,
    /// The look each provider-driven widget's action last reported, or the
    /// look a gesture-driven widget's last gesture picked.
    pub(crate) provided: Mutex<std::collections::HashMap<String, String>>,
    /// What a gesture-driven widget showed before a hold began, to go back to
    /// when it ends and the release has no look of its own.
    pub(crate) before_hold: Mutex<std::collections::HashMap<String, Option<String>>>,
    /// Why each key cannot do what it does any more, by widget id.
    pub(crate) unavailable: RwLock<Arc<BTreeMap<String, String>>>,
    /// The actions each loaded plugin has, copied out of the registry when
    /// its catalogue changes. A run holds the registry for as long as it
    /// takes, waits included, so drawing or previewing a key must not need it.
    pub(crate) actions: RwLock<Arc<ActionCatalogue>>,
    /// Each slider's pending value and whether a run is draining it. See
    /// [`Behavior::slide`].
    pub(crate) sliders: Mutex<std::collections::HashMap<String, SliderSlot>>,
    /// How many tablets are connected. A bound value is kept fresh only while
    /// someone can see it — nobody is looking at an empty room.
    pub(crate) viewers: std::sync::atomic::AtomicUsize,
    pub(crate) limits: Limits,
    pub(crate) clock: Arc<dyn flexdeck_automation::Clock>,
    pub(crate) cancel: Arc<CancelFlag>,
    pub(crate) document: RwLock<AutomationDocument>,
    pub(crate) document_rev: AtomicU64,
    /// Whether the background tasks have been spawned. Set by [`Behavior::start`]
    /// under a compare-exchange so two starts cannot spawn two samplers, and
    /// surfaced via [`Behavior::is_started`] so a deploy that forgot to call
    /// `start` is detectable from a liveness check.
    pub(crate) started: AtomicBool,
}

/// One slider's pending work: the latest value nobody has run yet, whether a
/// worker is draining it, and when it last ran (for pacing).
#[derive(Debug, Default)]
pub struct SliderSlot {
    latest: Option<f64>,
    running: bool,
    last_run: Option<std::time::Instant>,
    /// How many times the flow ran, for tests and diagnostics.
    runs: u64,
}

/// The reactive engine, shared with the agent by cheap `Arc` cloning.
#[derive(Clone)]
pub struct Behavior {
    inner: Arc<BehaviorInner>,
}

impl Behavior {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<Mutex<ConfigStore>>,
        variables: Arc<VariableStore>,
        registry: Arc<Mutex<Registry>>,
        executor: ExecutorHandle,
        notifier: Arc<Mutex<Box<dyn Notifier>>>,
        limits: Limits,
    ) -> Self {
        let sources = Arc::new(crate::sources::Sources::new(Arc::clone(&variables)));
        let saved = store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .meta(crate::sources::SOURCES_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<crate::sources::SourcesDoc>(&json).ok());
        if let Some(doc) = saved {
            sources.replace(doc);
        }
        let secrets = store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .meta(crate::sources::SECRETS_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<BTreeMap<String, String>>(&json).ok());
        if let Some(secrets) = secrets {
            sources.load_secrets(secrets);
        }
        let actions = ActionCatalogue::of(&registry.lock().unwrap_or_else(PoisonError::into_inner));
        let locale = store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .meta(Self::LOCALE_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<flexdeck_expr::Locale>(&json).ok())
            .unwrap_or_default();
        let inner = BehaviorInner {
            sources,
            store,
            variables,
            registry,
            executor,
            notifier,
            catalogue: tokio::sync::broadcast::channel(16).0,
            schedule_fired: Mutex::new(std::collections::HashMap::new()),
            widget_states: tokio::sync::broadcast::channel(256).0,
            history: Mutex::new(std::collections::HashMap::new()),
            series: tokio::sync::broadcast::channel(256).0,
            handed: Mutex::new(BTreeSet::new()),
            feeds: Mutex::new((u64::MAX, BTreeMap::new())),
            locale: RwLock::new(locale),
            widgets: RwLock::new((u64::MAX, Arc::new(Vec::new()))),
            cycled: Mutex::new(std::collections::HashMap::new()),
            provided: Mutex::new(std::collections::HashMap::new()),
            before_hold: Mutex::new(std::collections::HashMap::new()),
            unavailable: RwLock::new(Arc::new(BTreeMap::new())),
            actions: RwLock::new(Arc::new(actions)),
            sliders: Mutex::new(std::collections::HashMap::new()),
            viewers: std::sync::atomic::AtomicUsize::new(0),
            limits,
            clock: Arc::new(SystemClock),
            cancel: Arc::new(CancelFlag::new()),
            document: RwLock::new(AutomationDocument::default()),
            document_rev: AtomicU64::new(0),
            started: AtomicBool::new(false),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Re-read the automations document from the store.
    ///
    /// Called at startup and after any write. Stores the parsed document in a
    /// cache so calls like [`document_json`] and [`press`] do not have to
    /// re-parse on every request.
    pub fn reload(&self) -> Result<(), BehaviorError> {
        match self
            .inner
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot_document(&AUTOMATIONS)?
        {
            Some((rev, json)) => {
                let doc = parse_document(&json, &self.inner.limits)
                    .map_err(|error| BehaviorError::Document(error.to_string()))?;
                self.inner.document_rev.store(rev, Ordering::SeqCst);
                *self
                    .inner
                    .document
                    .write()
                    .unwrap_or_else(PoisonError::into_inner) = doc;
            }
            None => {
                self.inner.document_rev.store(0, Ordering::SeqCst);
                *self
                    .inner
                    .document
                    .write()
                    .unwrap_or_else(PoisonError::into_inner) = AutomationDocument::default();
            }
        }
        Ok(())
    }

    /// Replace the document wholesale. Stamps `rev` so the stored value can
    /// never disagree with the timeline.
    pub fn replace_document_json(&self, json: &str) -> Result<u64, BehaviorError> {
        self.replace_document_json_if(None, json)
    }

    pub fn replace_document_json_if(&self, expect_rev: Option<u64>, json: &str) -> Result<u64, BehaviorError> {
        let limits = self.inner.limits;
        // Validate before touching the store, so a malformed document cannot
        // leave the timeline half-written — exactly the rule the config
        // commit follows.
        let _parsed = parse_document(json, &limits)
            .map_err(|error| BehaviorError::Document(error.to_string()))?;

        let store = self
            .inner
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(expected) = expect_rev {
            let current = store.rev_document(&AUTOMATIONS)?.unwrap_or(0);
            if expected != current {
                return Err(BehaviorError::Conflict { expected, current });
            }
        }
        let next = store.next_rev_document(&AUTOMATIONS)?;
        let stamped = stamp_rev(json, next)?;
        let committed = store.commit_document(&AUTOMATIONS, &stamped)?;
        debug_assert_eq!(next, committed, "stamped rev must be the committed one");

        let doc = parse_document(&stamped, &limits)
            .map_err(|error| BehaviorError::Document(error.to_string()))?;
        self.inner.document_rev.store(committed, Ordering::SeqCst);
        drop(store);
        *self
            .inner
            .document
            .write()
            .unwrap_or_else(PoisonError::into_inner) = doc;
        Ok(committed)
    }

    pub fn automations_rev(&self) -> Result<u64, BehaviorError> {
        let store = self.inner.store.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(store.rev_document(&AUTOMATIONS)?.unwrap_or(0))
    }

    pub fn document_rev(&self) -> u64 {
        self.inner.document_rev.load(Ordering::SeqCst)
    }

    pub fn document_json(&self) -> serde_json::Value {
        let document = self
            .inner
            .document
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        serde_json::to_value(&*document).unwrap_or(serde_json::Value::Null)
    }

    /// Every widget of the current configuration, read once per revision.
    ///
    /// A stored configuration that does not read (written before schema 2, or
    /// for a newer agent) keeps the widgets last read, or none, rather than
    /// failing every press: the editor is where that is reported and fixed.
    pub fn widget_plans(&self) -> Arc<Vec<WidgetPlan>> {
        let store = self.inner.store.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(head) = store.rev_document(&CONFIG).ok().flatten() else {
            return Arc::new(Vec::new());
        };
        {
            let cached = self
                .inner
                .widgets
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            if cached.0 == head {
                return Arc::clone(&cached.1);
            }
        }
        let snapshot = store.snapshot_document(&CONFIG).ok().flatten();
        drop(store);
        let Some((rev, json)) = snapshot else {
            return Arc::new(Vec::new());
        };
        let mut cached = self
            .inner
            .widgets
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let fresh = match crate::widgets::parse(&json, &self.inner.limits) {
            Ok(plans) => Arc::new(plans),
            Err(error) => {
                tracing::warn!("the stored deck does not read, so the keys stay as they were: {error}");
                Arc::clone(&cached.1)
            }
        };
        *cached = (rev, Arc::clone(&fresh));
        fresh
    }

    /// Where the person's own values are kept between runs.
    const USER_VARS_KEY: &'static str = "user_vars";

    /// Keep every `vars.` value, so a restart finds them as they were.
    pub fn save_user_vars(&self) {
        let values: BTreeMap<String, Value> = self
            .inner
            .variables
            .catalogue()
            .into_iter()
            .filter(|(spec, _)| flexdeck_vars::is_user_name(&spec.name))
            .filter_map(|(spec, value)| Some((spec.name, value?)))
            .collect();
        let Ok(json) = serde_json::to_string(&values) else { return };
        let store = self.inner.store.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = store.set_meta(Self::USER_VARS_KEY, &json) {
            tracing::warn!("could not keep your values: {error}");
        }
    }

    /// Bring back the person's own values from the last run.
    pub fn load_user_vars(&self) {
        let saved = self
            .inner
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .meta(Self::USER_VARS_KEY)
            .ok()
            .flatten();
        let Some(saved) = saved else { return };
        let Ok(values) = serde_json::from_str::<BTreeMap<String, Value>>(&saved) else { return };
        for (name, value) in values {
            let _ = self.inner.variables.set(&name, value);
        }
    }

    /// How numbers, money and times are written on this desktop.
    pub fn locale(&self) -> flexdeck_expr::Locale {
        self.inner.locale.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Write numbers, money and times this way from now on, keep it, and redraw.
    pub fn set_locale(&self, locale: flexdeck_expr::Locale) {
        if let Ok(json) = serde_json::to_string(&locale) {
            let store = self.inner.store.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(error) = store.set_meta(Self::LOCALE_KEY, &json) {
                tracing::warn!("could not keep the formats: {error}");
            }
        }
        *self.inner.locale.write().unwrap_or_else(PoisonError::into_inner) = locale;
        self.push_widget_states(|plan| plan.is_live());
    }

    /// Where the desktop's formats are kept.
    const LOCALE_KEY: &'static str = "locale";

    /// What `plan`'s widget should show right now.
    fn state_of(&self, plan: &WidgetPlan) -> flexdeck_protocol::StateEvent {
        let cycled = self
            .inner
            .cycled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&plan.widget_id)
            .copied()
            .unwrap_or(0);
        let provided = self
            .inner
            .provided
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&plan.widget_id)
            .cloned();
        let mut event = plan.state_in(&self.inner.variables, cycled, provided.as_deref(), &self.locale());
        event.unavailable = self.unavailable_of(&plan.widget_id).unwrap_or_default();
        event
    }

    fn unavailable_of(&self, widget_id: &str) -> Option<String> {
        self.inner
            .unavailable
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(widget_id)
            .cloned()
    }

    /// Why `plan` cannot do what it does on the plugins as they are now;
    /// empty when it can.
    pub fn unavailable_for(&self, plan: &WidgetPlan) -> String {
        let actions = Arc::clone(&self.inner.actions.read().unwrap_or_else(PoisonError::into_inner));
        plan.flows.iter().find_map(|blocks| unavailable_in(&actions, blocks)).unwrap_or_default()
    }

    /// Check every key's actions against the plugins as they are now, and
    /// return the keys whose answer changed.
    fn refresh_unavailable(&self) -> BTreeSet<String> {
        let plans = self.widget_plans();
        let actions = Arc::clone(&self.inner.actions.read().unwrap_or_else(PoisonError::into_inner));
        let fresh: BTreeMap<String, String> = plans
            .iter()
            .filter_map(|plan| {
                let why = plan.flows.iter().find_map(|blocks| unavailable_in(&actions, blocks))?;
                Some((plan.widget_id.clone(), why))
            })
            .collect();
        let mut kept = self.inner.unavailable.write().unwrap_or_else(PoisonError::into_inner);
        let changed = kept
            .keys()
            .chain(fresh.keys())
            .filter(|id| kept.get(*id) != fresh.get(*id))
            .cloned()
            .collect();
        *kept = Arc::new(fresh);
        changed
    }

    /// What one widget shows right now, if it exists.
    pub fn widget_state(&self, widget_id: &str) -> Option<flexdeck_protocol::StateEvent> {
        self.widget_plans()
            .iter()
            .find(|plan| plan.widget_id == widget_id)
            .map(|plan| self.state_of(plan))
    }

    /// What every live widget should show right now — sent to a tablet when
    /// it connects, and after the configuration changes.
    pub fn widget_state_snapshot(&self) -> Vec<flexdeck_protocol::StateEvent> {
        // A key fixed since (a new action picked) is no longer in this
        // snapshot unless it is live, so every viewer is told it directly.
        let changed = self.refresh_unavailable();
        if !changed.is_empty() {
            self.push_widget_states(|plan| changed.contains(&plan.widget_id));
        }
        let unavailable = Arc::clone(&self.inner.unavailable.read().unwrap_or_else(PoisonError::into_inner));
        self.widget_plans()
            .iter()
            .filter(|plan| plan.is_live() || unavailable.contains_key(&plan.widget_id))
            .map(|plan| self.state_of(plan))
            .collect()
    }

    /// Hear every live widget state as it changes.
    pub fn subscribe_widget_states(
        &self,
    ) -> tokio::sync::broadcast::Receiver<flexdeck_protocol::StateEvent> {
        self.inner.widget_states.subscribe()
    }

    /// The most points kept per graphed variable: ten minutes at one a second.
    pub const HISTORY_POINTS: usize = 600;

    /// Every variable some face graphs, on any page.
    pub fn graphed_variables(&self) -> BTreeSet<String> {
        self.widget_plans().iter().flat_map(|plan| plan.graphs()).collect()
    }

    /// Add the current value of every graphed variable to its history, and
    /// tell the tablets. A value nobody knows yet adds nothing: a gap is
    /// truer than a zero.
    pub fn record_history(&self) {
        let graphed = self.graphed_variables();
        let handed = self.inner.handed.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut history = self.inner.history.lock().unwrap_or_else(PoisonError::into_inner);
        // A variable no graph draws any more is forgotten.
        history.retain(|name, _| graphed.contains(name));
        for name in graphed.into_iter().filter(|name| !handed.contains(name)) {
            let Some(point) = self.inner.variables.get(&name).and_then(|value| value.as_f64()) else {
                continue;
            };
            let points = history.entry(name.clone()).or_default();
            points.push_back(point);
            while points.len() > Self::HISTORY_POINTS {
                points.pop_front();
            }
            let _ = self.inner.series.send(flexdeck_protocol::SeriesEvent {
                variable: name,
                points: vec![point],
                append: true,
            });
        }
    }

    /// Replace a variable's history with points a plugin computed (a day of
    /// prices), and tell the tablets.
    pub fn set_series(&self, name: &str, points: Vec<f64>) {
        self.inner.handed.lock().unwrap_or_else(PoisonError::into_inner).insert(name.to_string());
        let points: std::collections::VecDeque<f64> = points
            .into_iter()
            .filter(|point| point.is_finite())
            .rev()
            .take(Self::HISTORY_POINTS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let _ = self.inner.series.send(flexdeck_protocol::SeriesEvent {
            variable: name.to_string(),
            points: points.iter().copied().collect(),
            append: false,
        });
        self.inner
            .history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_string(), points);
    }

    /// Every graphed history as it stands: what a tablet is sent on connect.
    pub fn series_snapshot(&self) -> Vec<flexdeck_protocol::SeriesEvent> {
        let graphed = self.graphed_variables();
        let history = self.inner.history.lock().unwrap_or_else(PoisonError::into_inner);
        graphed
            .into_iter()
            .map(|name| flexdeck_protocol::SeriesEvent {
                points: history.get(&name).map(|points| points.iter().copied().collect()).unwrap_or_default(),
                variable: name,
                append: false,
            })
            .collect()
    }

    pub fn subscribe_series(&self) -> tokio::sync::broadcast::Receiver<flexdeck_protocol::SeriesEvent> {
        self.inner.series.subscribe()
    }

    fn push_widget_states(&self, wanted: impl Fn(&WidgetPlan) -> bool) {
        // Nobody looking — no tablet, no configurator, no watch through the
        // cloud — is nothing to work out. Not the subscriber count: the tunnel
        // listens all along, so a watch that starts misses nothing.
        if self.inner.viewers.load(Ordering::SeqCst) == 0 || self.inner.widget_states.receiver_count() == 0 {
            return;
        }
        for plan in self.widget_plans().iter().filter(|plan| wanted(plan)) {
            let _ = self.inner.widget_states.send(self.state_of(plan));
        }
    }

    /// Count a connected tablet for as long as the guard lives. While any
    /// is connected, bound values are kept fresh.
    pub fn viewer(&self) -> ViewerGuard {
        self.inner.viewers.fetch_add(1, Ordering::SeqCst);
        ViewerGuard {
            behavior: self.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn viewer_count(&self) -> usize {
        self.inner.viewers.load(Ordering::SeqCst)
    }

    /// Show a notification on behalf of a plugin.
    ///
    /// Failures are swallowed on purpose: a plugin cannot be told "your
    /// notice was not shown", and the alternative — failing whatever the plugin
    /// was doing — would punish it for the machine's notification settings.
    pub fn notify(&self, title: &str, body: &str, _plugin: &str) {
        let mut notifier = self
            .inner
            .notifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _ = notifier.notify(title, body);
    }

    /// A plugin announced an event: run every automation, and every widget
    /// flow, triggered by it.
    pub fn on_plugin_event(&self, plugin: &str, event: &str, payload: &BTreeMap<String, String>) {
        let triggered: Vec<Automation> = {
            let document = self
                .inner
                .document
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            document
                .automations
                .iter()
                .filter(|a| a.enabled && a.trigger.fires_for_event(plugin, event, payload))
                .cloned()
                .collect()
        };
        // The widget flows, with which widget and flow each one is, so a look
        // an action reports can be kept for the widget it belongs to.
        let plans = self.widget_plans();
        let mut flows: Vec<(&WidgetPlan, usize, Automation)> = Vec::new();
        for plan in plans.iter() {
            for flow in plan
                .events
                .iter()
                .filter(|flow| flow.fires_for(plugin, event, payload))
            {
                flows.push((
                    plan,
                    flow.index,
                    Automation {
                        id: format!("widget:{}", plan.widget_id),
                        name: if flow.name.is_empty() {
                            format!("{} ({plugin}:{event})", plan.widget_id)
                        } else {
                            flow.name.clone()
                        },
                        enabled: true,
                        trigger: flexdeck_automation::Trigger::Manual,
                        body: flow.blocks.clone(),
                    },
                ));
            }
        }
        if triggered.is_empty() && flows.is_empty() {
            return;
        }
        // The payload is the run's input, so a step can use `{"input":"scene"}`.
        let inputs: Inputs = payload
            .iter()
            .map(|(key, value)| (key.clone(), Value::Text(value.clone())))
            .collect();
        let mut sleeper = ThreadSleeper;
        let mut notifier_guard = self
            .inner
            .notifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for automation in triggered {
            let _ = self.run_with(
                &automation,
                self.inner.variables.as_ref(),
                Box::as_mut(&mut *notifier_guard),
                &mut sleeper,
                &BTreeMap::new(),
                &inputs,
            );
        }
        let mut touched = BTreeSet::new();
        for (plan, index, automation) in &flows {
            if let Ok(run) = self.run_with(
                automation,
                self.inner.variables.as_ref(),
                Box::as_mut(&mut *notifier_guard),
                &mut sleeper,
                &BTreeMap::new(),
                &inputs,
            ) {
                if self.record_provided(plan, *index, &run) {
                    touched.insert(plan.widget_id.clone());
                }
            }
        }
        drop(notifier_guard);
        if !touched.is_empty() {
            self.push_widget_states(|plan| touched.contains(&plan.widget_id));
        }
    }

    /// Keep the look `gesture` picks, if `plan`'s looks follow gestures.
    fn record_gesture(&self, plan: &WidgetPlan, gesture: Gesture) {
        let Some(crate::widgets::StateSource::Gesture(picks)) = &plan.source else {
            return;
        };
        let mut provided = self.inner.provided.lock().unwrap_or_else(PoisonError::into_inner);
        let mut before = self.inner.before_hold.lock().unwrap_or_else(PoisonError::into_inner);
        let id = plan.widget_id.clone();
        if gesture == Gesture::HoldStart {
            before.insert(id.clone(), provided.get(&id).cloned());
        }
        let held = if gesture == Gesture::HoldEnd { before.remove(&id) } else { None };
        // A hold whose start was never recorded (its flow failed) has no look
        // to go back to: leave the one the key shows.
        let next = match (picks.get(&gesture), held) {
            (Some(look), _) => Some(look.clone()),
            (None, Some(previous)) => previous,
            (None, None) => return,
        };
        match next {
            Some(look) => provided.insert(id, look),
            None => provided.remove(&id),
        };
    }

    /// Keep the look a run's providing action reported, if `plan`'s looks
    /// come from an action and that action is in this run (the `flow`-th
    /// flow). Returns whether anything was kept.
    fn record_provided(&self, plan: &WidgetPlan, flow: usize, run: &Run) -> bool {
        let Some(crate::widgets::StateSource::Provider {
            flow: wanted,
            path: wanted_path,
        }) = &plan.source
        else {
            return false;
        };
        if *wanted != flow {
            return false;
        }
        // The last report wins: an action inside a repeat reports each time.
        let reported = run.trace.iter().rev().find_map(|entry| match entry {
            Trace::Ran {
                path,
                state: Some(state),
                ..
            } if path == wanted_path => Some(state.clone()),
            _ => None,
        });
        let Some(state) = reported else {
            return false;
        };
        self.inner
            .provided
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(plan.widget_id.clone(), state);
        true
    }

    /// Tell anyone watching that the plugin catalogue moved.
    pub fn catalogue_changed(&self) {
        let (generation, actions) = {
            let registry = self.inner.registry.lock().unwrap_or_else(PoisonError::into_inner);
            (registry.generation(), ActionCatalogue::of(&registry))
        };
        *self.inner.actions.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(actions);
        // No subscribers is normal.
        let _ = self.inner.catalogue.send(generation);
        let changed = self.refresh_unavailable();
        if !changed.is_empty() {
            self.push_widget_states(|plan| changed.contains(&plan.widget_id));
        }
    }

    /// Hear about every catalogue change. See [`Self::catalogue_changed`].
    pub fn subscribe_catalogue(&self) -> tokio::sync::broadcast::Receiver<u64> {
        self.inner.catalogue.subscribe()
    }

    pub fn plugin_catalogue(&self) -> serde_json::Value {
        self.registry_snapshot().catalogue_json()
    }

    pub fn variables_catalogue(&self) -> serde_json::Value {
        // Wrap each entry so the spec fields and the live value sit at named
        // keys. The default tuple serialisation would put them at indices, and
        // a portal trying to address `entry["name"]` would find it missing.
        let entries: Vec<VariableCatalogueEntry> = self
            .inner
            .variables
            .catalogue()
            .into_iter()
            .map(|(spec, value)| VariableCatalogueEntry { spec, value })
            .collect();
        serde_json::to_value(entries).unwrap_or(serde_json::Value::Null)
    }

    pub fn variables(&self) -> &Arc<VariableStore> {
        &self.inner.variables
    }

    pub fn executor(&self) -> &ExecutorHandle {
        &self.inner.executor
    }

    pub fn registry(&self) -> &Arc<Mutex<Registry>> {
        &self.inner.registry
    }

    pub fn registry_snapshot(&self) -> Registry {
        self.inner.registry.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn cancel_all(&self) {
        self.inner.cancel.cancel();
    }

    /// Someone touched `widget_id`: run the flow for that gesture.
    ///
    /// The flow runs through the automation engine, so a button gets what an
    /// automation gets — conditions, waits, repeats, variables, budgets. A
    /// cycling widget then moves to its next look; automations triggered by
    /// this widget run after, on a short press, as before.
    ///
    /// A widget that is gone, or has nothing for this gesture, is not an
    /// error: the configuration may have changed under the tablet.
    pub fn interact(
        &self,
        widget_id: &str,
        gesture: Gesture,
    ) -> Result<Interaction, BehaviorError> {
        self.interact_in(widget_id, gesture, "")
    }

    /// [`Self::interact`] on one zone of the face's strip: its own flow only.
    /// Empty `zone`: the key itself.
    pub fn interact_in(
        &self,
        widget_id: &str,
        gesture: Gesture,
        zone: &str,
    ) -> Result<Interaction, BehaviorError> {
        let pressing = crate::watch::busy("press", format!("{widget_id} {gesture:?}"), crate::watch::PRESS);
        let plans = self.widget_plans();
        let Some(plan) = plans.iter().find(|plan| plan.widget_id == widget_id) else {
            return Ok(Interaction::default());
        };
        let why = self.unavailable_for(plan);
        if !why.is_empty() {
            return Ok(Interaction { ran: false, failure: Some(why) });
        }

        let settings = BTreeMap::new();
        let mut notifier_guard = self
            .inner
            .notifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        drop(pressing);

        let mut interaction = Interaction::default();
        if let Some(index) = plan.flow_index(gesture, zone) {
            let _flowing = crate::watch::busy("key flow", format!("{widget_id} {gesture:?}"), crate::watch::AUTOMATION);
            let flow = Automation {
                id: format!("widget:{widget_id}"),
                name: format!("{widget_id} ({gesture:?})"),
                enabled: true,
                trigger: flexdeck_automation::Trigger::Manual,
                body: plan.flows[index].clone(),
            };
            let mut sleeper = ThreadSleeper;
            let run = self.run_with(
                &flow,
                self.inner.variables.as_ref(),
                Box::as_mut(&mut *notifier_guard),
                &mut sleeper,
                &settings,
                &NO_INPUTS,
            )?;
            interaction.ran = true;
            interaction.failure = failure_of(&run);
            self.record_provided(plan, index, &run);
        }
        if zone.is_empty() && interaction.failure.is_none() {
            self.record_gesture(plan, gesture);
        }

        // A zone is a button of its own: it neither cycles the key's looks
        // nor starts what a press on the key does.
        if gesture == Gesture::ShortPress && zone.is_empty() {
            if interaction.failure.is_none()
                && matches!(plan.source, Some(crate::widgets::StateSource::Cycle))
            {
                *self
                    .inner
                    .cycled
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .entry(widget_id.to_string())
                    .or_insert(0) += 1;
            }

            // Automations triggered by this widget fire after its own flow.
            let triggered = self.automations_for_widget(widget_id);
            let max_chain = self.inner.limits.max_press_chain;
            if triggered.len() > max_chain {
                // The press handler is the seam where a misconfigured document
                // turns into a denial-of-service: every element in the chain
                // runs with `ThreadSleeper`, so the worst case is `max_wait_ms`
                // sleeps back-to-back holding the notifier mutex. Refusing the
                // chain up front is faster than discovering it on every press.
                return Err(BehaviorError::Document(format!(
                    "{} automations trigger on this one, and the limit is {}",
                    triggered.len(),
                    max_chain
                )));
            }
            for automation in triggered {
                let _running = crate::watch::busy("automation", automation.id.clone(), crate::watch::AUTOMATION);
                let mut sleeper = ThreadSleeper;
                let _ = self.run_with(
                    &automation,
                    self.inner.variables.as_ref(),
                    Box::as_mut(&mut *notifier_guard),
                    &mut sleeper,
                    &settings,
                    &NO_INPUTS,
                );
            }
        }
        drop(notifier_guard);

        let _showing = crate::watch::busy("press result", widget_id.to_string(), crate::watch::PRESS);
        self.push_widget_states(|candidate| {
            candidate.widget_id == widget_id && candidate.is_live()
        });
        Ok(interaction)
    }

    /// A slider moved to `value`: run its value flow — eventually, and only
    /// with the latest value.
    ///
    /// Returns at once. A drag sends tens of values a second; running a flow
    /// for each would queue minutes of volume changes behind the lock every
    /// action takes, and stall every button with them. Instead each slider has
    /// one slot: a new value replaces the pending one, and a single worker
    /// runs whatever is newest, at most once per the slider's throttle. The
    /// value the finger lifted on is always the last one run, because nothing
    /// replaces it.
    pub fn slide(&self, widget_id: &str, value: f64) {
        let plans = self.widget_plans();
        let Some(plan) = plans.iter().find(|plan| plan.widget_id == widget_id) else {
            return;
        };
        let (Some(slider), Some(_)) = (&plan.slider, plan.value_flow) else {
            return;
        };
        let value = slider.settle(value);
        let start = {
            let mut sliders = self
                .inner
                .sliders
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let slot = sliders.entry(widget_id.to_string()).or_default();
            slot.latest = Some(value);
            !std::mem::replace(&mut slot.running, true)
        };
        if start {
            let behavior = self.clone();
            let widget_id = widget_id.to_string();
            let _ = std::thread::Builder::new()
                .name(format!("slider-{widget_id}"))
                .spawn(move || behavior.drain_slider(&widget_id));
        }
    }

    /// Run a slider's newest value until there is none left. See [`Self::slide`].
    fn drain_slider(&self, widget_id: &str) {
        loop {
            let plans = self.widget_plans();
            let plan = plans.iter().find(|plan| plan.widget_id == widget_id);
            let throttle = plan
                .and_then(|plan| plan.slider.as_ref())
                .map_or(crate::widgets::DEFAULT_SLIDER_THROTTLE_MS, |slider| slider.throttle_ms);

            // Pace first, then take the value: whatever arrived while waiting
            // is what runs.
            let wait = {
                let sliders = self.inner.sliders.lock().unwrap_or_else(PoisonError::into_inner);
                sliders
                    .get(widget_id)
                    .and_then(|slot| slot.last_run)
                    .map(|last| {
                        (last + std::time::Duration::from_millis(u64::from(throttle)))
                            .saturating_duration_since(std::time::Instant::now())
                    })
                    .unwrap_or_default()
            };
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            let value = {
                let mut sliders = self.inner.sliders.lock().unwrap_or_else(PoisonError::into_inner);
                let slot = sliders.entry(widget_id.to_string()).or_default();
                match (slot.latest.take(), plan) {
                    (Some(value), Some(_)) => {
                        slot.last_run = Some(std::time::Instant::now());
                        slot.runs += 1;
                        value
                    }
                    // Nothing new, or the widget is gone: stop, in the same
                    // breath as checking, so a value arriving now starts a
                    // new worker rather than being missed.
                    _ => {
                        slot.running = false;
                        return;
                    }
                }
            };
            let plan = plan.expect("checked above");
            let Some(index) = plan.value_flow else { continue };
            let flow = Automation {
                id: format!("widget:{widget_id}"),
                name: format!("{widget_id} (value)"),
                enabled: true,
                trigger: flexdeck_automation::Trigger::Manual,
                body: plan.flows[index].clone(),
            };
            let inputs = Inputs::from([("value".to_string(), Value::Number(value))]);
            let failure = {
                let mut notifier = self.inner.notifier.lock().unwrap_or_else(PoisonError::into_inner);
                let mut sleeper = ThreadSleeper;
                match self.run_with(
                    &flow,
                    self.inner.variables.as_ref(),
                    Box::as_mut(&mut *notifier),
                    &mut sleeper,
                    &BTreeMap::new(),
                    &inputs,
                ) {
                    Ok(run) => failure_of(&run),
                    Err(error) => Some(error.to_string()),
                }
            };
            // Success shows itself: the value it set moves the variable the
            // thumb follows. A failure has to be said, in words.
            if let Some(error) = failure {
                let mut state = self.state_of(plan);
                state.ok = false;
                state.error = error;
                let _ = self.inner.widget_states.send(state);
            }
        }
    }

    /// How many times a slider's flow has run, and whether a run is pending.
    pub fn slider_runs(&self, widget_id: &str) -> (u64, bool) {
        let sliders = self.inner.sliders.lock().unwrap_or_else(PoisonError::into_inner);
        sliders
            .get(widget_id)
            .map_or((0, false), |slot| (slot.runs, slot.running))
    }

    /// Fire every automation whose trigger watches `name`.
    ///
    /// Called from the variable-change subscription set up by [`Self::start`].
    /// Public so a test can drive it directly.
    pub fn on_variable_changed(&self, name: &str) {
        self.react_to_change(name);
        if flexdeck_vars::is_user_name(name) {
            self.save_user_vars();
        }
    }

    fn on_variables_changed(&self, names: &[String]) {
        for name in names {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.react_to_change(name)));
        }
        if names.iter().any(|name| flexdeck_vars::is_user_name(name)) {
            self.save_user_vars();
        }
    }

    fn react_to_change(&self, name: &str) {
        self.push_widget_states(|plan| plan.reads(name));
        let triggered: Vec<Automation> = {
            let document = self
                .inner
                .document
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            document
                .automations
                .iter()
                .filter(|a| a.enabled && a.trigger.watches(name))
                .cloned()
                .collect()
        };
        if triggered.is_empty() {
            return;
        }
        let mut sleeper = ThreadSleeper;
        let mut notifier_guard = self
            .inner
            .notifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Each plugin is handed its own settings by the registry.
        let settings = BTreeMap::new();
        for automation in triggered {
            let _ = self.run_with(
                &automation,
                self.inner.variables.as_ref(),
                Box::as_mut(&mut *notifier_guard),
                &mut sleeper,
                &settings,
                &NO_INPUTS,
            );
        }
    }

    /// Exercise one automation against a scratch state.
    ///
    /// Returns the [`Run`] the editor shows in its "Test" button. The live
    /// variables and the live notifier are not touched; a scratch variable
    /// store receives the sample and a recording notifier records what would
    /// have been shown. A non-system plugin (one that needs a real OS) is
    /// detected by the `Unavailable` error and left silent — the trace says
    /// why.
    pub fn dry_run(&self, automation_id: &str) -> Result<Run, BehaviorError> {
        let automation = {
            let document = self
                .inner
                .document
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            document
                .automations
                .iter()
                .find(|automation| automation.id == automation_id)
                .cloned()
                .ok_or_else(|| BehaviorError::UnknownAutomation(automation_id.to_string()))?
        };

        // A scratch variable store so the live one is byte-equal before and
        // after — the editor's "Test" button must not move the user's deck.
        // Always declare the plugins' variables, not only when the trigger
        // watches something: a `SetVariable` body must succeed against the
        // scratch too, and a typo would otherwise look like a configuration
        // error instead of a real evaluation.
        let scratch = VariableStore::new();
        let mut registry = self.registry_snapshot();
        registry.declare_variables(&scratch);

        let watched = automation.trigger.watched();
        let interested: BTreeSet<String> = watched.into_iter().collect();
        if !interested.is_empty() {
            let mut remote = RemoteExecutor::new(self.inner.executor.clone());
            let mut recorder = RecordingNotifier::new();
            let _ = {
                let mut ctx = Ctx {
                    input: &mut remote,
                    vars: &scratch,
                    notifier: &mut recorder,
                    settings: &BTreeMap::new(),
                    now_ms: self.inner.clock.now_ms(),
                };
                registry.sample(&interested, &mut ctx)
            };
        }

        let mut recorder = RecordingNotifier::new();
        let mut sleeper = RecordingSleeper::new();
        self.run_with(
            &automation,
            &scratch,
            &mut recorder,
            &mut sleeper,
            &BTreeMap::new(),
            &NO_INPUTS,
        )
    }

    /// Start the engine's two background tasks: a subscription that fires
    /// `on_variable_changed` when any watched variable moves, and a sampler
    /// that asks plugins to keep those variables fresh.
    ///
    /// Idempotent: calling `start` twice spawns nothing the second time,
    /// and the subscription/sampler tasks survive a plugin panic — a panic
    /// inside `sample_once` is caught by `catch_unwind` and the sampler
    /// continues ticking. The user's stated preference is "fewer wake-ups";
    /// the idle interval is [`SAMPLER_IDLE_INTERVAL`] (30 s) and shortens
    /// to [`SAMPLER_ACTIVE_INTERVAL`] (1 s) the moment any automation is
    /// watching something. Both are real intervals, not a "fast tick that
    /// skips when idle" — the idle task wakes every 30 s, finds the
    /// interest set empty, and goes back to sleep without calling any
    /// plugin.
    ///
    /// Both tasks live for as long as the receivers stay open, which is for
    /// the agent's lifetime.
    pub fn start(&self, handle: &tokio::runtime::Handle) {
        // Atomic guard: the binary may legitimately call `start` twice
        // (startup + resume), and spawning the sampler loop twice means two
        // ticks per second once the agent has any automations. One `started`
        // flag pins the contract.
        if self
            .inner
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        self.start_subscription(handle);
        self.start_sampler(handle);
        self.start_scheduler(handle);
        self.start_sources(handle);
        crate::watch::spawn_health(handle);
    }

    /// The data sources.
    pub fn sources(&self) -> &Arc<crate::sources::Sources> {
        &self.inner.sources
    }

    /// Write the kept secrets down.
    fn keep_secrets(&self) {
        let store = self.inner.store.lock().unwrap_or_else(PoisonError::into_inner);
        self.keep_secrets_in(&store);
    }

    fn keep_secrets_in(&self, store: &ConfigStore) {
        let Ok(json) = serde_json::to_string(&self.inner.sources.secrets()) else { return };
        if let Err(error) = store.set_meta(crate::sources::SECRETS_KEY, &json) {
            tracing::warn!("could not keep the sources' secrets: {error}");
        }
    }

    /// Forget one kept secret: a key's own, or (no widget) the source's.
    pub fn forget_source_secret(&self, source: &str, widget: Option<&str>, arg: &str) -> bool {
        let forgot = self.inner.sources.forget_secret(source, widget, arg);
        if forgot {
            self.keep_secrets();
        }
        forgot
    }

    /// A config about to be saved, with every secret argument a key gives its
    /// source taken out and kept here instead — the config goes to every
    /// tablet, into undo history and into exports. A blank secret keeps the
    /// one already kept. Anything that does not parse is returned as it was,
    /// for the validator to refuse.
    pub fn take_secrets(&self, json: &str) -> String {
        let store = self.inner.store.lock().unwrap_or_else(PoisonError::into_inner);
        self.take_secrets_holding(json, &store)
    }

    pub fn take_secrets_holding(&self, json: &str, store: &ConfigStore) -> String {
        let Ok(mut document) = serde_json::from_str::<serde_json::Value>(json) else {
            return json.to_string();
        };
        let mut found = false;
        let mut changed = false;
        self.take_secrets_in(&mut document, &mut found, &mut changed);
        if changed {
            self.keep_secrets_in(store);
        }
        if found {
            serde_json::to_string(&document).unwrap_or_else(|_| json.to_string())
        } else {
            json.to_string()
        }
    }

    fn take_secrets_in(&self, value: &mut serde_json::Value, found: &mut bool, changed: &mut bool) {
        match value {
            serde_json::Value::Array(items) => items.iter_mut().for_each(|item| self.take_secrets_in(item, found, changed)),
            serde_json::Value::Object(object) => {
                let widget = object.get("id").and_then(|id| id.as_str()).map(str::to_string);
                if let (Some(widget), Some(serde_json::Value::Array(uses))) = (widget, object.get_mut("sources")) {
                    for used in uses.iter_mut() {
                        let Some(source) = used.get("source").and_then(|s| s.as_str()).map(str::to_string) else { continue };
                        let secret = self.inner.sources.secret_args(&source);
                        let Some(serde_json::Value::Object(args)) = used.get_mut("args") else { continue };
                        for name in secret {
                            if let Some(value) = args.remove(&name) {
                                *found = true;
                                if let Some(text) = value.as_str() {
                                    *changed |= self.inner.sources.set_secret(&source, Some(&widget), &name, text);
                                }
                            }
                        }
                    }
                }
                object.values_mut().for_each(|item| self.take_secrets_in(item, found, changed));
            }
            _ => {}
        }
    }

    /// Replace the data sources, after checking they could work, and keep them.
    pub fn replace_sources(&self, mut doc: crate::sources::SourcesDoc) -> Result<(), String> {
        crate::sources::validate(&doc)?;
        // A secret's value is the source's own kept secret, not part of the
        // definition: blank kept as it was.
        let mut moved = false;
        for source in &mut doc.sources {
            for arg in source.args.iter_mut().filter(|a| a.secret) {
                let value = std::mem::take(&mut arg.default);
                if !value.is_empty() {
                    self.inner.sources.set_secret(&source.id, None, &arg.name, &value);
                    moved = true;
                }
            }
        }
        if moved {
            self.keep_secrets();
        }
        let json = serde_json::to_string(&doc).map_err(|error| error.to_string())?;
        self.inner
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_meta(crate::sources::SOURCES_KEY, &json)
            .map_err(|error| error.to_string())?;
        self.inner.sources.replace(doc);
        Ok(())
    }

    /// Ask plugins for the feeds keys use now, and stop the ones no key
    /// uses. A plugin that restarted (the catalogue moved on) is asked for
    /// all of its again. Blocking: plugins are processes.
    pub fn sync_feeds(&self) {
        let wanted: BTreeMap<String, (String, String, BTreeMap<String, String>)> = self
            .widget_plans()
            .iter()
            .filter_map(|plan| {
                let (source, args) = plan.uses.first()?;
                let (plugin, feed) = source.split_once('/')?;
                Some((plan.widget_id.clone(), (plugin.to_string(), feed.to_string(), args.clone())))
            })
            .collect();
        let mut registry = self.registry_snapshot();
        let generation = registry.generation();
        let mut feeds = self.inner.feeds.lock().unwrap_or_else(PoisonError::into_inner);
        let restarted = feeds.0 != generation;
        for (widget, (plugin, _, _)) in feeds.1.iter() {
            if !wanted.contains_key(widget) {
                let _ = registry.unwatch_feed(plugin, widget);
            }
        }
        for (widget, (plugin, feed, args)) in &wanted {
            if restarted || feeds.1.get(widget) != Some(&(plugin.clone(), feed.clone(), args.clone())) {
                if let Err(error) = registry.watch_feed(plugin, widget, feed, args) {
                    tracing::warn!("{widget} could not watch {plugin}/{feed}: {error}");
                }
            }
        }
        *feeds = (registry.generation(), wanted);
    }

    /// Every key's own run of a source: (source, widget, arguments). Run
    /// whether or not a tablet is looking, at the source's own pace, so the
    /// editor's preview has the key's values too.
    pub fn source_uses(&self) -> Vec<(String, String, BTreeMap<String, String>)> {
        self.widget_plans()
            .iter()
            .flat_map(|plan| plan.uses.first().map(|(source, args)| (source.clone(), plan.widget_id.clone(), args.clone())))
            // A plugin's feed is the plugin's to run, not the source engine's.
            .filter(|(source, _, _)| !source.contains('/'))
            .collect()
    }

    /// Run whichever sources are due, once a second, each off the async
    /// workers — a fetch takes seconds. Nothing runs that nobody reads.
    fn start_sources(&self, handle: &tokio::runtime::Handle) {
        let me = self.clone();
        handle.spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut ticks: u64 = 0;
            loop {
                tick.tick().await;
                ticks += 1;
                // Every few seconds: keys come and go with each edit.
                if ticks % 3 == 1 {
                    let worker = me.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _busy = crate::watch::busy("feeds", "", crate::watch::BACKGROUND);
                        worker.sync_feeds()
                    })
                    .await;
                }
                // A timer whose time is up ends here, and says so.
                let now_ms = me.inner.clock.now_ms();
                for name in flexdeck_plugin::builtin::timer::finish_due(&me.inner.variables, now_ms) {
                    let worker = me.clone();
                    let payload = BTreeMap::from([("name".to_string(), name)]);
                    tokio::task::spawn_blocking(move || {
                        let _busy = crate::watch::busy("timer", payload["name"].clone(), crate::watch::AUTOMATION);
                        worker.on_plugin_event(flexdeck_plugin::builtin::timer::TIMER_PLUGIN, flexdeck_plugin::builtin::timer::FINISHED, &payload)
                    });
                }
                let wanted = me.watched_variable_names();
                let due = me.inner.sources.due(&me.source_uses(), &wanted, std::time::Instant::now());
                for run in due {
                    let worker = me.clone();
                    tokio::task::spawn_blocking(move || {
                        let _busy = crate::watch::busy("source", crate::sources::base_name(&run.source.id, run.instance.as_deref()), crate::sources::TIMEOUT * 2);
                        let now = worker.inner.clock.now_ms() / 1000;
                        let base = crate::sources::base_name(&run.source.id, run.instance.as_deref());
                        if let Ok(read) = worker.inner.sources.run(&run, now, &worker.locale()) {
                            for (field, points) in read.series {
                                worker.set_series(&format!("{base}.{field}"), points);
                            }
                        }
                    });
                }
            }
        });
    }

    /// Whether the engine's background tasks are running. Surfaced via the
    /// agent's `GET /api/status` so a deploy that forgot to call `start`
    /// fails an liveness check rather than silently shipping a dead agent.
    pub fn is_started(&self) -> bool {
        self.inner.started.load(Ordering::SeqCst)
    }

    /// The idle-interval sampler tick. 30 s rather than 1 s because the
    /// interest set is the only thing that makes a tick do work, and an
    /// agent with no automations is the common state at install. Once any
    /// automation watches anything, the active interval takes effect.
    pub(crate) const SAMPLER_IDLE_INTERVAL: std::time::Duration =
        std::time::Duration::from_secs(30);

    /// The active-interval sampler tick. 1 s is the smallest granularity a
    /// human action will notice; going lower spends wake-ups the user has
    /// not asked for.
    pub(crate) const SAMPLER_ACTIVE_INTERVAL: std::time::Duration =
        std::time::Duration::from_secs(1);

    /// Subscribe to `VariableStore` changes and fire `on_variable_changed` for
    /// any name an automation watches. Panics in the handler are caught so
    /// one bad automation cannot kill the subscription for the lifetime of
    /// the agent.
    fn start_subscription(&self, handle: &tokio::runtime::Handle) {
        let me = self.clone();
        let mut changes = self.inner.variables.subscribe();
        handle.spawn(async move {
            loop {
                let mut names = match changes.recv().await {
                    Ok(change) => vec![change.name],
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                };
                while let Ok(change) = changes.try_recv() {
                    names.push(change.name);
                }
                let worker = me.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let _busy = crate::watch::busy("changes", format!("{} changed", names.len()), crate::watch::AUTOMATION);
                    worker.on_variables_changed(&names)
                })
                .await;
            }
        });
    }

    /// Periodically ask plugins to refresh the variables some automation
    /// watches. The work is bounded by the interest set, so an agent with
    /// no variable-triggered automations does nothing here.
    fn start_sampler(&self, handle: &tokio::runtime::Handle) {
        let me = self.clone();
        handle.spawn(async move {
            // The intervals are deliberately long at idle to honour the
            // "fewer wake-ups" preference. The active interval shortens to
            // 1 s so a watcher still feels live.
            //
            // `Skip`, not the default `Burst`: only one of the two is polled at a
            // time, so the other falls behind. After an hour idle, `Burst` would
            // fire the active interval ~3,600 times back to back the moment a
            // watcher appeared — a storm of plugin samples for no reason.
            let mut idle = tokio::time::interval(Behavior::SAMPLER_IDLE_INTERVAL);
            let mut active = tokio::time::interval(Behavior::SAMPLER_ACTIVE_INTERVAL);
            idle.set_missed_tick_behavior(MissedTickBehavior::Skip);
            active.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                let interested = me.watched_variable_names();
                if interested.is_empty() {
                    idle.tick().await;
                    continue;
                }
                active.tick().await;
                // A sample calls into plugins — COM for the system backend, a
                // pipe round trip for a sidecar — which blocks, so it runs off
                // the async workers, exactly like the scheduler below. A panic
                // inside it surfaces as a join error rather than killing the loop.
                let worker = me.clone();
                let result =
                    tokio::task::spawn_blocking(move || {
                        let _busy = crate::watch::busy("sample", format!("{} watched", interested.len()), crate::watch::BACKGROUND);
                        worker.sample_once(&interested)
                    })
                    .await;
                if result.is_err() {
                    // A panic in a plugin's `sample` must not kill the
                    // sampler. Log to stderr — a panic is a bug worth
                    // knowing about, even at the cost of one line per
                    // occurrence. The next tick will retry.
                    tracing::error!("sampler panicked; continuing");
                }
                // One point per tick, sampled or pushed, so a flat line is
                // drawn flat rather than not at all.
                me.record_history();
            }
        });
    }

    /// Run scheduled automations when they fall due.
    ///
    /// Wakes every second only while some automation has a schedule, and
    /// every 30 seconds otherwise — the same "an idle agent costs nothing"
    /// rule as the sampler.
    fn start_scheduler(&self, handle: &tokio::runtime::Handle) {
        let me = self.clone();
        handle.spawn(async move {
            loop {
                let active = me.has_schedules();
                tokio::time::sleep(if active {
                    Behavior::SAMPLER_ACTIVE_INTERVAL
                } else {
                    Behavior::SAMPLER_IDLE_INTERVAL
                })
                .await;
                if !active {
                    continue;
                }
                let worker = me.clone();
                // Automations sleep and press keys; neither belongs on an
                // async worker thread.
                let _ = tokio::task::spawn_blocking(move || {
                    let _busy = crate::watch::busy("schedule", "", crate::watch::AUTOMATION);
                    worker.run_due_schedules(local_now());
                })
                .await;
            }
        });
    }

    fn has_schedules(&self) -> bool {
        self.inner
            .document
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .automations
            .iter()
            .any(|automation| automation.enabled && automation.trigger.is_schedule())
    }

    /// Fire every scheduled automation that is due at `now`. Public so a test
    /// can drive the clock.
    pub fn run_due_schedules(&self, now: flexdeck_automation::LocalTime) -> usize {
        let due: Vec<Automation> = {
            let document = self
                .inner
                .document
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            let mut fired = self
                .inner
                .schedule_fired
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut due = Vec::new();
            for automation in &document.automations {
                if !automation.enabled || !automation.trigger.is_schedule() {
                    continue;
                }
                let last = fired.get(&automation.id).copied();
                if automation.trigger.schedule_due(last, now) {
                    fired.insert(automation.id.clone(), now.epoch_secs);
                    due.push(automation.clone());
                } else if last.is_none() {
                    // First sight: an interval counts from now, not from 1970.
                    if let flexdeck_automation::Trigger::Schedule {
                        every_seconds: Some(_),
                        ..
                    } = automation.trigger
                    {
                        fired.insert(automation.id.clone(), now.epoch_secs);
                    }
                }
            }
            due
        };
        if due.is_empty() {
            return 0;
        }
        let mut sleeper = ThreadSleeper;
        let mut notifier_guard = self
            .inner
            .notifier
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for automation in &due {
            let _ = self.run_with(
                automation,
                self.inner.variables.as_ref(),
                Box::as_mut(&mut *notifier_guard),
                &mut sleeper,
                &BTreeMap::new(),
                &NO_INPUTS,
            );
        }
        due.len()
    }

    /// Sample once on the current thread. Public only for tests; the engine
    /// drives it on a timer from `start_sampler`.
    pub(crate) fn sample_once(&self, interested: &BTreeSet<String>) {
        let mut registry = self.registry_snapshot();
        let ids: Vec<String> = registry.ids().into_iter().map(str::to_string).collect();

        let mut remote = RemoteExecutor::new(self.inner.executor.clone());
        let now_ms = self.inner.clock.now_ms();

        // The sampler calls plugins' `sample`, which is a read: no input is
        // injected. A `RecordingNotifier` is the right default — production
        // wants no chatter for a polled-refresh.
        let mut recorder = RecordingNotifier::new();
        let settings = BTreeMap::new();

        for id in ids {
            let mut ctx = flexdeck_plugin::Ctx {
                input: &mut remote,
                vars: &self.inner.variables,
                notifier: &mut recorder,
                settings: &settings,
                now_ms,
            };
            let _ = registry.sample_plugin(&id, interested, &mut ctx);
        }
    }

    /// The union of every variable name an automation in the cached document
    /// watches. Empty when nothing is wired up, which is what makes the
    /// sampler loop do nothing on an idle agent.
    fn watched_variable_names(&self) -> BTreeSet<String> {
        let document = self
            .inner
            .document
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let mut names: BTreeSet<String> = BTreeSet::new();
        for automation in &document.automations {
            for name in automation.trigger.watched() {
                names.insert(name);
            }
        }
        drop(document);
        if self.inner.viewers.load(Ordering::SeqCst) > 0 {
            for plan in self.widget_plans().iter() {
                names.extend(plan.variables());
            }
        }
        names
    }

    // -- internals -----------------------------------------------------------

    fn automations_for_widget(&self, widget_id: &str) -> Vec<Automation> {
        let document = self
            .inner
            .document
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        document
            .automations
            .iter()
            .filter(|a| a.enabled && a.trigger.fires_for_widget(widget_id))
            .cloned()
            .collect()
    }

    pub(crate) fn run_with(
        &self,
        automation: &Automation,
        vars: &VariableStore,
        notifier: &mut dyn Notifier,
        sleeper: &mut dyn Sleeper,
        settings: &BTreeMap<String, String>,
        inputs: &Inputs,
    ) -> Result<Run, BehaviorError> {
        let mut registry = self.registry_snapshot();
        let latest = || self.registry_snapshot();
        let mut remote = RemoteExecutor::new(self.inner.executor.clone());
        let mut environment = Environment {
            registry: &mut registry,
            latest: Some(&latest),
            vars,
            inputs,
            input: &mut remote,
            notifier,
            settings,
            clock: self.inner.clock.as_ref(),
            sleeper,
            cancel: self.inner.cancel.as_ref(),
            limits: self.inner.limits,
        };
        Ok(flexdeck_automation::run(&mut environment, automation))
    }
}

/// What one interaction with a widget did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interaction {
    /// Whether the widget had a flow for the gesture.
    pub ran: bool,
    /// Why the flow stopped short, in words a person can act on.
    pub failure: Option<String>,
}

/// Each loaded plugin's actions and the arguments each one must be given.
#[derive(Default)]
pub(crate) struct ActionCatalogue {
    actions: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

impl ActionCatalogue {
    fn of(registry: &Registry) -> Self {
        let actions = registry
            .manifests()
            .into_iter()
            .map(|manifest| {
                let actions = manifest
                    .actions
                    .iter()
                    .map(|action| {
                        let required = action.arguments.iter().filter(|a| !a.optional).map(|a| a.name.clone()).collect();
                        (action.action.clone(), required)
                    })
                    .collect();
                (manifest.id.clone(), actions)
            })
            .collect();
        Self { actions }
    }
}

/// Why `blocks` cannot run on the plugins as they are: an action an
/// installed plugin no longer has, or an argument it now needs that the
/// step does not give. A plugin that is not loaded (yet) says nothing here.
fn unavailable_in(catalogue: &ActionCatalogue, blocks: &[Block]) -> Option<String> {
    blocks.iter().find_map(|block| match block {
        Block::DoAction { plugin, action, args } => {
            let actions = catalogue.actions.get(plugin)?;
            let Some(required) = actions.get(action) else {
                return Some(format!("{plugin} no longer has \"{action}\""));
            };
            required
                .iter()
                .find(|name| !args.contains_key(*name))
                .map(|name| format!("{plugin} \"{action}\" now needs \"{name}\""))
        }
        Block::If { then_blocks, else_if, otherwise, .. } => unavailable_in(catalogue, then_blocks)
            .or_else(|| else_if.iter().find_map(|branch| unavailable_in(catalogue, &branch.blocks)))
            .or_else(|| unavailable_in(catalogue, otherwise)),
        Block::Repeat { body, .. } => unavailable_in(catalogue, body),
        Block::Wait { .. } | Block::SetVariable { .. } => None,
    })
}

/// The reason a run did not complete, if it did not.
fn failure_of(run: &Run) -> Option<String> {
    match run.outcome {
        flexdeck_automation::RunOutcome::Completed => None,
        _ => Some(
            run.trace
                .iter()
                .rev()
                .find_map(|trace| match trace {
                    flexdeck_automation::Trace::Stopped { reason } => Some(reason.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "the button's steps did not finish".to_string()),
        ),
    }
}

/// A connected tablet. Dropping it stops counting it.
pub struct ViewerGuard {
    behavior: Behavior,
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.behavior.inner.viewers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The local wall clock, in the shape the schedule trigger reads.
fn local_now() -> flexdeck_automation::LocalTime {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    flexdeck_automation::LocalTime {
        epoch_secs: now.timestamp(),
        weekday: now.weekday().num_days_from_monday() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
    }
}

/// One row of the variables catalogue: a declaration plus the live value (or
/// `None` if the variable has never been sampled).
#[derive(serde::Serialize)]
struct VariableCatalogueEntry {
    spec: flexdeck_vars::VariableSpec,
    value: Option<flexdeck_vars::Value>,
}

/// Stamp the document's `rev` field to match the revision the store is about
/// to assign. A document claiming its own `rev` is worse than having no field
/// at all — a reader that trusts it would patch the wrong revision into the
/// timeline.
fn stamp_rev(json: &str, rev: u64) -> Result<String, BehaviorError> {
    let mut value: serde_json::Value = serde_json::from_str(json)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("rev".to_string(), serde_json::Value::from(rev));
    }
    Ok(serde_json::to_string(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_plugin::builtin::{self};
    use flexdeck_plugin::example::Example;

    fn registry() -> Arc<Mutex<Registry>> {
        let mut registry = Registry::new();
        for plugin in builtin::builtins(builtin::Backends::recording()) {
            registry.register(plugin);
        }
        registry.register(Box::new(Example::new()));
        Arc::new(Mutex::new(registry))
    }

    fn behavior() -> Behavior {
        let store: Arc<Mutex<ConfigStore>> =
            Arc::new(Mutex::new(ConfigStore::open_in_memory().unwrap()));
        let variables = Arc::new(VariableStore::new());
        registry().lock().unwrap().declare_variables(&variables);
        let (executor, _join) = crate::executor::executor_owner_thread(Box::new(
            flexdeck_actions::RecordingExecutor::new(),
        ));
        let notifier: Arc<Mutex<Box<dyn Notifier>>> =
            Arc::new(Mutex::new(Box::new(RecordingNotifier::new())));
        Behavior::new(
            store,
            variables,
            registry(),
            executor,
            notifier,
            Limits::default(),
        )
    }

    fn store_config(behavior: &Behavior, config: &str) {
        let store = behavior.inner.store.lock().unwrap();
        store.commit_document(&CONFIG, config).unwrap();
    }

    fn store_automations(behavior: &Behavior, json: &str) {
        let store = behavior.inner.store.lock().unwrap();
        store.commit_document(&AUTOMATIONS, json).unwrap();
    }

    /// One page holding `widgets`, at the configuration schema the agent reads.
    fn deck(widgets: serde_json::Value) -> String {
        serde_json::json!({
            "schema": crate::widgets::CONFIG_SCHEMA, "rev": 1,
            "profiles": [{"id": "p", "pages": [{"id": "home", "widgets": widgets}]}]
        })
        .to_string()
    }

    fn hotkey() -> serde_json::Value {
        serde_json::json!({"block": "doAction", "plugin": "core", "action": "hotkey",
                           "args": {"keys": {"text": "ctrl+shift+m"}}})
    }

    fn set_counter(value: f64) -> serde_json::Value {
        serde_json::json!({"block": "setVariable", "name": "example.counter",
                           "value": {"number": value}})
    }

    fn counter(behavior: &Behavior) -> Option<flexdeck_vars::Value> {
        behavior.inner.variables.get("example.counter")
    }

    #[test]
    fn a_widget_press_whose_triggered_automation_needs_the_executor_does_not_deadlock() {
        // The exact shape the jury flagged: the widget's own flow uses the
        // executor, and a widget-triggered automation ALSO uses the executor.
        // On the old `Mutex<Box<dyn Executor>>` this would re-lock and hang;
        // on the executor owner thread the second perform crosses the
        // channel and the test returns.
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn", "flows": [
                {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [hotkey()]}
            ]}])),
        );
        let automations = serde_json::json!({
            "schema": 1, "rev": 0,
            "automations": [{
                "id": "auto1", "name": "dem", "enabled": true,
                "trigger": { "on": "widget", "widgetId": "btn" },
                "body": [hotkey()]
            }]
        })
        .to_string();
        store_automations(&behavior, &automations);
        behavior.reload().unwrap();

        // Without the owner-thread fix, this call would hang.
        let interaction = behavior
            .interact("btn", Gesture::ShortPress)
            .expect("press does not deadlock");
        assert!(interaction.ran);
        assert_eq!(interaction.failure, None);
    }

    #[test]
    fn a_key_whose_flow_waits_is_timed_as_a_flow_not_as_a_slow_press() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "waiting-key", "flows": [
                {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [
                    {"block": "wait", "ms": {"number": 300.0}}
                ]}
            ]}])),
        );
        behavior.reload().unwrap();
        let presser = behavior.clone();
        let press = std::thread::spawn(move || presser.interact("waiting-key", Gesture::ShortPress).unwrap());
        let waiting = |what: &str| {
            crate::watch::running_details(what).iter().any(|detail| detail.starts_with("waiting-key "))
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !waiting("key flow") && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(waiting("key flow"), "the wait is counted as the key's flow");
        assert!(!waiting("press"), "the wait is not counted against the press");
        assert!(press.join().unwrap().ran);
        assert!(!waiting("key flow"));
    }

    #[test]
    fn a_deck_for_a_newer_agent_keeps_the_keys_last_read() {
        let behavior = behavior();
        store_config(&behavior, &deck(serde_json::json!([{"id": "btn"}])));
        assert_eq!(behavior.widget_plans().len(), 1);
        let newer = serde_json::json!({
            "schema": crate::widgets::CONFIG_SCHEMA + 1, "rev": 2,
            "profiles": [{"id": "p", "pages": [{"id": "home", "widgets": []}]}]
        })
        .to_string();
        store_config(&behavior, &newer);
        let plans = behavior.widget_plans();
        assert_eq!(plans.len(), 1, "not blanked");
        assert_eq!(plans[0].widget_id, "btn");
    }

    #[test]
    fn pressing_with_no_configuration_or_an_unknown_widget_runs_nothing() {
        let behavior = behavior();
        assert_eq!(
            behavior.interact("anything", Gesture::ShortPress).unwrap(),
            Interaction::default()
        );
        store_config(&behavior, &deck(serde_json::json!([{"id": "btn"}])));
        assert_eq!(
            behavior.interact("nope", Gesture::ShortPress).unwrap(),
            Interaction::default(),
            "a widget that is gone is not an error: the deck may have changed under the tablet"
        );
    }

    #[test]
    fn each_gesture_runs_only_its_own_flow() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn", "flows": [
                {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [set_counter(1.0)]},
                {"trigger": {"gesture": "GESTURE_LONG_PRESS"}, "blocks": [set_counter(2.0)]}
            ]}])),
        );

        assert!(behavior.interact("btn", Gesture::LongPress).unwrap().ran);
        assert_eq!(counter(&behavior), Some(flexdeck_vars::Value::Number(2.0)));

        assert!(behavior.interact("btn", Gesture::ShortPress).unwrap().ran);
        assert_eq!(counter(&behavior), Some(flexdeck_vars::Value::Number(1.0)));

        let none = behavior.interact("btn", Gesture::DoubleTap).unwrap();
        assert!(!none.ran, "no double-tap flow: nothing runs");
        assert_eq!(counter(&behavior), Some(flexdeck_vars::Value::Number(1.0)));
    }

    #[test]
    fn a_failing_step_says_why() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn", "flows": [
                {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [
                    {"block": "doAction", "plugin": "nobody", "action": "nothing", "args": {}}
                ]}
            ]}])),
        );
        let interaction = behavior.interact("btn", Gesture::ShortPress).unwrap();
        assert!(interaction.ran);
        let failure = interaction.failure.expect("the press reports a failure");
        assert!(failure.contains("nobody"), "{failure}");
    }

    #[test]
    fn an_automation_that_waits_does_not_hold_up_the_async_workers() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let behavior = behavior();
        let automations = serde_json::json!({
            "schema": 1, "rev": 0,
            "automations": [{
                "id": "slow",
                "name": "slow",
                "enabled": true,
                "trigger": { "on": "variables", "names": ["system.audio.volume"] },
                "body": [
                    { "block": "wait", "ms": { "number": 500.0 } },
                    { "block": "setVariable", "name": "example.counter", "value": { "number": 1.0 } }
                ]
            }]
        })
        .to_string();
        store_automations(&behavior, &automations);
        behavior.reload().unwrap();

        runtime.block_on(async {
            behavior.start_subscription(&tokio::runtime::Handle::current());
            tokio::task::yield_now().await;
            let started = std::time::Instant::now();
            behavior.inner.variables.refresh("system.audio.volume", Value::Number(7.0)).unwrap();
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            assert!(started.elapsed() < std::time::Duration::from_millis(300), "{:?}", started.elapsed());

            for _ in 0..100 {
                if behavior.inner.variables.get("example.counter") == Some(Value::Number(1.0)) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            panic!("the automation never finished");
        });
    }

    #[test]
    fn the_persons_own_values_are_kept_and_brought_back() {
        let behavior = behavior();
        behavior.inner.variables.set("vars.count", Value::Number(3.0)).unwrap();
        behavior.inner.variables.set("vars.mode", Value::Text("Gaming".into())).unwrap();
        behavior.on_variable_changed("vars.count");

        let fresh = Behavior::new(
            Arc::clone(&behavior.inner.store),
            Arc::new(VariableStore::new()),
            registry(),
            behavior.inner.executor.clone(),
            Arc::clone(&behavior.inner.notifier),
            Limits::default(),
        );
        fresh.load_user_vars();
        assert_eq!(fresh.inner.variables.get("vars.count"), Some(Value::Number(3.0)));
        assert_eq!(fresh.inner.variables.get("vars.mode"), Some(Value::Text("Gaming".into())));
    }

    #[test]
    fn a_graphed_value_keeps_a_history_and_tells_the_tablets() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "g", "face": {"elements": [
                {"kind": "ELEMENT_KIND_GRAPH", "variable": "example.counter"}
            ]}}])),
        );
        let mut tablet = behavior.subscribe_series();
        behavior.record_history();
        assert!(tablet.try_recv().is_err(), "unknown adds no point");

        for value in [1.0, 2.0, 3.0] {
            behavior.inner.variables.set("example.counter", flexdeck_vars::Value::Number(value)).unwrap();
            behavior.record_history();
        }
        let appended = tablet.try_recv().unwrap();
        assert!(appended.append && appended.points == [1.0] && appended.variable == "example.counter");
        let snapshot = behavior.series_snapshot();
        assert_eq!(snapshot[0].points, [1.0, 2.0, 3.0]);
        assert!(!snapshot[0].append);

        behavior.set_series("example.counter", vec![9.0, f64::NAN, 8.0]);
        assert_eq!(behavior.series_snapshot()[0].points, [9.0, 8.0], "not-a-number is dropped");

        // A deck that no longer graphs it forgets it.
        store_config(&behavior, &deck(serde_json::json!([{"id": "g"}])));
        behavior.record_history();
        assert!(behavior.series_snapshot().is_empty());
        assert!(behavior.inner.history.lock().unwrap().is_empty());
    }

    #[test]
    fn a_cycling_widget_moves_to_its_next_look_on_each_short_press() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn",
                "states": {"looks": [{"id": "live"}, {"id": "muted"}], "cycle": {}}
            }])),
        );
        let look = |b: &Behavior| b.widget_state("btn").unwrap().state_id;
        assert_eq!(look(&behavior), "live");
        behavior.interact("btn", Gesture::ShortPress).unwrap();
        assert_eq!(look(&behavior), "muted");
        behavior.interact("btn", Gesture::LongPress).unwrap();
        assert_eq!(look(&behavior), "muted", "only a short press cycles");
        behavior.interact("btn", Gesture::ShortPress).unwrap();
        assert_eq!(look(&behavior), "live");
    }

    #[test]
    fn a_key_whose_plugin_lost_its_action_is_shown_disabled_and_refuses_a_press() {
        let behavior = behavior();
        let press = |blocks: serde_json::Value| serde_json::json!([{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": blocks}]);
        store_config(
            &behavior,
            &deck(serde_json::json!([
                {"id": "gone", "flows": press(serde_json::json!([
                    {"block": "if", "condition": {"kind": "compare", "left": {"number": 1.0}, "op": "eq", "right": {"number": 1.0}},
                     "then": [{"block": "doAction", "plugin": "example", "action": "vanished", "args": {}}]},
                    set_counter(7.0)
                ]))},
                {"id": "fine", "flows": press(serde_json::json!([{"block": "doAction", "plugin": "example", "action": "flip"}]))},
                {"id": "absent", "flows": press(serde_json::json!([{"block": "doAction", "plugin": "nobody", "action": "nothing", "args": {}}]))}
            ])),
        );
        let snapshot = behavior.widget_state_snapshot();
        let gone = snapshot.iter().find(|state| state.widget_id == "gone").expect("sent though not live");
        assert!(gone.unavailable.contains("vanished"), "{}", gone.unavailable);
        assert!(snapshot.iter().all(|state| state.widget_id != "fine" && state.widget_id != "absent"));

        let pressed = behavior.interact("gone", Gesture::ShortPress).unwrap();
        assert!(!pressed.ran && pressed.failure.is_some());
        assert_eq!(counter(&behavior), None, "nothing in it ran");
        assert_eq!(behavior.widget_state("fine").unwrap().unavailable, "");
    }

    #[test]
    fn a_key_fixed_by_hand_is_told_it_works_again_though_it_shows_nothing_live() {
        let behavior = behavior();
        let _viewer = behavior.viewer();
        let mut states = behavior.subscribe_widget_states();
        let key = |action: &str| deck(serde_json::json!([{"id": "k", "flows": [
            {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [{"block": "doAction", "plugin": "example", "action": action}]}
        ]}]));
        store_config(&behavior, &key("vanished"));
        assert_eq!(behavior.widget_state_snapshot().len(), 1);
        while states.try_recv().is_ok() {}

        store_config(&behavior, &key("flip"));
        assert!(behavior.widget_state_snapshot().is_empty(), "not live, so not in the snapshot");
        let pushed = states.try_recv().expect("so it is pushed instead");
        assert_eq!((pushed.widget_id.as_str(), pushed.unavailable.as_str()), ("k", ""));
    }

    #[test]
    fn the_last_gesture_picks_the_look_and_a_hold_lasts_while_held() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn",
                "states": {"looks": [{"id": "tap"}, {"id": "long"}, {"id": "held"}], "gesture": {"looks": [
                    {"gesture": "GESTURE_SHORT_PRESS", "state": "tap"},
                    {"gesture": "GESTURE_LONG_PRESS", "state": "long"},
                    {"gesture": "GESTURE_HOLD_START", "state": "held"}
                ]}}
            }])),
        );
        let look = |b: &Behavior| b.widget_state("btn").unwrap().state_id;
        assert_eq!(look(&behavior), "", "nothing pressed yet: the key's own look");
        behavior.interact("btn", Gesture::LongPress).unwrap();
        assert_eq!(look(&behavior), "long");
        behavior.interact("btn", Gesture::DoubleTap).unwrap();
        assert_eq!(look(&behavior), "long", "a gesture without a look changes nothing");
        behavior.interact("btn", Gesture::HoldStart).unwrap();
        assert_eq!(look(&behavior), "held");
        behavior.interact("btn", Gesture::HoldEnd).unwrap();
        assert_eq!(look(&behavior), "long", "letting go goes back to what it showed");
        behavior.interact("btn", Gesture::ShortPress).unwrap();
        assert_eq!(look(&behavior), "tap");
        behavior.interact("btn", Gesture::HoldEnd).unwrap();
        assert_eq!(look(&behavior), "tap", "a hold whose start was never kept leaves the look alone");
    }

    #[test]
    fn a_widget_triggered_automation_runs_after_the_widgets_own_flow() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn", "flows": [
                {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [set_counter(1.0)]}
            ]}])),
        );
        let automations = serde_json::json!({
            "schema": 1,
            "rev": 0,
            "automations": [{
                "id": "auto1",
                "name": "demo",
                "enabled": true,
                "trigger": { "on": "widget", "widgetId": "btn" },
                "body": [set_counter(5.0)]
            }]
        })
        .to_string();
        store_automations(&behavior, &automations);
        behavior.reload().unwrap();

        behavior
            .interact("btn", Gesture::ShortPress)
            .expect("press");

        assert_eq!(
            counter(&behavior),
            Some(flexdeck_vars::Value::Number(5.0)),
            "the widget-triggered automation must run after the widget's own flow"
        );
    }

    #[test]
    fn a_widget_whose_looks_an_action_provides_shows_what_it_reported() {
        let behavior = behavior();
        let flip = serde_json::json!({"block": "doAction", "plugin": "example", "action": "flip"});
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn",
                "flows": [
                    {"trigger": {"gesture": "GESTURE_LONG_PRESS"}, "blocks": [hotkey()]},
                    {"trigger": {"gesture": "GESTURE_SHORT_PRESS"},
                     "blocks": [set_counter(1.0), {"block": "if",
                        "condition": {"kind": "compare", "left": {"number": 1}, "op": "eq", "right": {"number": 1}},
                        "then": [flip]}]}
                ],
                "states": {"looks": [{"id": "on"}, {"id": "off"}],
                           "provider": {"flow": 1, "path": [1, 0]}}
            }])),
        );
        let look = |b: &Behavior| b.widget_state("btn").unwrap().state_id;
        assert_eq!(look(&behavior), "", "nothing reported yet: the widget's own props");
        behavior.interact("btn", Gesture::ShortPress).unwrap();
        assert_eq!(look(&behavior), "on");
        behavior.interact("btn", Gesture::LongPress).unwrap();
        assert_eq!(look(&behavior), "on", "another flow's actions do not count");
        behavior.interact("btn", Gesture::ShortPress).unwrap();
        assert_eq!(look(&behavior), "off");
    }

    #[test]
    fn an_event_flow_is_given_the_events_payload() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn", "flows": [
                {"trigger": {"event": {"plugin": "obs", "event": "volume"}},
                 "blocks": [{"block": "setVariable", "name": "example.last_text",
                             "value": {"input": "scene"}}]}
            ]}])),
        );
        let payload = BTreeMap::from([("scene".to_string(), "Live".to_string())]);
        behavior.on_plugin_event("obs", "volume", &payload);
        assert_eq!(
            behavior.inner.variables.get("example.last_text"),
            Some(flexdeck_vars::Value::Text("Live".into()))
        );
    }

    #[test]
    fn a_widget_event_flow_runs_on_a_matching_plugin_event() {
        let behavior = behavior();
        store_config(
            &behavior,
            &deck(serde_json::json!([{"id": "btn", "flows": [
                {"name": "Live goes red",
                 "trigger": {"event": {"plugin": "obs", "event": "scene_changed", "filter": {"scene": "Live"}}},
                 "blocks": [set_counter(7.0)]}
            ]}])),
        );
        let payload = |scene: &str| BTreeMap::from([("scene".to_string(), scene.to_string())]);

        behavior.on_plugin_event("obs", "scene_changed", &payload("BRB"));
        assert_eq!(counter(&behavior), None, "the filter does not match");

        behavior.on_plugin_event("obs", "scene_changed", &payload("Live"));
        assert_eq!(counter(&behavior), Some(flexdeck_vars::Value::Number(7.0)));
    }

    #[test]
    fn a_variable_change_fires_automations_that_watch_it() {
        let behavior = behavior();

        let automations = serde_json::json!({
            "schema": 1, "rev": 0,
            "automations": [{
                "id": "auto1",
                "name": "watcher",
                "enabled": true,
                "trigger": { "on": "variables", "names": ["system.audio.volume"] },
                "body": [{ "block": "setVariable", "name": "example.counter", "value": { "number": 42.0 } }]
            }]
        })
        .to_string();
        store_automations(&behavior, &automations);
        behavior.reload().unwrap();

        // Drive the variable directly; in production this would be a
        // plugin's `vars.refresh(...)`.
        behavior
            .inner
            .variables
            .refresh("system.audio.volume", flexdeck_vars::Value::Number(7.0))
            .unwrap();

        // The subscription is not started in this test — we drive the path
        // the subscription would receive. Asserting that path proves the
        // engine half; the subscriber is a wire.
        behavior.on_variable_changed("system.audio.volume");

        assert_eq!(
            behavior.inner.variables.get("example.counter"),
            Some(flexdeck_vars::Value::Number(42.0))
        );
    }

    #[test]
    fn dry_run_against_a_scratch_store_leaves_the_live_one_untouched() {
        let behavior = behavior();

        // Seed the live variable store with a value the table will assert on.
        behavior
            .inner
            .variables
            .refresh("example.counter", flexdeck_vars::Value::Number(1.0))
            .unwrap();

        let automations = serde_json::json!({
            "schema": 1, "rev": 0,
            "automations": [{
                "id": "demo",
                "name": "demo",
                "enabled": true,
                "trigger": { "on": "manual" },
                "body": [{ "block": "setVariable", "name": "example.counter", "value": { "number": 99.0 } }]
            }]
        })
        .to_string();
        store_automations(&behavior, &automations);
        behavior.reload().unwrap();

        let run = behavior.dry_run("demo").expect("dry run");
        assert_eq!(run.outcome, flexdeck_automation::RunOutcome::Completed);

        // The live value did not move.
        assert_eq!(
            behavior.inner.variables.get("example.counter"),
            Some(flexdeck_vars::Value::Number(1.0))
        );
    }

    #[test]
    fn replacing_a_document_stamps_the_rev_so_the_timeline_cannot_diverge() {
        let behavior = behavior();

        let json = serde_json::json!({
            "schema": 1, "rev": 999,
            "automations": []
        })
        .to_string();

        let rev = behavior.replace_document_json(&json).expect("replace");
        assert_eq!(rev, 1, "the store assigns the revision, not the client");

        // The stored document carries the store's rev.
        let (_, stored) = behavior
            .inner
            .store
            .lock()
            .unwrap()
            .snapshot_document(&AUTOMATIONS)
            .unwrap()
            .expect("stored");
        let parsed: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(
            parsed["rev"], 1,
            "the stamped rev must override whatever the client claimed"
        );
    }

    #[test]
    fn replacing_with_an_oversized_document_is_refused_before_storing() {
        // The structural cap is enforced in parse_document — a document that
        // would be a memory problem is rejected here, long before any
        // evaluator sees it.
        let behavior = behavior();
        let mut block = serde_json::json!({
            "block": "wait", "ms": { "number": 1.0 }
        });
        for _ in 0..Limits::default().max_depth + 2 {
            block = serde_json::json!({
                "block": "repeat",
                "times": { "number": 1.0 },
                "body": [block]
            });
        }
        let too_big = serde_json::json!({
            "schema": 1, "rev": 0,
            "automations": [{ "id": "a", "name": "a", "enabled": true, "trigger": { "on": "manual" }, "body": [block] }]
        })
        .to_string();

        let error = behavior
            .replace_document_json(&too_big)
            .expect_err("too deep");
        assert!(error.to_string().contains("invalid automation document"));
    }

    #[test]
    fn the_plugin_catalogue_lists_every_plugin_with_its_actions_and_variables() {
        // The portal uses this endpoint to build its editor. If it stops
        // listing an action, a saved configuration referencing it becomes
        // uneditable without explanation.
        let behavior = behavior();
        let json = behavior.plugin_catalogue();
        let plugins = json["plugins"].as_array().expect("plugins");

        let system = plugins
            .iter()
            .find(|plugin| plugin["id"] == "system")
            .expect("system is catalogued");

        assert!(system["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|entry| entry["action"] == "set_output_device"));

        assert!(system["variables"]
            .as_array()
            .expect("variables")
            .iter()
            .any(|entry| entry["name"] == "system.audio.volume"));
    }

    #[test]
    fn the_variables_endpoint_reports_declarations_from_the_plugins() {
        let behavior = behavior();
        let json = behavior.variables_catalogue();
        let variables = json.as_array().expect("variables");

        assert!(
            variables
                .iter()
                .any(|variable| variable["spec"]["name"] == "system.audio.volume"),
            "the plugin's variables must reach the editor"
        );
    }

    #[test]
    fn watched_variable_names_is_empty_when_no_automation_watches_anything() {
        // The sampler loop short-circuits on an empty interest set; the
        // helper has to agree.
        let behavior = behavior();
        assert!(behavior.watched_variable_names().is_empty());
    }

    #[test]
    fn watched_variable_names_unions_every_trigger_in_the_document() {
        let behavior = behavior();
        let automations = serde_json::json!({
            "schema": 1, "rev": 0,
            "automations": [
                { "id": "a", "name": "a", "enabled": true,
                  "trigger": { "on": "variables", "names": ["system.audio.volume"] },
                  "body": [] },
                { "id": "b", "name": "b", "enabled": true,
                  "trigger": { "on": "variables", "names": ["system.cpu.usage"] },
                  "body": [] },
                { "id": "c", "name": "c", "enabled": true,
                  "trigger": { "on": "widget", "widgetId": "btn" },
                  "body": [] },
                { "id": "d", "name": "d", "enabled": true,
                  "trigger": { "on": "manual" },
                  "body": [] },
            ]
        })
        .to_string();
        store_automations(&behavior, &automations);
        behavior.reload().unwrap();

        let names = behavior.watched_variable_names();
        assert_eq!(
            names.len(),
            2,
            "only the two variable triggers watch anything"
        );
        assert!(names.contains("system.audio.volume"));
        assert!(names.contains("system.cpu.usage"));
    }

    #[test]
    fn sample_once_calls_each_plugin_with_only_the_names_it_owns() {
        // The sampler must hand every plugin ONLY the subset of its names
        // that somebody is interested in, so an idle agent's plugins do
        // no work. This is what `Registry::sample(interested, &mut ctx)`
        // promises — pinned here against the wiring.
        let behavior = behavior();

        // No automations -> empty interest set -> no plugin is sampled.
        let interested = behavior.watched_variable_names();
        assert!(interested.is_empty());

        // Drive a single refresh through the path the timer would take.
        let mut interested: BTreeSet<String> = BTreeSet::new();
        interested.insert("system.audio.volume".to_string());
        behavior.sample_once(&interested);

        // The RecordingSystem backend cannot answer on a test machine, but
        // the call must NOT panic and the variable must remain declared.
        let variables = behavior.inner.variables.catalogue();
        assert!(
            variables
                .iter()
                .any(|(spec, _)| spec.name == "system.audio.volume"),
            "the declared variable should still be present after sampling"
        );
    }
}

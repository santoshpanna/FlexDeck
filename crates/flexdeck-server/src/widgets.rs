//! What each widget does and how it looks, read out of the configuration.
//!
//! # A widget, as the engine sees it
//!
//! A widget has **flows** — runs of steps in the automation engine's block
//! language — each started by something: a gesture (short press, long press,
//! double tap) or a plugin announcing an event. And it can have **states**:
//! several looks (Live / Muted, Recording / Stopped) and a rule for which one
//! shows — tapping through them, a variable's value, or an action reporting it.
//!
//! The configuration is read once per revision into a [`WidgetPlan`] per
//! widget, so a press or a variable change never re-parses the document.
//!
//! # Refused on save, not on press
//!
//! [`validate`] is what `PUT /api/config` runs. A flow that does not parse, a
//! state that points at a look that does not exist, two short-press flows on
//! one widget — each is a sentence at save time rather than a button that
//! silently does nothing on the tablet.
//!
//! # Labels that follow values
//!
//! `labelTemplate` fills `{{ … }}` with live values: `"REC {{obs.record_time}}"`,
//! `"{{ system.cpu.usage | round }}%"`. Templates are `flexdeck_expr`'s.
//!
//! # Faces
//!
//! Schema 4 gives a widget a face — a layout, elements, a background, rules —
//! per look; see [`crate::faces`]. A widget without one has the face schema 3
//! drew from its props, so every widget is planned and shown the same way.

use std::collections::{BTreeMap, BTreeSet};

use flexdeck_automation::{validate_blocks, Block, Limits};
use flexdeck_expr::{Locale, Template, Val};
use flexdeck_protocol::{
    flow_trigger, widget_states, CompareOp, Config, Gesture, SliderOrientation, StateEvent, Widget,
    WidgetType,
};
use flexdeck_vars::{Value, VariableStore};

use crate::faces::{plan_faces, FacePlan, VarLookup};

/// The configuration schema this agent writes, and the newest it reads.
pub const CONFIG_SCHEMA: u32 = 4;

/// The oldest schema this agent still reads. Schemas 3 and 4 only *added*
/// (sliders, holds and value flows; faces), so an older deck means the same.
pub const OLDEST_CONFIG_SCHEMA: u32 = 2;

/// How often a slider's flow may run while it is dragged, unless it says.
pub const DEFAULT_SLIDER_THROTTLE_MS: u32 = 50;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WidgetsError {
    #[error(
        "this deck was made for configuration schema {found}, and this agent reads schemas \
         {OLDEST_CONFIG_SCHEMA} to {CONFIG_SCHEMA}; {}",
        if *found > CONFIG_SCHEMA { "it needs a newer agent" } else { "it has to be set up again" }
    )]
    Schema { found: u32 },

    #[error("the configuration is not readable: {0}")]
    Unreadable(String),

    #[error("widget '{widget}': {problem}")]
    Widget { widget: String, problem: String },
}

/// A flow a plugin event starts.
#[derive(Debug, Clone, PartialEq)]
pub struct EventFlow {
    /// Where this flow sits in [`WidgetPlan::flows`].
    pub index: usize,
    pub name: String,
    pub plugin: String,
    pub event: String,
    pub filter: BTreeMap<String, String>,
    pub blocks: Vec<Block>,
}

impl EventFlow {
    /// Whether this flow runs for `plugin`'s `event` with this payload: every
    /// filter key must equal the same payload key.
    pub fn fires_for(&self, plugin: &str, event: &str, payload: &BTreeMap<String, String>) -> bool {
        self.plugin == plugin
            && self.event == event
            && self
                .filter
                .iter()
                .all(|(key, want)| payload.get(key).is_some_and(|got| got == want))
    }
}

/// What decides which look a widget shows.
#[derive(Debug, Clone, PartialEq)]
pub enum StateSource {
    /// Each short press moves to the next look.
    Cycle,
    /// The value of `variable` picks the look; `otherwise` when nothing matches.
    Variable {
        variable: String,
        cases: Vec<Case>,
        otherwise: String,
    },
    /// An action in the widget's flows reports it: the `flow`-th flow, and the
    /// path down to the action (see [`block_at`]).
    Provider { flow: usize, path: Vec<usize> },
}

/// "When the value is `op` `value`, show `state`."
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub op: CompareOp,
    pub value: String,
    pub state: String,
}

/// What a slider widget needs: its range, and the variable it follows.
#[derive(Debug, Clone, PartialEq)]
pub struct SliderPlan {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub variable: Option<String>,
    pub throttle_ms: u32,
    pub orientation: SliderOrientation,
}

impl SliderPlan {
    /// Bring a value a tablet sent into range, on a step. The tablet does this
    /// too; the agent does not rely on it.
    pub fn settle(&self, value: f64) -> f64 {
        let value = if value.is_finite() { value } else { self.min };
        let clamped = value.clamp(self.min, self.max);
        if self.step > 0.0 {
            let steps = ((clamped - self.min) / self.step).round();
            (self.min + steps * self.step).clamp(self.min, self.max)
        } else {
            clamped
        }
    }
}

/// Everything the engine needs about one widget.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetPlan {
    pub widget_id: String,
    /// An icon that follows a value: `{{media.spotify.art}}`.
    pub icon_template: Option<String>,
    /// Set for a slider widget.
    pub slider: Option<SliderPlan>,
    /// The flow a slider's value runs, as an index into `flows`.
    pub value_flow: Option<usize>,
    pub label_template: Option<String>,
    /// The ids of the widget's looks, in order. Empty: one look, its props.
    pub looks: Vec<String>,
    pub source: Option<StateSource>,
    /// Every flow, in the order the configuration lists them — the order a
    /// [`StateSource::Provider`] counts in.
    pub flows: Vec<Vec<Block>>,
    /// The flow each gesture runs, as an index into `flows`.
    pub gestures: BTreeMap<Gesture, usize>,
    /// The flow a gesture on a zone of the face's strip runs.
    pub zone_flows: BTreeMap<(String, Gesture), usize>,
    /// The face's zones that change their icon, with when.
    pub zones: Vec<(String, flexdeck_expr::Expression)>,
    pub events: Vec<EventFlow>,
    /// The face of each look, the widget's own under the empty id.
    pub faces: Vec<(String, FacePlan)>,
    /// The data sources this key runs its own copy of, with their arguments.
    pub uses: Vec<(String, BTreeMap<String, String>)>,
}

/// The variables, with `this.` meaning this key's own run of its first source.
struct ScopedLookup<'a> {
    vars: VarLookup<'a>,
    this: Option<&'a str>,
}

impl flexdeck_expr::Lookup for ScopedLookup<'_> {
    fn get(&self, name: &str) -> Val {
        match (name.strip_prefix("this."), self.this) {
            (Some(field), Some(base)) => self.vars.get(&format!("{base}.{field}")),
            _ => self.vars.get(name),
        }
    }
}

impl WidgetPlan {
    /// The steps a gesture runs, if the widget has any for it.
    pub fn flow_for(&self, gesture: Gesture) -> Option<&[Block]> {
        self.gestures
            .get(&gesture)
            .map(|&index| self.flows[index].as_slice())
    }

    /// The flow a gesture runs on `zone` of the strip — the key's own when
    /// `zone` is empty — as an index into `flows`.
    pub fn flow_index(&self, gesture: Gesture, zone: &str) -> Option<usize> {
        if zone.is_empty() {
            self.gestures.get(&gesture).copied()
        } else {
            self.zone_flows.get(&(zone.to_string(), gesture)).copied()
        }
    }

    /// Whether the widget's look or label ever changes on its own — the
    /// widgets a tablet needs live state for.
    pub fn is_live(&self) -> bool {
        self.label_template.is_some()
            || self.icon_template.is_some()
            || self.source.is_some()
            || self.faces.iter().any(|(_, face)| face.is_live())
            || !self.zones.is_empty()
            || self
                .slider
                .as_ref()
                .is_some_and(|slider| slider.variable.is_some())
    }

    /// Whether a finger going down and lifting has flows of its own, so the
    /// tablet should report the hold rather than a press.
    pub fn holds(&self) -> bool {
        self.gestures.contains_key(&Gesture::HoldStart) || self.gestures.contains_key(&Gesture::HoldEnd)
    }

    /// Every variable the widget's look or label reads.
    pub fn variables(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = self
            .label_template
            .as_deref()
            .map(template_variables)
            .unwrap_or_default();
        if let Some(StateSource::Variable { variable, .. }) = &self.source {
            names.insert(variable.clone());
        }
        if let Some(template) = &self.icon_template {
            names.extend(template_variables(template));
        }
        if let Some(variable) = self.slider.as_ref().and_then(|slider| slider.variable.as_ref()) {
            names.insert(variable.clone());
        }
        for (_, face) in &self.faces {
            names.extend(face.variables());
        }
        for (_, when) in &self.zones {
            names.extend(when.variables());
        }
        // `this.x` is this key's own run's `x`.
        let this = self.this_base();
        let mut names: BTreeSet<String> = names
            .into_iter()
            .filter_map(|name| match name.strip_prefix("this.") {
                Some(field) => this.as_ref().map(|base| format!("{base}.{field}")),
                None => Some(name),
            })
            .collect();
        // A key reading a source also follows whether that source is working.
        let bases: BTreeSet<String> = names.iter().filter_map(|name| crate::sources::base_of(name)).collect();
        for base in bases {
            names.insert(format!("{base}.ok"));
            names.insert(format!("{base}.error"));
        }
        names
    }

    /// `src.<source>.@<widget>`: where this key's own run of its first source
    /// keeps its values, which its templates call `this`.
    pub fn this_base(&self) -> Option<String> {
        self.uses.first().map(|(source, _)| match source.split_once('/') {
            // A plugin's feed: `<plugin>/<feed>`, its values `<plugin>.@<widget>.`.
            Some((plugin, _)) => format!("{plugin}.@{}", self.widget_id),
            None => crate::sources::base_name(source, Some(&self.widget_id)),
        })
    }

    /// Why what it shows may be stale: the first failing source it reads.
    pub fn problem(&self, vars: &VariableStore) -> String {
        let bases: BTreeSet<String> = self.variables().iter().filter_map(|name| crate::sources::base_of(name)).collect();
        for base in bases {
            if vars.get(&format!("{base}.ok")) == Some(Value::Bool(false)) {
                return match vars.get(&format!("{base}.error")) {
                    Some(Value::Text(error)) if !error.is_empty() => error,
                    _ => "a data source it reads is not working".to_string(),
                };
            }
        }
        String::new()
    }

    /// The variables whose history one of its faces graphs, `this.` resolved.
    pub fn graphs(&self) -> BTreeSet<String> {
        let this = self.this_base();
        self.faces
            .iter()
            .flat_map(|(_, face)| face.graphs())
            .filter_map(|name| match name.strip_prefix("this.") {
                Some(field) => this.as_ref().map(|base| format!("{base}.{field}")),
                None => Some(name),
            })
            .collect()
    }

    /// The face that shows in look `state_id`: its own, else the widget's.
    fn face(&self, state_id: &str) -> Option<&FacePlan> {
        self.faces
            .iter()
            .find(|(id, _)| id == state_id)
            .or_else(|| self.faces.first())
            .map(|(_, face)| face)
    }

    pub fn reads(&self, name: &str) -> bool {
        self.variables().contains(name)
    }

    /// The action that reports this widget's state, when its state is
    /// provided by one.
    pub fn provider_block(&self) -> Option<&Block> {
        let Some(StateSource::Provider { flow, path }) = &self.source else {
            return None;
        };
        block_at(self.flows.get(*flow)?, path)
    }

    /// The look that applies now.
    ///
    /// `cycled` is how many times a cycling widget has advanced; `provided` is
    /// the look its providing action last reported.
    pub fn state_id(&self, vars: &VariableStore, cycled: usize, provided: Option<&str>) -> String {
        match &self.source {
            None => String::new(),
            Some(StateSource::Cycle) => self
                .looks
                .get(cycled % self.looks.len().max(1))
                .cloned()
                .unwrap_or_default(),
            Some(StateSource::Variable {
                variable,
                cases,
                otherwise,
            }) => {
                let value = vars.get(variable);
                cases
                    .iter()
                    .find(|case| {
                        value
                            .as_ref()
                            .is_some_and(|value| matches_case(value, case.op, &case.value))
                    })
                    .map(|case| case.state.clone())
                    .unwrap_or_else(|| otherwise.clone())
            }
            // A look the action no longer reports, or none yet: the widget's
            // own props, rather than a stale or invented state.
            Some(StateSource::Provider { .. }) => provided
                .filter(|id| self.looks.iter().any(|look| look == id))
                .map(str::to_string)
                .unwrap_or_default(),
        }
    }

    /// What the tablet should show for this widget now.
    pub fn state(&self, vars: &VariableStore, cycled: usize, provided: Option<&str>) -> StateEvent {
        self.state_in(vars, cycled, provided, &Locale::default())
    }

    /// [`Self::state`], writing numbers and times the `locale`'s way.
    pub fn state_in(
        &self,
        vars: &VariableStore,
        cycled: usize,
        provided: Option<&str>,
        locale: &Locale,
    ) -> StateEvent {
        let state_id = self.state_id(vars, cycled, provided);
        let this = self.this_base();
        let lookup = ScopedLookup { vars: VarLookup(vars), this: this.as_deref() };
        let face = self
            .face(&state_id)
            .map(|face| face.values(&lookup, locale))
            .unwrap_or_default();
        StateEvent {
            widget_id: self.widget_id.clone(),
            ok: true,
            error: String::new(),
            label: self
                .label_template
                .as_deref()
                .map(|template| flexdeck_expr::render(template, &lookup, locale))
                .unwrap_or_default(),
            problem: self.problem(vars),
            elements: face.elements,
            background: face.background,
            pulse: face.pulse,
            background_image: face.background_image,
            zones_on: self
                .zones
                .iter()
                .filter(|(_, when)| when.eval(&lookup, locale).truthy())
                .map(|(name, _)| name.clone())
                .collect(),
            state_id,
            // A value nobody has read yet is absent, so the thumb stays put
            // rather than dropping to zero.
            value: self.slider.as_ref().and_then(|slider| {
                let name = slider.variable.as_ref()?;
                let value = vars.get(name)?.as_f64()?;
                Some(value.clamp(slider.min, slider.max))
            }),
            icon: self
                .icon_template
                .as_deref()
                .map(|template| render_icon(template, vars))
                .unwrap_or_default(),
        }
    }
}

/// Text matches exactly, numbers by value, bools as `true`/`false`. An
/// ordering compares numbers as numbers and anything else as text; a bool is
/// only ever equal or not.
fn matches_case(value: &Value, op: CompareOp, want: &str) -> bool {
    let want = want.trim();
    let ordering = match value {
        Value::Bool(actual) => {
            let Ok(want) = want.parse::<bool>() else { return false };
            return match op {
                CompareOp::Eq => want == *actual,
                CompareOp::Ne => want != *actual,
                _ => false,
            };
        }
        Value::Number(actual) => match want.parse::<f64>() {
            Ok(want) => actual.partial_cmp(&want),
            Err(_) => return false,
        },
        Value::Text(actual) => Some(actual.as_str().cmp(want)),
    };
    let Some(ordering) = ordering else { return false };
    use std::cmp::Ordering::*;
    match op {
        CompareOp::Eq => ordering == Equal,
        CompareOp::Ne => ordering != Equal,
        CompareOp::Lt => ordering == Less,
        CompareOp::Le => ordering != Greater,
        CompareOp::Gt => ordering == Greater,
        CompareOp::Ge => ordering != Less,
    }
}

/// The block at `path`, walking down from `blocks`.
///
/// Each index picks from the current list; the next index goes into that
/// block's children. An `if`'s children are its `then` blocks, then each
/// `else if`'s, then its `otherwise`, in that order; a `repeat`'s are its body.
pub fn block_at<'a>(blocks: &'a [Block], path: &[usize]) -> Option<&'a Block> {
    let (&first, rest) = path.split_first()?;
    let mut current = blocks.get(first)?;
    for &index in rest {
        current = *children(current).get(index)?;
    }
    Some(current)
}

/// A block's children, in the order [`block_at`] counts them.
fn children(block: &Block) -> Vec<&Block> {
    match block {
        Block::If {
            then_blocks,
            else_if,
            otherwise,
            ..
        } => then_blocks
            .iter()
            .chain(else_if.iter().flat_map(|branch| branch.blocks.iter()))
            .chain(otherwise.iter())
            .collect(),
        Block::Repeat { body, .. } => body.iter().collect(),
        _ => Vec::new(),
    }
}

/// Read every widget's plan, refusing a configuration that is not schema 2 or
/// has a widget that could not work.
pub fn parse(config_json: &str, limits: &Limits) -> Result<Vec<WidgetPlan>, WidgetsError> {
    let raw: serde_json::Value = serde_json::from_str(config_json)
        .map_err(|error| WidgetsError::Unreadable(error.to_string()))?;
    let found = raw
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if !(OLDEST_CONFIG_SCHEMA..=CONFIG_SCHEMA).contains(&found) {
        return Err(WidgetsError::Schema { found });
    }
    let config: Config =
        serde_json::from_value(raw).map_err(|error| WidgetsError::Unreadable(error.to_string()))?;

    let mut plans = Vec::new();
    let mut seen = BTreeSet::new();
    for profile in &config.profiles {
        for page in &profile.pages {
            for widget in &page.widgets {
                if !seen.insert(widget.id.clone()) {
                    return Err(WidgetsError::Widget {
                        widget: widget.id.clone(),
                        problem: "another widget has the same id".into(),
                    });
                }
                plans.push(
                    plan(widget, limits).map_err(|problem| WidgetsError::Widget {
                        widget: widget.id.clone(),
                        problem,
                    })?,
                );
            }
        }
    }
    Ok(plans)
}

/// What one widget would show right now, in look `look` (empty: whatever
/// its state source picks), or why it could not work. What the editor's
/// preview asks for, so it never evaluates a template itself.
pub fn preview(
    widget_json: &serde_json::Value,
    look: &str,
    vars: &VariableStore,
    limits: &Limits,
    locale: &Locale,
) -> Result<StateEvent, String> {
    let widget: Widget = serde_json::from_value(widget_json.clone())
        .map_err(|error| format!("the widget is not readable: {error}"))?;
    let plan = plan(&widget, limits)?;
    let mut state = plan.state_in(vars, 0, None, locale);
    if !look.is_empty() && plan.looks.iter().any(|known| known == look) {
        let this = plan.this_base();
        let lookup = ScopedLookup { vars: VarLookup(vars), this: this.as_deref() };
        let face = plan
            .face(look)
            .map(|face| face.values(&lookup, locale))
            .unwrap_or_default();
        state.state_id = look.to_string();
        state.elements = face.elements;
        state.background = face.background;
        state.pulse = face.pulse;
        state.background_image = face.background_image;
    }
    Ok(state)
}

/// Refuse a configuration [`parse`] would refuse. What `PUT /api/config` runs.
pub fn validate(config_json: &str, limits: &Limits) -> Result<(), WidgetsError> {
    parse(config_json, limits).map(drop)
}

fn gesture_name(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::ShortPress => "short press",
        Gesture::LongPress => "long press",
        Gesture::DoubleTap => "double tap",
        Gesture::HoldStart => "hold start",
        Gesture::HoldEnd => "release",
        Gesture::Unspecified => "unspecified gesture",
    }
}

/// Refuse a widget (as JSON) this agent would refuse on save: a preset a
/// recipe or a plugin brings is checked with this before it is offered.
pub fn check_widget(widget: &serde_json::Value) -> Result<(), String> {
    let widget: Widget = serde_json::from_value(widget.clone()).map_err(|error| format!("not a widget: {error}"))?;
    plan(&widget, &Limits::default()).map(drop)
}

/// The most zones one face's strip may have.
pub const MAX_ZONES: usize = 5;

fn plan_zones(widget: &Widget) -> Result<(BTreeSet<String>, Vec<(String, flexdeck_expr::Expression)>), String> {
    let zones = widget.face.as_ref().map(|face| face.zones.as_slice()).unwrap_or_default();
    if zones.len() > MAX_ZONES {
        return Err(format!("its face's strip has more than {MAX_ZONES} zones"));
    }
    let mut names = BTreeSet::new();
    let mut live = Vec::new();
    for (index, zone) in zones.iter().enumerate() {
        let name = zone.name.trim();
        let ok = !name.is_empty()
            && name.len() <= 24
            && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !ok {
            return Err(format!("its face's zone {} needs a name of lowercase letters, digits, - and _", index + 1));
        }
        if !names.insert(name.to_string()) {
            return Err(format!("its face has two zones called '{name}'"));
        }
        if !zone.when.trim().is_empty() {
            let when = flexdeck_expr::parse_expression(&zone.when).map_err(|error| format!("its face's zone '{name}': {error}"))?;
            live.push((name.to_string(), when));
        }
    }
    Ok((names, live))
}

fn plan(widget: &Widget, limits: &Limits) -> Result<WidgetPlan, String> {
    let mut flows = Vec::with_capacity(widget.flows.len());
    let mut gestures = BTreeMap::new();
    let mut zone_flows = BTreeMap::new();
    let (zone_names, zones) = plan_zones(widget)?;
    let mut events = Vec::new();
    let mut value_flow = None;
    let is_slider = widget.r#type == WidgetType::Slider as i32;

    for (index, flow) in widget.flows.iter().enumerate() {
        let what = if flow.name.trim().is_empty() {
            format!("flow {}", index + 1)
        } else {
            format!("flow '{}'", flow.name.trim())
        };
        let blocks: Vec<Block> = match &flow.blocks {
            None => Vec::new(),
            Some(list) => serde_json::to_value(list)
                .and_then(serde_json::from_value)
                .map_err(|error| format!("{what} has a step that is not understood: {error}"))?,
        };
        validate_blocks(&blocks, limits).map_err(|error| format!("{what}: {error}"))?;

        match flow
            .trigger
            .as_ref()
            .and_then(|trigger| trigger.on.as_ref())
        {
            None => return Err(format!("{what} has nothing that starts it")),
            Some(flow_trigger::On::Gesture(raw)) => {
                let gesture = Gesture::try_from(*raw)
                    .ok()
                    .filter(|gesture| *gesture != Gesture::Unspecified)
                    .ok_or_else(|| format!("{what} is started by an unknown gesture"))?;
                let zone = flow.zone.trim();
                if zone.is_empty() {
                    if gestures.insert(gesture, index).is_some() {
                        return Err(format!("it has two {} flows", gesture_name(gesture)));
                    }
                } else {
                    if !zone_names.contains(zone) {
                        return Err(format!("{what} is for the zone '{zone}', which its face does not have"));
                    }
                    if matches!(gesture, Gesture::HoldStart | Gesture::HoldEnd) {
                        return Err(format!("{what}: a zone is pressed, not held"));
                    }
                    if zone_flows.insert((zone.to_string(), gesture), index).is_some() {
                        return Err(format!("its zone '{zone}' has two {} flows", gesture_name(gesture)));
                    }
                }
            }
            Some(_) if !flow.zone.trim().is_empty() => {
                return Err(format!("{what} is in a zone, but only a press starts a zone's flow"));
            }
            Some(flow_trigger::On::Value(_)) => {
                if !is_slider {
                    return Err(format!("{what} runs on a value, but only a slider has one"));
                }
                if value_flow.replace(index).is_some() {
                    return Err("it has two flows that run on its value".into());
                }
            }
            Some(flow_trigger::On::Event(event)) => {
                if event.plugin.trim().is_empty() || event.event.trim().is_empty() {
                    return Err(format!("{what} is started by an event, but not which one"));
                }
                events.push(EventFlow {
                    index,
                    name: flow.name.clone(),
                    plugin: event.plugin.clone(),
                    event: event.event.clone(),
                    filter: event.filter.clone().into_iter().collect(),
                    blocks: blocks.clone(),
                });
            }
        }
        flows.push(blocks);
    }

    let (looks, source) = match &widget.states {
        None => (Vec::new(), None),
        Some(states) => {
            let mut looks = Vec::with_capacity(states.looks.len());
            for look in &states.looks {
                let id = look.id.trim();
                if id.is_empty() {
                    return Err("a state has no id".into());
                }
                if looks.iter().any(|known: &String| known == id) {
                    return Err(format!("two states are called '{id}'"));
                }
                looks.push(id.to_string());
            }
            let has = |id: &str| looks.iter().any(|known| known == id);
            let source = match &states.source {
                None => None,
                Some(_) if looks.is_empty() => {
                    return Err("it says what picks its state, but has no states".into())
                }
                Some(widget_states::Source::Cycle(_)) => Some(StateSource::Cycle),
                Some(widget_states::Source::Variable(variable)) => {
                    let name = variable.variable.trim();
                    if name.is_empty() {
                        return Err("its state follows a variable, but not which one".into());
                    }
                    for case in &variable.cases {
                        if !has(case.state.trim()) {
                            return Err(format!(
                                "a case shows state '{}', which it does not have",
                                case.state
                            ));
                        }
                    }
                    let otherwise = variable.otherwise.trim();
                    if !otherwise.is_empty() && !has(otherwise) {
                        return Err(format!(
                            "its fallback is state '{otherwise}', which it does not have"
                        ));
                    }
                    Some(StateSource::Variable {
                        variable: name.to_string(),
                        cases: variable
                            .cases
                            .iter()
                            .map(|case| Case {
                                op: CompareOp::try_from(case.op).unwrap_or(CompareOp::Eq),
                                value: case.value.clone(),
                                state: case.state.trim().to_string(),
                            })
                            .collect(),
                        otherwise: otherwise.to_string(),
                    })
                }
                Some(widget_states::Source::Provider(provider)) => {
                    let flow = provider.flow as usize;
                    let path: Vec<usize> = provider.path.iter().map(|&i| i as usize).collect();
                    match flows.get(flow).and_then(|blocks| block_at(blocks, &path)) {
                        Some(Block::DoAction { .. }) => {}
                        _ => {
                            return Err(
                                "its state comes from an action that is no longer in its flows"
                                    .into(),
                            )
                        }
                    }
                    Some(StateSource::Provider { flow, path })
                }
            };
            (looks, source)
        }
    };

    let slider = if is_slider {
        let props = widget.slider.clone().unwrap_or_default();
        let (min, max) = if props.min == 0.0 && props.max == 0.0 {
            (0.0, 100.0)
        } else {
            (props.min, props.max)
        };
        if !(min.is_finite() && max.is_finite() && min < max) {
            return Err(format!("its slider runs from {min} to {max}, which is not a range"));
        }
        if !props.step.is_finite() || props.step < 0.0 || props.step > max - min {
            return Err(format!("its slider moves in steps of {}, which does not fit its range", props.step));
        }
        // A slider that drives nothing yet is inert, like a button with no
        // action: the editor saves on every change, so it passes through here.
        let variable = Some(props.variable.trim().to_string()).filter(|name| !name.is_empty());
        Some(SliderPlan {
            min,
            max,
            step: props.step,
            variable,
            throttle_ms: if props.throttle_ms == 0 {
                DEFAULT_SLIDER_THROTTLE_MS
            } else {
                props.throttle_ms.min(5_000)
            },
            orientation: SliderOrientation::try_from(props.orientation).unwrap_or_default(),
        })
    } else {
        None
    };

    // The live label and icon were never parsed in schema 3; a broken one now
    // says so here, in the words a person used for it.
    for (what, template) in [("label", &widget.props.as_ref().map(|p| p.label_template.clone())), ("icon", &widget.props.as_ref().map(|p| p.icon_template.clone()))] {
        if let Some(template) = template.as_deref().filter(|t| !t.trim().is_empty()) {
            Template::parse(template).map_err(|error| format!("its live {what}: {error}"))?;
        }
    }
    let faces = plan_faces(widget)?;
    let mut uses = Vec::new();
    for used in &widget.sources {
        let source = used.source.trim();
        if source.is_empty() {
            return Err("it runs a data source, but not which one".into());
        }
        uses.push((source.to_string(), used.args.clone().into_iter().collect()));
    }

    Ok(WidgetPlan {
        widget_id: widget.id.clone(),
        faces,
        uses,
        icon_template: widget
            .props
            .as_ref()
            .map(|props| props.icon_template.clone())
            .filter(|template| !template.trim().is_empty()),
        slider,
        value_flow,
        label_template: widget
            .props
            .as_ref()
            .map(|props| props.label_template.clone())
            .filter(|template| !template.trim().is_empty()),
        looks,
        source,
        flows,
        gestures,
        zone_flows,
        zones,
        events,
    })
}

/// The variables a template reads. A template that does not parse reads none.
pub fn template_variables(template: &str) -> BTreeSet<String> {
    Template::parse(template)
        .map(|template| template.variables())
        .unwrap_or_default()
}

/// An icon template's value: an icon name or an `img:` address as it is (it
/// must not become "–"), or empty while anything in it is unknown, so the
/// configured icon shows instead.
pub fn render_icon(template: &str, vars: &VariableStore) -> String {
    match Template::parse(template)
        .map(|template| template.value_if_known(&VarLookup(vars), &Locale::default()))
    {
        Ok(Val::Text(text)) => text,
        _ => String::new(),
    }
}

/// Fill a template with current values. A value nobody has sampled yet
/// renders as `–`, not as an empty string or `0`: a blank label looks
/// broken, and a zero looks like a real reading.
pub fn render(template: &str, vars: &VariableStore) -> String {
    render_in(template, vars, &Locale::default())
}

fn render_in(template: &str, vars: &VariableStore, locale: &Locale) -> String {
    flexdeck_expr::render(template, &VarLookup(vars), locale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_vars::{Kind, VariableSpec};
    use serde_json::json;

    fn hotkey(keys: &str) -> serde_json::Value {
        json!({"block": "doAction", "plugin": "core", "action": "hotkey",
               "args": {"keys": {"text": keys}}})
    }

    fn config(widgets: serde_json::Value) -> String {
        json!({
            "schema": 2,
            "profiles": [{"id": "p", "pages": [{"id": "page", "widgets": widgets}]}]
        })
        .to_string()
    }

    fn one(widget: serde_json::Value) -> Result<WidgetPlan, WidgetsError> {
        parse(&config(json!([widget])), &Limits::default()).map(|mut plans| plans.remove(0))
    }

    fn vars() -> VariableStore {
        let vars = VariableStore::new();
        vars.declare(VariableSpec::new("obs.recording", Kind::Bool, ""));
        vars.declare(VariableSpec::new("obs.record_time", Kind::Text, ""));
        vars.declare(VariableSpec::new("obs.scene", Kind::Text, ""));
        vars.declare(VariableSpec::new("system.audio.volume", Kind::Number, ""));
        vars
    }

    #[test]
    fn each_gesture_has_its_own_flow() {
        let plan = one(json!({
            "id": "w", "type": "WIDGET_TYPE_BUTTON",
            "flows": [
                {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [hotkey("a")]},
                {"trigger": {"gesture": "GESTURE_LONG_PRESS"}, "blocks": [hotkey("b"), hotkey("c")]}
            ]
        }))
        .unwrap();
        assert_eq!(plan.flow_for(Gesture::ShortPress).unwrap().len(), 1);
        assert_eq!(plan.flow_for(Gesture::LongPress).unwrap().len(), 2);
        assert!(plan.flow_for(Gesture::DoubleTap).is_none());
    }

    #[test]
    fn a_zone_of_the_strip_runs_its_own_flow_and_shows_its_state() {
        let music = |flows: serde_json::Value| {
            one(json!({
                "id": "w", "type": "WIDGET_TYPE_BUTTON",
                "face": {"zones": [
                    {"name": "prev", "icon": "skip-back"},
                    {"name": "play", "icon": "play", "when": "obs.recording", "iconOn": "pause"},
                    {"name": "next", "icon": "skip-forward"}
                ]},
                "flows": flows
            }))
        };
        let plan = music(json!([
            {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [hotkey("open")]},
            {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "zone": "next", "blocks": [hotkey("n")]},
            {"trigger": {"gesture": "GESTURE_LONG_PRESS"}, "zone": "next", "blocks": [hotkey("n"), hotkey("n")]}
        ]))
        .unwrap();
        assert_eq!(plan.flow_index(Gesture::ShortPress, ""), Some(0));
        assert_eq!(plan.flow_index(Gesture::ShortPress, "next"), Some(1));
        assert_eq!(plan.flow_index(Gesture::LongPress, "next"), Some(2));
        assert_eq!(plan.flow_index(Gesture::ShortPress, "prev"), None, "never the key's own flow");
        assert!(plan.variables().contains("obs.recording"));

        let vars = vars();
        assert!(plan.state(&vars, 0, None).zones_on.is_empty());
        vars.set("obs.recording", Value::Bool(true)).unwrap();
        assert_eq!(plan.state(&vars, 0, None).zones_on, vec!["play".to_string()]);

        let refused = |flows: serde_json::Value| music(flows).unwrap_err().to_string();
        assert!(refused(json!([{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "zone": "shuffle", "blocks": []}])).contains("does not have"));
        assert!(refused(json!([{"trigger": {"gesture": "GESTURE_HOLD_START"}, "zone": "play", "blocks": []}])).contains("not held"));
        let twice = json!({"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "zone": "play", "blocks": []});
        assert!(refused(json!([twice.clone(), twice])).contains("two"));
        let bad = one(json!({"id": "w", "type": "WIDGET_TYPE_BUTTON", "face": {"zones": [{"name": "Play"}]}}));
        assert!(bad.unwrap_err().to_string().contains("lowercase"));
    }

    fn slider(extra: serde_json::Value) -> serde_json::Value {
        let mut widget = json!({"id": "s", "type": "WIDGET_TYPE_SLIDER",
            "flows": [{"trigger": {"value": {}}, "blocks": [hotkey("a")]}]});
        for (key, value) in extra.as_object().unwrap() {
            widget[key] = value.clone();
        }
        widget
    }

    #[test]
    fn an_icon_template_shows_an_image_address_as_it_is_and_nothing_while_unknown() {
        let plan = one(json!({"id": "art", "props": {"iconTemplate": "{{media.tidal.art}}"}})).unwrap();
        assert!(plan.is_live() && plan.reads("media.tidal.art"));
        let vars = VariableStore::new();
        vars.declare(VariableSpec::new("media.tidal.art", Kind::Text, ""));
        assert_eq!(plan.state(&vars, 0, None).icon, "", "unknown: the configured icon");
        let address = format!("img:{}", "ab".repeat(32));
        vars.set("media.tidal.art", Value::Text(address.clone())).unwrap();
        assert_eq!(plan.state(&vars, 0, None).icon, address, "not '–', not changed");
    }
    #[test]
    fn schemas_two_to_four_all_read() {
        for schema in 2..=4 {
            let deck = json!({"schema": schema, "profiles": []}).to_string();
            validate(&deck, &Limits::default()).unwrap_or_else(|error| panic!("schema {schema}: {error}"));
        }
        let five = json!({"schema": 5, "profiles": []}).to_string();
        let error = validate(&five, &Limits::default()).unwrap_err();
        assert!(error.to_string().contains("newer agent"), "{error}");
    }

    #[test]
    fn a_slider_defaults_to_zero_to_a_hundred_and_settles_values_on_its_step() {
        let plan = one(slider(json!({"slider": {"variable": "system.audio.volume"}}))).unwrap();
        let range = plan.slider.as_ref().unwrap();
        assert_eq!((range.min, range.max), (0.0, 100.0));
        assert_eq!(range.throttle_ms, DEFAULT_SLIDER_THROTTLE_MS);
        assert_eq!(plan.value_flow, Some(0));
        assert!(plan.is_live(), "it follows a variable");

        let stepped = one(slider(json!({"slider": {"min": 0, "max": 10, "step": 2.5}}))).unwrap();
        let range = stepped.slider.unwrap();
        assert_eq!(range.settle(3.7), 2.5);
        assert_eq!(range.settle(99.0), 10.0);
        assert_eq!(range.settle(-5.0), 0.0);
        assert_eq!(range.settle(f64::NAN), 0.0);
    }

    #[test]
    fn a_sliders_state_carries_its_value_only_once_it_is_known() {
        let plan = one(slider(json!({"slider": {"variable": "system.audio.volume"}}))).unwrap();
        let vars = vars();
        assert_eq!(plan.state(&vars, 0, None).value, None, "unknown is absent, not 0");
        vars.set("system.audio.volume", Value::Number(140.0)).unwrap();
        assert_eq!(plan.state(&vars, 0, None).value, Some(100.0), "clamped into range");
        assert!(plan.variables().contains("system.audio.volume"));
    }

    #[test]
    fn a_slider_that_drives_nothing_yet_is_inert_not_refused() {
        let plan = one(json!({"id": "s", "type": "WIDGET_TYPE_SLIDER"})).unwrap();
        assert!(plan.slider.is_some());
        assert_eq!(plan.value_flow, None);
        assert!(!plan.is_live());
    }

    #[test]
    fn a_broken_slider_is_refused_with_a_reason() {
        let problem = |widget| match one(widget) {
            Err(WidgetsError::Widget { problem, .. }) => problem,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(problem(slider(json!({"slider": {"min": 5, "max": 5}}))).contains("not a range"));
        assert!(problem(slider(json!({"slider": {"step": 500}}))).contains("steps"));
        assert!(problem(json!({"id": "b", "type": "WIDGET_TYPE_BUTTON",
            "flows": [{"trigger": {"value": {}}}]}))
        .contains("only a slider"));
        assert!(problem(slider(json!({"flows": [
            {"trigger": {"value": {}}}, {"trigger": {"value": {}}}
        ]})))
        .contains("two flows"));
    }

    #[test]
    fn a_button_with_hold_flows_says_so() {
        let plan = one(json!({"id": "ptt", "type": "WIDGET_TYPE_BUTTON", "flows": [
            {"trigger": {"gesture": "GESTURE_HOLD_START"}, "blocks": [hotkey("a")]},
            {"trigger": {"gesture": "GESTURE_HOLD_END"}, "blocks": [hotkey("b")]}
        ]}))
        .unwrap();
        assert!(plan.holds());
        assert!(plan.flow_for(Gesture::HoldEnd).is_some());
    }

    #[test]
    fn a_schema_one_deck_is_refused_by_name() {
        let old = json!({"schema": 1, "profiles": []}).to_string();
        assert_eq!(
            validate(&old, &Limits::default()),
            Err(WidgetsError::Schema { found: 1 })
        );
        assert!(validate("not json", &Limits::default()).is_err());
    }

    #[test]
    fn broken_widgets_are_refused_with_a_reason() {
        let two_short = one(json!({"id": "w", "flows": [
            {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}},
            {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}}
        ]}));
        assert!(
            matches!(two_short, Err(WidgetsError::Widget { problem, .. }) if problem.contains("two short press"))
        );

        let bad_step = one(json!({"id": "w", "flows": [
            {"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [{"block": "teleport"}]}
        ]}));
        assert!(
            matches!(bad_step, Err(WidgetsError::Widget { problem, .. }) if problem.contains("not understood"))
        );

        let no_trigger = one(json!({"id": "w", "flows": [{"blocks": []}]}));
        assert!(no_trigger.is_err());

        let missing_look = one(json!({"id": "w", "states": {
            "looks": [{"id": "on"}],
            "variable": {"variable": "obs.recording", "cases": [{"value": "true", "state": "lit"}]}
        }}));
        assert!(
            matches!(missing_look, Err(WidgetsError::Widget { problem, .. }) if problem.contains("'lit'"))
        );

        let duplicate = parse(
            &config(json!([{"id": "w"}, {"id": "w"}])),
            &Limits::default(),
        );
        assert!(duplicate.is_err());
    }

    #[test]
    fn a_variable_picks_the_look_and_unknown_falls_back() {
        let plan = one(json!({"id": "rec", "states": {
            "looks": [{"id": "idle"}, {"id": "recording", "bgColor": "#b91c1c"}],
            "variable": {"variable": "obs.recording",
                         "cases": [{"value": "true", "state": "recording"}],
                         "otherwise": "idle"}
        }}))
        .unwrap();
        let vars = vars();
        assert_eq!(
            plan.state_id(&vars, 0, None),
            "idle",
            "unknown: the fallback"
        );
        vars.refresh("obs.recording", Value::Bool(true)).unwrap();
        assert_eq!(plan.state_id(&vars, 0, None), "recording");
        vars.refresh("obs.recording", Value::Bool(false)).unwrap();
        assert_eq!(plan.state_id(&vars, 0, None), "idle");
        assert!(plan.reads("obs.recording"));
    }

    #[test]
    fn numbers_match_by_value_and_text_exactly() {
        let eq = CompareOp::Eq;
        assert!(matches_case(&Value::Number(40.0), eq, "40"));
        assert!(matches_case(&Value::Number(40.0), eq, "40.0"));
        assert!(!matches_case(&Value::Text("Live".into()), eq, "live"));
        assert!(matches_case(&Value::Text("Live".into()), eq, "Live"));
        assert!(matches_case(&Value::Bool(true), eq, "true"));
        assert!(!matches_case(&Value::Bool(true), eq, "yes"));
    }

    #[test]
    fn a_case_can_be_a_range() {
        assert!(matches_case(&Value::Number(91.0), CompareOp::Gt, "80"));
        assert!(!matches_case(&Value::Number(80.0), CompareOp::Gt, "80"));
        assert!(matches_case(&Value::Number(80.0), CompareOp::Ge, "80"));
        assert!(matches_case(&Value::Number(3.0), CompareOp::Lt, "10"), "as numbers, not text");
        assert!(!matches_case(&Value::Number(3.0), CompareOp::Lt, "ten"));
        assert!(!matches_case(&Value::Bool(true), CompareOp::Gt, "false"), "a bool has no order");
        assert!(matches_case(&Value::Bool(true), CompareOp::Ne, "false"));

        let plan = one(json!({"id": "cpu", "states": {
            "looks": [{"id": "cool"}, {"id": "warm"}, {"id": "hot"}],
            "variable": {"variable": "system.audio.volume", "otherwise": "cool", "cases": [
                {"op": "COMPARE_OP_GT", "value": "80", "state": "hot"},
                {"op": "COMPARE_OP_GT", "value": "50", "state": "warm"}
            ]}
        }}))
        .unwrap();
        let vars = vars();
        let look = |level: f64| {
            vars.set("system.audio.volume", Value::Number(level)).unwrap();
            plan.state_id(&vars, 0, None)
        };
        assert_eq!([look(20.0), look(60.0), look(95.0)], ["cool", "warm", "hot"], "the first that holds");
    }

    #[test]
    fn a_schema_four_face_is_planned_and_its_values_ride_on_the_state() {
        let plan = one(json!({"id": "cpu", "face": {
            "layout": "FACE_LAYOUT_VALUE",
            "rules": [{"when": "system.audio.volume > 80", "color": "#7f1d1d"}],
            "elements": [
                {"slot": "SLOT_VALUE", "template": "{{ system.audio.volume | round }}%"},
                {"slot": "SLOT_VISUAL", "kind": "ELEMENT_KIND_GRAPH", "variable": "system.audio.volume"}
            ]
        }}))
        .unwrap();
        assert!(plan.is_live());
        assert_eq!(plan.graphs().into_iter().collect::<Vec<_>>(), ["system.audio.volume"]);
        let vars = vars();
        vars.set("system.audio.volume", Value::Number(90.4)).unwrap();
        let state = plan.state(&vars, 0, None);
        assert_eq!(state.elements[0].text, "90%");
        assert_eq!(state.background, "#7f1d1d");
        assert_eq!(state.label, "", "no live label: the old field stays empty");
    }

    #[test]
    fn a_looks_own_face_shows_while_that_look_does() {
        let plan = one(json!({"id": "rec",
            "props": {"label": "Rec"},
            "states": {
                "looks": [{"id": "idle"}, {"id": "live", "face": {"elements": [{"template": "{{obs.record_time}}"}]}}],
                "variable": {"variable": "obs.recording", "cases": [{"value": "true", "state": "live"}], "otherwise": "idle"}
            }
        }))
        .unwrap();
        let vars = vars();
        vars.set("obs.record_time", Value::Text("00:00:09".into())).unwrap();
        vars.set("obs.recording", Value::Bool(false)).unwrap();
        assert_eq!(plan.state(&vars, 0, None).elements[0].text, "Rec");
        vars.set("obs.recording", Value::Bool(true)).unwrap();
        assert_eq!(plan.state(&vars, 0, None).elements[0].text, "00:00:09");
        assert!(plan.reads("obs.record_time"), "a look's face is read even while it does not show");
    }

    /// The gallery's presets (`contract/fixtures/face/presets.json`, written
    /// by `pnpm gen:presets`): every one must be a widget this agent accepts.
    #[test]
    fn every_gallery_preset_is_a_widget_the_agent_accepts() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/fixtures/face/presets.json"))
            .expect("contract/fixtures/face/presets.json");
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let presets = doc["presets"].as_array().unwrap();
        assert!(presets.len() >= 15);
        for preset in presets {
            let mut widget = preset["widget"].clone();
            widget["id"] = json!(preset["id"]);
            if let Err(error) = one(widget) {
                panic!("preset {}: {error}", preset["id"]);
            }
        }
    }

    #[test]
    fn a_key_reads_its_own_run_of_a_source_as_this_and_says_when_it_fails() {
        let plan = one(json!({"id": "w12",
            "sources": [{"source": "weather", "args": {"city": "Tokyo"}}],
            "face": {"elements": [{"template": "{{ this.temp | round }}°"}]}
        }))
        .unwrap();
        assert_eq!(plan.this_base().as_deref(), Some("src.weather.@w12"));
        let names = plan.variables();
        assert!(names.contains("src.weather.@w12.temp"), "{names:?}");
        assert!(names.contains("src.weather.@w12.ok"), "it follows whether its source works");

        let vars = VariableStore::new();
        vars.declare(VariableSpec::new("src.weather.@w12.temp", Kind::Number, ""));
        vars.declare(VariableSpec::new("src.weather.@w12.ok", Kind::Bool, ""));
        vars.declare(VariableSpec::new("src.weather.@w12.error", Kind::Text, ""));
        vars.set("src.weather.@w12.temp", Value::Number(18.6)).unwrap();
        vars.set("src.weather.@w12.ok", Value::Bool(true)).unwrap();
        let state = plan.state(&vars, 0, None);
        assert_eq!(state.elements[0].text, "19°");
        assert_eq!(state.problem, "");

        vars.set("src.weather.@w12.ok", Value::Bool(false)).unwrap();
        vars.set("src.weather.@w12.error", Value::Text("Weather: api.open-meteo.com answered 500".into())).unwrap();
        assert_eq!(plan.state(&vars, 0, None).problem, "Weather: api.open-meteo.com answered 500");
        assert!(one(json!({"id": "w", "sources": [{"source": " "}]})).is_err());
    }

    #[test]
    fn a_broken_template_is_refused_on_save() {
        let problem = |widget| match one(widget) {
            Err(WidgetsError::Widget { problem, .. }) => problem,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(problem(json!({"id": "w", "props": {"labelTemplate": "{{ x | rond }}"}}))
            .contains("its live label: 'rond' is not a filter"));
        assert!(problem(json!({"id": "w", "face": {"rules": [{"when": "x >"}]}}))
            .contains("rule 1"));
    }

    #[test]
    fn a_cycling_widget_wraps_around() {
        let plan = one(json!({"id": "w", "states": {
            "looks": [{"id": "a"}, {"id": "b"}, {"id": "c"}], "cycle": {}
        }}))
        .unwrap();
        let vars = vars();
        let seen: Vec<String> = (0..4).map(|n| plan.state_id(&vars, n, None)).collect();
        assert_eq!(seen, ["a", "b", "c", "a"]);
    }

    #[test]
    fn a_provided_state_must_be_one_of_the_looks_and_its_action_must_exist() {
        let plan = one(json!({"id": "w",
            "flows": [{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [hotkey("a")]}],
            "states": {"looks": [{"id": "live"}, {"id": "idle"}], "provider": {"flow": 0, "path": [0]}}
        }))
        .unwrap();
        let vars = vars();
        assert_eq!(plan.state_id(&vars, 0, Some("live")), "live");
        assert_eq!(
            plan.state_id(&vars, 0, Some("bogus")),
            "",
            "not a look: the props"
        );
        assert!(matches!(
            plan.provider_block(),
            Some(Block::DoAction { .. })
        ));

        let gone = one(json!({"id": "w",
            "flows": [{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": [hotkey("a")]}],
            "states": {"looks": [{"id": "live"}], "provider": {"flow": 0, "path": [3]}}
        }));
        assert!(gone.is_err());
    }

    #[test]
    fn a_nested_action_is_found_by_its_path() {
        let blocks: Vec<Block> = serde_json::from_value(json!([
            {"block": "wait", "ms": {"number": 10}},
            {"block": "if",
             "condition": {"kind": "compare", "left": {"bool": true}, "op": "eq", "right": {"bool": true}},
             "then": [hotkey("then")],
             "elseIf": [{"condition": {"kind": "compare", "left": {"bool": true}, "op": "eq", "right": {"bool": true}},
                         "blocks": [hotkey("elseif")]}],
             "otherwise": [hotkey("otherwise")]},
            {"block": "repeat", "times": {"number": 2}, "body": [hotkey("body")]}
        ]))
        .unwrap();
        let keys = |path: &[usize]| match block_at(&blocks, path) {
            Some(Block::DoAction { args, .. }) => format!("{:?}", args.get("keys")),
            other => format!("{other:?}"),
        };
        assert!(keys(&[1, 0]).contains("then"));
        assert!(keys(&[1, 1]).contains("elseif"));
        assert!(keys(&[1, 2]).contains("otherwise"));
        assert!(keys(&[2, 0]).contains("body"));
        assert!(block_at(&blocks, &[1, 3]).is_none());
        assert!(block_at(&blocks, &[]).is_none());
    }

    #[test]
    fn an_event_flow_fires_only_on_a_matching_payload() {
        let plan = one(json!({"id": "w", "flows": [{
            "name": "Live goes red",
            "trigger": {"event": {"plugin": "obs", "event": "scene_changed", "filter": {"scene": "Live"}}}
        }]}))
        .unwrap();
        let flow = &plan.events[0];
        let payload = |scene: &str| BTreeMap::from([("scene".to_string(), scene.to_string())]);
        assert!(flow.fires_for("obs", "scene_changed", &payload("Live")));
        assert!(!flow.fires_for("obs", "scene_changed", &payload("BRB")));
        assert!(!flow.fires_for("obs", "stream_started", &payload("Live")));
    }

    #[test]
    fn a_label_follows_its_template() {
        let plan =
            one(json!({"id": "rec", "props": {"labelTemplate": "REC {{ obs.record_time }}"}}))
                .unwrap();
        let vars = vars();
        assert_eq!(plan.state(&vars, 0, None).label, "REC –");
        vars.refresh("obs.record_time", Value::Text("00:01:05".into()))
            .unwrap();
        assert_eq!(plan.state(&vars, 0, None).label, "REC 00:01:05");
        assert!(plan.is_live());
        assert!(!one(json!({"id": "plain", "props": {"label": "Plain"}}))
            .unwrap()
            .is_live());
    }

    #[test]
    fn templates_render_each_kind_and_say_when_a_value_is_unknown() {
        let vars = vars();
        assert_eq!(render("{{obs.scene}}", &vars), "–");
        vars.refresh("system.audio.volume", Value::Number(40.0))
            .unwrap();
        assert_eq!(render("Vol {{system.audio.volume}}%", &vars), "Vol 40%");
        vars.refresh("obs.recording", Value::Bool(false)).unwrap();
        assert_eq!(render("{{obs.recording}}", &vars), "off");
        assert_eq!(render("no braces", &vars), "no braces");
        assert_eq!(render("{{ unclosed", &vars), "{{ unclosed");
    }

    #[test]
    fn a_fade_with_a_direction_or_a_round_shape_can_be_saved() {
        for background in [
            json!({"color": "#111111", "colorTo": "#222222", "angle": 135}),
            json!({"color": "#111111", "colorTo": "#222222", "gradient": "GRADIENT_RADIAL"}),
        ] {
            let widget = json!({"id": "w", "face": {"layout": "FACE_LAYOUT_VALUE", "background": background}});
            assert_eq!(check_widget(&widget), Ok(()), "{widget}");
        }
    }
}

//! What a key's face shows: its elements' live values, and the colour its
//! rules pick.
//!
//! # One face per look
//!
//! Every widget has a face for each of its looks, worked out by
//! [`face_for`]: the look's own face if it has one, else the widget's (its
//! `face`, or one [`derive`]d from schema 3's `props`) with the look's label,
//! icon and colours laid over it. The tablet and the editor work it out the
//! same way — `contract/fixtures/face/derive.json` holds the three of them
//! together — so the agent only sends what changes: each element's filled
//! template, a gauge's number, a rule's colour.
//!
//! # Parsed once, refused on save
//!
//! Every template and every rule is parsed when the deck is read into a
//! [`FacePlan`]. One that does not parse refuses the save with a sentence
//! naming the element, the same as a flow that does not.

use std::collections::BTreeSet;

use flexdeck_expr::{parse_expression, Expression, Locale, Lookup, Template, Val};
use flexdeck_protocol::{
    Background, Element, ElementKind, ElementValue, Face, FaceLayout, Rule, Slot, StateLook, Widget,
    WidgetProps,
};
use flexdeck_vars::{Value, VariableStore};

/// The most elements one face may have.
pub const MAX_ELEMENTS: usize = 12;

/// The most rules one face or element may have.
pub const MAX_RULES: usize = 8;

/// The variable store, as expressions read it.
pub struct VarLookup<'a>(pub &'a VariableStore);

impl Lookup for VarLookup<'_> {
    fn get(&self, name: &str) -> Val {
        match self.0.get(name) {
            Some(Value::Number(n)) => Val::Number(n),
            Some(Value::Text(text)) => Val::Text(text),
            Some(Value::Bool(b)) => Val::Bool(b),
            None => Val::Missing,
        }
    }
}

/// The face schema 3 drew from `props`: an icon over a label, in the props'
/// colours. A live label (`labelTemplate`) wins over the plain one, and a
/// live icon over the plain icon — which stays as what shows while the live
/// one is unknown.
pub fn derive(props: Option<&WidgetProps>) -> Face {
    let props = props.cloned().unwrap_or_default();
    let mut elements = Vec::new();
    let icon = first_set(&[&props.icon_template, &props.icon]);
    if !icon.is_empty() {
        elements.push(Element {
            slot: Slot::Icon as i32,
            kind: ElementKind::Icon as i32,
            template: icon.to_string(),
            color: props.fg_color.clone(),
            ..Element::default()
        });
    }
    elements.push(Element {
        slot: Slot::Title as i32,
        kind: ElementKind::Text as i32,
        template: first_set(&[&props.label_template, &props.label]).to_string(),
        color: props.fg_color.clone(),
        ..Element::default()
    });
    Face {
        layout: FaceLayout::IconLabel as i32,
        background: Some(Background {
            color: props.bg_color.trim().to_string(),
            ..Background::default()
        })
        .filter(|background| !background.color.is_empty()),
        elements,
        rules: Vec::new(),
        zones: Vec::new(),
    }
}

fn first_set<'a>(candidates: &[&'a String]) -> &'a str {
    candidates
        .iter()
        .map(|text| text.trim())
        .find(|text| !text.is_empty())
        .unwrap_or("")
}

/// The face a widget shows in `look` (none: its own).
pub fn face_for(widget: &Widget, look: Option<&StateLook>) -> Face {
    let Some(look) = look else {
        return widget.face.clone().unwrap_or_else(|| derive(widget.props.as_ref()));
    };
    if let Some(face) = &look.face {
        return face.clone();
    }
    match &widget.face {
        // Schema 3's look: its fields replace the props' where it has them,
        // but a live label still wins, as it always has.
        None => {
            let mut props = widget.props.clone().unwrap_or_default();
            let set = |field: &mut String, from: &String| {
                if !from.trim().is_empty() {
                    *field = from.clone();
                }
            };
            set(&mut props.label, &look.label);
            set(&mut props.icon, &look.icon);
            set(&mut props.bg_color, &look.bg_color);
            set(&mut props.fg_color, &look.fg_color);
            derive(Some(&props))
        }
        Some(face) => {
            let mut face = face.clone();
            if !look.label.trim().is_empty() {
                if let Some(title) = face.elements.iter_mut().find(|e| {
                    e.slot == Slot::Title as i32 && e.kind == ElementKind::Text as i32
                }) {
                    title.template = look.label.clone();
                }
            }
            if !look.icon.trim().is_empty() {
                if let Some(icon) = face.elements.iter_mut().find(|e| e.kind == ElementKind::Icon as i32) {
                    icon.template = look.icon.clone();
                }
            }
            if !look.bg_color.trim().is_empty() {
                face.background.get_or_insert_with(Background::default).color = look.bg_color.clone();
            }
            if !look.fg_color.trim().is_empty() {
                for element in &mut face.elements {
                    if matches!(kind_of(element), ElementKind::Text | ElementKind::Icon) {
                        element.color = look.fg_color.clone();
                    }
                }
            }
            face
        }
    }
}

fn kind_of(element: &Element) -> ElementKind {
    ElementKind::try_from(element.kind).unwrap_or(ElementKind::Text)
}

#[derive(Debug, Clone)]
struct RulePlan {
    when: Expression,
    color: String,
    pulse: bool,
}

impl PartialEq for RulePlan {
    fn eq(&self, other: &Self) -> bool {
        self.color == other.color && self.pulse == other.pulse && self.when == other.when
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ElementPlan {
    kind: ElementKind,
    template: Option<Template>,
    /// An icon to show while a live one is unknown.
    fallback: String,
    rules: Vec<RulePlan>,
    /// A graph's variable.
    graph: Option<String>,
    /// A timer element's timer.
    timer: Option<String>,
}

/// One face, parsed: what to fill and what to test.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FacePlan {
    elements: Vec<ElementPlan>,
    rules: Vec<RulePlan>,
    background_image: Option<Template>,
    background_graph: Option<String>,
}

fn parse_rules(rules: &[Rule], what: &str) -> Result<Vec<RulePlan>, String> {
    if rules.len() > MAX_RULES {
        return Err(format!("{what} has more than {MAX_RULES} rules"));
    }
    rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let when = parse_expression(&rule.when)
                .map_err(|error| format!("{what}, rule {}: {error}", index + 1))?;
            Ok(RulePlan {
                when,
                color: rule.color.trim().to_string(),
                pulse: rule.pulse,
            })
        })
        .collect()
}

fn element_name(index: usize, kind: ElementKind) -> String {
    let kind = match kind {
        ElementKind::Text => "text",
        ElementKind::Icon => "icon",
        ElementKind::Bar => "bar",
        ElementKind::Gauge => "gauge",
        ElementKind::Graph => "graph",
        ElementKind::Clock => "clock",
        ElementKind::Timer => "timer",
    };
    format!("its face's element {} ({kind})", index + 1)
}

impl FacePlan {
    /// Parse `face`. `icon_fallback` is the plain icon behind a derived live one.
    pub fn parse(face: &Face, icon_fallback: &str) -> Result<Self, String> {
        if face.elements.len() > MAX_ELEMENTS {
            return Err(format!("its face has more than {MAX_ELEMENTS} elements"));
        }
        let mut elements = Vec::with_capacity(face.elements.len());
        for (index, element) in face.elements.iter().enumerate() {
            let kind = ElementKind::try_from(element.kind)
                .map_err(|_| format!("its face's element {} is of a kind this agent does not know", index + 1))?;
            let what = element_name(index, kind);
            let template = match kind {
                ElementKind::Text | ElementKind::Icon | ElementKind::Bar | ElementKind::Gauge => Some(
                    Template::parse(&element.template).map_err(|error| format!("{what}: {error}"))?,
                ),
                ElementKind::Graph | ElementKind::Clock | ElementKind::Timer => None,
            };
            if matches!(kind, ElementKind::Bar | ElementKind::Gauge)
                && !template.as_ref().is_some_and(Template::is_live)
            {
                return Err(format!("{what} shows no value: give it one, like {{{{system.cpu.usage}}}}"));
            }
            let graph = (kind == ElementKind::Graph).then(|| element.variable.trim().to_string());
            if graph.as_deref() == Some("") {
                return Err(format!("{what} draws no value: pick the one it graphs"));
            }
            let timer = (kind == ElementKind::Timer).then(|| element.variable.trim().to_string());
            if let Some(name) = &timer {
                let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                if !ok {
                    return Err(format!("{what} counts down no timer: name one, like focus"));
                }
            }
            if let Some(b) = &element.r#box {
                let fits = [b.x, b.y, b.w, b.h].iter().all(|n| n.is_finite())
                    && b.x >= 0.0
                    && b.y >= 0.0
                    && b.w > 0.0
                    && b.h > 0.0
                    && b.x + b.w <= 1.0001
                    && b.y + b.h <= 1.0001;
                if !fits {
                    return Err(format!("{what} is placed partly off the key"));
                }
            }
            if element.min != 0.0 || element.max != 0.0 {
                if !(element.min.is_finite() && element.max.is_finite() && element.min < element.max) {
                    return Err(format!("{what} runs from {} to {}, which is not a range", element.min, element.max));
                }
            }
            elements.push(ElementPlan {
                kind,
                template,
                fallback: if kind == ElementKind::Icon { icon_fallback.to_string() } else { String::new() },
                rules: parse_rules(&element.rules, &what)?,
                graph,
                timer,
            });
        }
        let background = face.background.clone().unwrap_or_default();
        let background_image = match background.image.trim() {
            "" => None,
            image => Some(
                Template::parse(image).map_err(|error| format!("its face's background picture: {error}"))?,
            ),
        }
        .filter(Template::is_live);
        Ok(Self {
            elements,
            rules: parse_rules(&face.rules, "its face")?,
            background_image,
            background_graph: Some(background.graph.trim().to_string()).filter(|name| !name.is_empty()),
        })
    }

    /// Whether anything on it changes by itself.
    pub fn is_live(&self) -> bool {
        !self.rules.is_empty()
            || self.background_image.is_some()
            || self.background_graph.is_some()
            || self.elements.iter().any(|element| {
                !element.rules.is_empty()
                    || element.graph.is_some()
                    || element.timer.is_some()
                    || element.template.as_ref().is_some_and(Template::is_live)
            })
    }

    /// Every variable it reads, graphs included.
    pub fn variables(&self) -> BTreeSet<String> {
        let mut names = self.graphs();
        let rules = self.rules.iter().chain(self.elements.iter().flat_map(|e| e.rules.iter()));
        for rule in rules {
            names.extend(rule.when.variables());
        }
        for element in &self.elements {
            if let Some(template) = &element.template {
                names.extend(template.variables());
            }
            if let Some(timer) = &element.timer {
                names.extend(["running", "left", "ends"].map(|part| format!("timer.{timer}.{part}")));
            }
        }
        if let Some(image) = &self.background_image {
            names.extend(image.variables());
        }
        names
    }

    /// The variables whose history it draws.
    pub fn graphs(&self) -> BTreeSet<String> {
        self.elements
            .iter()
            .filter_map(|element| element.graph.clone())
            .chain(self.background_graph.clone())
            .collect()
    }

    /// What it shows now.
    pub fn values(&self, lookup: &dyn Lookup, locale: &Locale) -> FaceValues {
        let first_rule = |rules: &[RulePlan]| {
            rules
                .iter()
                .find(|rule| rule.when.eval(lookup, locale).truthy())
                .map(|rule| (rule.color.clone(), rule.pulse))
        };
        let elements = self
            .elements
            .iter()
            .enumerate()
            .map(|(index, element)| {
                let (color, pulse) = first_rule(&element.rules).unwrap_or_default();
                let mut value = ElementValue {
                    index: index as u32,
                    color,
                    pulse,
                    ..ElementValue::default()
                };
                if let Some(template) = &element.template {
                    match element.kind {
                        ElementKind::Text => value.text = template.render(lookup, locale),
                        ElementKind::Icon => {
                            value.text = match template.value_if_known(lookup, locale) {
                                Val::Text(text) if !text.trim().is_empty() => text,
                                _ => element.fallback.clone(),
                            }
                        }
                        ElementKind::Bar | ElementKind::Gauge => {
                            value.number = template.value(lookup, locale).as_number();
                        }
                        ElementKind::Graph | ElementKind::Clock | ElementKind::Timer => {}
                    }
                }
                if let Some(timer) = &element.timer {
                    // The seconds left now; a running one is counted down by the
                    // tablet from when this arrives, so no two clocks must agree.
                    let running = lookup.get(&format!("timer.{timer}.running")).truthy();
                    let number = |part: &str| lookup.get(&format!("timer.{timer}.{part}")).as_number();
                    let left = if running {
                        number("ends").map(|ends| ((ends - epoch_ms() as f64) / 1000.0).max(0.0))
                    } else {
                        number("left")
                    };
                    value.number = left;
                    value.text = if running { "run".to_string() } else { String::new() };
                }
                value
            })
            .collect();
        let (background, pulse) = first_rule(&self.rules).unwrap_or_default();
        FaceValues {
            elements,
            background,
            pulse,
            background_image: self
                .background_image
                .as_ref()
                .map(|image| match image.value_if_known(lookup, locale) {
                    Val::Text(text) => text,
                    _ => String::new(),
                })
                .unwrap_or_default(),
        }
    }
}

fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A face's live values, as a `StateEvent` carries them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FaceValues {
    pub elements: Vec<ElementValue>,
    pub background: String,
    pub pulse: bool,
    pub background_image: String,
}

/// Parse the face of every look of `widget`, the widget's own first (under
/// the empty id).
pub fn plan_faces(widget: &Widget) -> Result<Vec<(String, FacePlan)>, String> {
    let props = widget.props.clone().unwrap_or_default();
    let mut plans = vec![(
        String::new(),
        FacePlan::parse(&face_for(widget, None), props.icon.trim())?,
    )];
    for look in widget.states.iter().flat_map(|states| states.looks.iter()) {
        let fallback = first_set(&[&look.icon, &props.icon]);
        let face = face_for(widget, Some(look));
        let plan = FacePlan::parse(&face, fallback)
            .map_err(|problem| format!("in its look '{}', {problem}", look.id.trim()))?;
        plans.push((look.id.trim().to_string(), plan));
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_protocol::{TextStyle, WidgetStates};
    use flexdeck_vars::{Kind, VariableSpec};
    use serde_json::json;

    fn widget(value: serde_json::Value) -> Widget {
        serde_json::from_value(value).expect("a widget")
    }

    /// The fixture every renderer agrees on.
    fn fixture() -> serde_json::Value {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contract/fixtures/face/derive.json"
        ))
        .expect("contract/fixtures/face/derive.json");
        serde_json::from_str(&text).expect("the fixture is JSON")
    }

    #[test]
    fn every_derive_fixture_gives_its_face() {
        for case in fixture()["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let widget = widget(case["widget"].clone());
            let look_id = case["look"].as_str().unwrap_or("");
            let look = widget
                .states
                .iter()
                .flat_map(|states| states.looks.iter())
                .find(|look| look.id == look_id);
            let face = face_for(&widget, look);
            let expected: Face = serde_json::from_value(case["face"].clone())
                .unwrap_or_else(|error| panic!("{name}: the expected face: {error}"));
            assert_eq!(face, expected, "{name}");
        }
    }

    fn vars() -> VariableStore {
        let vars = VariableStore::new();
        vars.declare(VariableSpec::new("system.cpu.usage", Kind::Number, ""));
        vars.declare(VariableSpec::new("media.art", Kind::Text, ""));
        vars
    }

    #[test]
    fn a_face_fills_its_elements_and_its_rules_pick_colours() {
        let face: Face = serde_json::from_value(json!({
            "layout": "FACE_LAYOUT_DIAL",
            "background": {"image": "{{media.art}}", "graph": "system.cpu.usage"},
            "rules": [{"when": "system.cpu.usage > 80", "color": "#7f1d1d", "pulse": true}],
            "elements": [
                {"slot": "SLOT_VISUAL", "kind": "ELEMENT_KIND_GAUGE", "template": "{{system.cpu.usage}}",
                 "rules": [{"when": "system.cpu.usage > 50", "color": "#ef4444"}]},
                {"slot": "SLOT_VALUE", "template": "{{ system.cpu.usage | round }}%"},
                {"slot": "SLOT_CAPTION", "template": "CPU"},
                {"slot": "SLOT_ICON", "kind": "ELEMENT_KIND_ICON", "template": "{{media.art}}"}
            ]
        }))
        .unwrap();
        let plan = FacePlan::parse(&face, "music").unwrap();
        assert!(plan.is_live());
        assert_eq!(
            plan.variables().into_iter().collect::<Vec<_>>(),
            ["media.art", "system.cpu.usage"]
        );
        assert_eq!(plan.graphs().into_iter().collect::<Vec<_>>(), ["system.cpu.usage"]);

        let vars = vars();
        let locale = Locale::default();
        let unknown = plan.values(&VarLookup(&vars), &locale);
        assert_eq!(unknown.elements[0].number, None, "unknown: no number");
        assert_eq!(unknown.elements[1].text, "–%");
        assert_eq!(unknown.elements[3].text, "music", "the plain icon while the art is unknown");
        assert_eq!(unknown.background, "");

        vars.set("system.cpu.usage", Value::Number(91.4)).unwrap();
        let art = format!("img:{}", "ab".repeat(32));
        vars.set("media.art", Value::Text(art.clone())).unwrap();
        let hot = plan.values(&VarLookup(&vars), &locale);
        assert_eq!(hot.elements[0].number, Some(91.4));
        assert_eq!(hot.elements[0].color, "#ef4444");
        assert_eq!(hot.elements[1].text, "91%");
        assert_eq!(hot.elements[2].text, "CPU");
        assert_eq!(hot.elements[3].text, art);
        assert_eq!((hot.background.as_str(), hot.pulse), ("#7f1d1d", true));
        assert_eq!(hot.background_image, art);
    }

    #[test]
    fn an_element_put_anywhere_must_be_on_the_key() {
        let face = |b: serde_json::Value| -> Face { serde_json::from_value(json!({"elements": [{"template": "hi", "box": b}]})).unwrap() };
        FacePlan::parse(&face(json!({"x": 0.1, "y": 0.2, "w": 0.5, "h": 0.8})), "").unwrap();
        for off in [json!({"x": 0.6, "y": 0, "w": 0.5, "h": 0.5}), json!({"x": -0.1, "y": 0, "w": 0.5, "h": 0.5}), json!({"x": 0, "y": 0, "w": 0, "h": 0.5})] {
            assert!(FacePlan::parse(&face(off), "").unwrap_err().contains("off the key"));
        }
    }

    #[test]
    fn a_timer_element_sends_the_seconds_left_and_whether_it_runs() {
        let face: Face = serde_json::from_value(json!({
            "elements": [{"slot": "SLOT_VALUE", "kind": "ELEMENT_KIND_TIMER", "variable": "tea"}]
        }))
        .unwrap();
        let plan = FacePlan::parse(&face, "").unwrap();
        assert!(plan.is_live());
        assert!(plan.variables().contains("timer.tea.ends"));
        let vars = VariableStore::new();
        for (name, kind) in [("timer.tea.running", Kind::Bool), ("timer.tea.left", Kind::Number), ("timer.tea.ends", Kind::Number)] {
            vars.declare(VariableSpec::new(name, kind, ""));
        }
        let locale = Locale::default();
        vars.set("timer.tea.left", Value::Number(40.0)).unwrap();
        let paused = plan.values(&VarLookup(&vars), &locale);
        assert_eq!((paused.elements[0].number, paused.elements[0].text.as_str()), (Some(40.0), ""));

        vars.set("timer.tea.running", Value::Bool(true)).unwrap();
        vars.set("timer.tea.ends", Value::Number(epoch_ms() as f64 + 90_000.0)).unwrap();
        let running = plan.values(&VarLookup(&vars), &locale);
        let left = running.elements[0].number.unwrap();
        assert!((88.0..=90.0).contains(&left), "{left}");
        assert_eq!(running.elements[0].text, "run");

        let nameless: Face = serde_json::from_value(json!({"elements": [{"kind": "ELEMENT_KIND_TIMER"}]})).unwrap();
        assert!(FacePlan::parse(&nameless, "").unwrap_err().contains("timer"));
    }

    #[test]
    fn a_face_that_cannot_work_is_refused_naming_the_element() {
        let refused = |face: serde_json::Value| {
            FacePlan::parse(&serde_json::from_value(face).unwrap(), "").unwrap_err()
        };
        assert!(refused(json!({"elements": [{"template": "{{ x | rond }}"}]}))
            .contains("element 1 (text): 'rond' is not a filter"));
        assert!(refused(json!({"elements": [{"kind": "ELEMENT_KIND_GAUGE", "template": "50"}]}))
            .contains("shows no value"));
        assert!(refused(json!({"elements": [{"kind": "ELEMENT_KIND_GRAPH"}]})).contains("draws no value"));
        assert!(refused(json!({"elements": [{"kind": "ELEMENT_KIND_BAR", "template": "{{x}}", "min": 5, "max": 1}]}))
            .contains("not a range"));
        assert!(refused(json!({"rules": [{"when": "x >"}]})).contains("its face, rule 1"));
        let many: Vec<_> = (0..13).map(|_| json!({"template": "x"})).collect();
        assert!(refused(json!({"elements": many})).contains("more than"));
    }

    #[test]
    fn every_look_gets_its_own_face_plan() {
        let widget = Widget {
            id: "w".into(),
            props: Some(WidgetProps { label: "Mic".into(), icon: "mic".into(), ..Default::default() }),
            states: Some(WidgetStates {
                looks: vec![
                    StateLook { id: "muted".into(), label: "Muted".into(), icon: "mic-off".into(), ..Default::default() },
                    StateLook {
                        id: "live".into(),
                        face: Some(Face {
                            elements: vec![Element {
                                template: "{{ x | nope }}".into(),
                                text: Some(TextStyle::default()),
                                ..Default::default()
                            }],
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ],
                source: None,
                rules: vec![],
            }),
            ..Default::default()
        };
        let problem = plan_faces(&widget).unwrap_err();
        assert!(problem.starts_with("in its look 'live'"), "{problem}");
    }
}

//! The editing rules the MCP tools apply to a deck, as pure functions over its
//! JSON. They mirror the portal's (`editing.lib.ts`, `config.lib.ts`), and the
//! fixtures in `contract/fixtures/edit` hold the two to the same answers.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

pub const SHORT_PRESS: &str = "GESTURE_SHORT_PRESS";
pub const BUTTON: &str = "WIDGET_TYPE_BUTTON";
const FACES_SCHEMA: u64 = 4;
const GESTURE_STATES_SCHEMA: u64 = 5;
const CONDITION_STATES_SCHEMA: u64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min_w: i64,
    pub min_h: i64,
    pub max_w: i64,
    pub max_h: i64,
}

pub fn limits_for(kind: Option<&str>) -> Limits {
    match kind {
        Some("WIDGET_TYPE_BUTTON") => Limits { min_w: 1, min_h: 1, max_w: 4, max_h: 4 },
        _ => Limits { min_w: 1, min_h: 1, max_w: 8, max_h: 8 },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub id: String,
    pub col: i64,
    pub row: i64,
    pub w: i64,
    pub h: i64,
}

pub fn placement_problem(cols: i64, rows: i64, others: &[Span], candidate: &Span, limits: Limits) -> Option<&'static str> {
    if candidate.w < limits.min_w || candidate.h < limits.min_h {
        return Some("too_small");
    }
    if candidate.w > limits.max_w || candidate.h > limits.max_h {
        return Some("too_large");
    }
    if candidate.col < 0 || candidate.row < 0 {
        return Some("out_of_bounds");
    }
    if candidate.col + candidate.w > cols || candidate.row + candidate.h > rows {
        return Some("out_of_bounds");
    }
    if others.iter().any(|other| other.id != candidate.id && overlaps(other, candidate)) {
        return Some("overlap");
    }
    None
}

pub fn describe_problem(problem: &str) -> &'static str {
    match problem {
        "out_of_bounds" => "that position is outside the grid",
        "overlap" => "another widget is already there",
        "too_small" => "that is smaller than this widget allows",
        "too_large" => "that is larger than this widget allows",
        _ => "that placement is not allowed",
    }
}

fn overlaps(a: &Span, b: &Span) -> bool {
    a.col < b.col + b.w && b.col < a.col + a.w && a.row < b.row + b.h && b.row < a.row + a.h
}

pub fn first_free_cell(cols: i64, rows: i64, spans: &[Span], w: i64, h: i64) -> Option<(i64, i64)> {
    let mut row = 0;
    while row + h <= rows {
        let mut col = 0;
        while col + w <= cols {
            let candidate = Span { id: String::new(), col, row, w, h };
            if !spans.iter().any(|span| overlaps(span, &candidate)) {
                return Some((col, row));
            }
            col += 1;
        }
        row += 1;
    }
    None
}

fn int(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_f64).map(|n| n.trunc() as i64).unwrap_or(0)
}

pub fn grid_of(profile: &Value) -> (i64, i64) {
    let grid = profile.get("grid");
    (int(grid.and_then(|g| g.get("cols"))), int(grid.and_then(|g| g.get("rows"))))
}

pub fn span_of(widget: &Value) -> Span {
    Span {
        id: widget.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        col: int(widget.get("col")),
        row: int(widget.get("row")),
        w: int(widget.get("w")).max(1),
        h: int(widget.get("h")).max(1),
    }
}

pub fn spans_of(page: &Value) -> Vec<Span> {
    items(page.get("widgets")).iter().map(span_of).collect()
}

fn items(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

pub fn widgets(config: &Value) -> impl Iterator<Item = &Value> {
    items(config.get("profiles"))
        .iter()
        .flat_map(|profile| items(profile.get("pages")).iter())
        .flat_map(|page| items(page.get("widgets")).iter())
}

pub fn locate(config: &Value, widget_id: &str) -> Option<(usize, usize, usize)> {
    for (p, profile) in items(config.get("profiles")).iter().enumerate() {
        for (g, page) in items(profile.get("pages")).iter().enumerate() {
            if let Some(w) = items(page.get("widgets")).iter().position(|widget| text(widget, "id") == widget_id) {
                return Some((p, g, w));
            }
        }
    }
    None
}

pub fn locate_page(config: &Value, page_id: &str) -> Option<(usize, usize)> {
    for (p, profile) in items(config.get("profiles")).iter().enumerate() {
        if let Some(g) = items(profile.get("pages")).iter().position(|page| text(page, "id") == page_id) {
            return Some((p, g));
        }
    }
    None
}

fn page_mut(config: &mut Value, p: usize, g: usize) -> Option<&mut Value> {
    config.get_mut("profiles")?.get_mut(p)?.get_mut("pages")?.get_mut(g)
}

fn widgets_mut(config: &mut Value, p: usize, g: usize) -> Result<&mut Vec<Value>, String> {
    let page = page_mut(config, p, g).ok_or("no such page")?;
    let object = page.as_object_mut().ok_or("a page is not an object")?;
    object
        .entry("widgets")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "a page's widgets are not a list".to_string())
}

pub fn next_id(existing: &BTreeSet<String>, prefix: &str) -> String {
    (1..)
        .map(|n| format!("{prefix}-{n}"))
        .find(|candidate| !existing.contains(candidate))
        .unwrap_or_default()
}

pub fn widget_ids(config: &Value) -> BTreeSet<String> {
    widgets(config).map(|widget| text(widget, "id").to_string()).collect()
}

pub fn page_ids(config: &Value) -> BTreeSet<String> {
    items(config.get("profiles"))
        .iter()
        .flat_map(|profile| items(profile.get("pages")).iter())
        .map(|page| text(page, "id").to_string())
        .collect()
}

fn first_page(config: &Value) -> Option<(usize, usize)> {
    let profile = items(config.get("profiles")).first()?;
    items(profile.get("pages")).first().map(|_| (0, 0))
}

fn place(config: &Value, p: usize, g: usize, widget: &Value) -> Result<(), String> {
    let profile = &items(config.get("profiles"))[p];
    let (cols, rows) = grid_of(profile);
    let page = &items(profile.get("pages"))[g];
    let others: Vec<Span> = spans_of(page);
    let candidate = Span {
        id: text(widget, "id").to_string(),
        col: int(widget.get("col")),
        row: int(widget.get("row")),
        w: int(widget.get("w")),
        h: int(widget.get("h")),
    };
    match placement_problem(cols, rows, &others, &candidate, limits_for(widget.get("type").and_then(Value::as_str))) {
        Some(problem) => Err(format!(
            "{} (col {}, row {}, {}x{} on a {}x{} grid)",
            describe_problem(problem),
            candidate.col,
            candidate.row,
            candidate.w,
            candidate.h,
            cols,
            rows
        )),
        None => Ok(()),
    }
}

pub fn upsert(config: &mut Value, page_id: Option<&str>, mut widget: Value) -> Result<String, String> {
    let object = widget.as_object_mut().ok_or("widget must be a JSON object")?;
    let given_id = object.get("id").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
    if !object.contains_key("type") {
        object.insert("type".into(), json!(BUTTON));
    }
    let existing = (!given_id.is_empty()).then(|| locate(config, &given_id)).flatten();
    let target = match page_id {
        Some(page_id) => locate_page(config, page_id).ok_or_else(|| format!("no page \"{page_id}\""))?,
        None => match existing {
            Some((p, g, _)) => (p, g),
            None => first_page(config).ok_or("the deck has no page to add a widget to")?,
        },
    };

    let id = if given_id.is_empty() { next_id(&widget_ids(config), "widget") } else { given_id };
    let object = widget.as_object_mut().ok_or("widget must be a JSON object")?;
    object.insert("id".into(), json!(id));

    if let Some((p, g, w)) = existing {
        let old = items(config["profiles"][p]["pages"][g].get("widgets"))[w].clone();
        for key in ["col", "row", "w", "h"] {
            if !object.contains_key(key) {
                if let Some(value) = old.get(key) {
                    object.insert(key.into(), value.clone());
                }
            }
        }
        widgets_mut(config, p, g)?.remove(w);
    }

    let limits = limits_for(object.get("type").and_then(Value::as_str));
    for (key, min) in [("w", limits.min_w), ("h", limits.min_h)] {
        if !object.contains_key(key) {
            object.insert(key.into(), json!(min));
        }
    }
    if !object.contains_key("col") || !object.contains_key("row") {
        let (p, g) = target;
        let profile = &items(config.get("profiles"))[p];
        let (cols, rows) = grid_of(profile);
        let spans = spans_of(&items(profile.get("pages"))[g]);
        let (col, row) = first_free_cell(cols, rows, &spans, int(object.get("w")), int(object.get("h")))
            .ok_or("that page has no free space for a widget this size; pick another page or a smaller size")?;
        object.insert("col".into(), json!(col));
        object.insert("row".into(), json!(row));
    }

    let (p, g) = target;
    place(config, p, g, &widget)?;
    widgets_mut(config, p, g)?.push(widget);
    Ok(id)
}

pub fn move_resize(
    config: &mut Value,
    widget_id: &str,
    col: i64,
    row: i64,
    w: Option<i64>,
    h: Option<i64>,
    page_id: Option<&str>,
) -> Result<(), String> {
    let (p, g, i) = locate(config, widget_id).ok_or_else(|| format!("no widget \"{widget_id}\""))?;
    let mut widget = items(config["profiles"][p]["pages"][g].get("widgets"))[i].clone();
    let object = widget.as_object_mut().ok_or("a widget is not an object")?;
    object.insert("col".into(), json!(col));
    object.insert("row".into(), json!(row));
    if let Some(w) = w {
        object.insert("w".into(), json!(w));
    }
    if let Some(h) = h {
        object.insert("h".into(), json!(h));
    }
    widgets_mut(config, p, g)?.remove(i);
    let (tp, tg) = match page_id {
        Some(page_id) => locate_page(config, page_id).ok_or_else(|| format!("no page \"{page_id}\""))?,
        None => (p, g),
    };
    place(config, tp, tg, &widget)?;
    widgets_mut(config, tp, tg)?.push(widget);
    Ok(())
}

pub fn remove_widget(config: &mut Value, widget_id: &str) -> Result<(), String> {
    let (p, g, i) = locate(config, widget_id).ok_or_else(|| format!("no widget \"{widget_id}\""))?;
    widgets_mut(config, p, g)?.remove(i);
    Ok(())
}

pub fn add_page(config: &mut Value, profile_id: Option<&str>, page_id: Option<&str>, name: Option<&str>) -> Result<String, String> {
    let existing = page_ids(config);
    let id = match page_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) if existing.contains(id) => return Err(format!("a page \"{id}\" already exists")),
        Some(id) => id.to_string(),
        None => next_id(&existing, "page"),
    };
    let profiles = config.get_mut("profiles").and_then(Value::as_array_mut).ok_or("the deck has no profiles")?;
    let profile = match profile_id {
        Some(wanted) => profiles
            .iter_mut()
            .find(|profile| text(profile, "id") == wanted)
            .ok_or_else(|| format!("no profile \"{wanted}\""))?,
        None => profiles.first_mut().ok_or("the deck has no profiles")?,
    };
    let object = profile.as_object_mut().ok_or("a profile is not an object")?;
    let pages = object
        .entry("pages")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("a profile's pages are not a list")?;
    let mut page = Map::new();
    page.insert("id".into(), json!(id));
    if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        page.insert("name".into(), json!(name));
    }
    pages.push(Value::Object(page));
    Ok(id)
}

pub fn rename_page(config: &mut Value, page_id: &str, name: &str) -> Result<(), String> {
    let (p, g) = locate_page(config, page_id).ok_or_else(|| format!("no page \"{page_id}\""))?;
    let page = page_mut(config, p, g).and_then(Value::as_object_mut).ok_or("a page is not an object")?;
    match name.trim() {
        "" => {
            page.remove("name");
        }
        trimmed => {
            page.insert("name".into(), json!(trimmed));
        }
    }
    Ok(())
}

pub fn remove_page(config: &mut Value, page_id: &str) -> Result<(), String> {
    let (p, g) = locate_page(config, page_id).ok_or_else(|| format!("no page \"{page_id}\""))?;
    let pages = config["profiles"][p]
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .ok_or("a profile's pages are not a list")?;
    if pages.len() <= 1 {
        return Err("a profile keeps at least one page".into());
    }
    pages.remove(g);
    Ok(())
}

pub fn min_schema_for(config: &Value) -> u64 {
    let all: Vec<&Value> = widgets(config).collect();
    let conditions = all.iter().any(|widget| {
        let states = widget.get("states");
        items(states.and_then(|s| s.get("rules"))).iter().any(|rule| !text(rule, "when").trim().is_empty())
            || items(states.and_then(|s| s.get("variable")).and_then(|v| v.get("cases")))
                .iter()
                .any(|case| !text(case, "when").trim().is_empty())
    });
    if conditions {
        return CONDITION_STATES_SCHEMA;
    }
    let gestures = all.iter().any(|widget| {
        let states = widget.get("states");
        let props = widget.get("props");
        truthy(states.and_then(|s| s.get("gesture")))
            || !items(states.and_then(|s| s.get("rules"))).is_empty()
            || truthy(props.and_then(|p| p.get("hideDot")))
            || truthy(props.and_then(|p| p.get("noAutoLight")))
    });
    if gestures {
        GESTURE_STATES_SCHEMA
    } else {
        FACES_SCHEMA
    }
}

pub fn raise_schema(config: &mut Value) {
    let needed = min_schema_for(config);
    let current = config.get("schema").and_then(Value::as_u64).unwrap_or(0);
    if current < needed {
        if let Some(object) = config.as_object_mut() {
            object.insert("schema".into(), json!(needed));
        }
    }
}

pub fn document_problems(config: &Value) -> BTreeSet<String> {
    let mut problems = BTreeSet::new();
    let mut pages_seen = BTreeSet::new();
    let mut profiles_seen = BTreeSet::new();
    let mut widgets_seen = BTreeSet::new();
    for profile in items(config.get("profiles")) {
        let profile_id = text(profile, "id");
        if !profiles_seen.insert(profile_id.to_string()) {
            problems.insert(format!("two profiles are called \"{profile_id}\""));
        }
        let (cols, rows) = grid_of(profile);
        for page in items(profile.get("pages")) {
            let page_id = text(page, "id");
            if !pages_seen.insert(page_id.to_string()) {
                problems.insert(format!("two pages are called \"{page_id}\""));
            }
            let spans = spans_of(page);
            for widget in items(page.get("widgets")) {
                let span = span_of(widget);
                if !widgets_seen.insert(span.id.clone()) {
                    problems.insert(format!("two widgets are called \"{}\"", span.id));
                }
                let limits = limits_for(widget.get("type").and_then(Value::as_str));
                if let Some(problem) = placement_problem(cols, rows, &spans, &span, limits) {
                    problems.insert(format!("widget \"{}\" on page \"{page_id}\": {}", span.id, describe_problem(problem)));
                }
            }
        }
    }
    problems
}

pub fn new_problems(before: &Value, after: &Value) -> Vec<String> {
    let known = document_problems(before);
    document_problems(after).into_iter().filter(|problem| !known.contains(problem)).collect()
}

fn value_text(value: &Value) -> String {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(number) = value.get("number") {
        return number.to_string();
    }
    if let Some(flag) = value.get("bool") {
        return flag.to_string();
    }
    if value.get("input").is_some() {
        return String::new();
    }
    format!("{{{{{}}}}}", text(value, "variable"))
}

pub fn press_action(widget: &Value) -> Option<(String, String, std::collections::BTreeMap<String, String>)> {
    let flow = items(widget.get("flows"))
        .iter()
        .find(|flow| flow.get("trigger").and_then(|t| t.get("gesture")).and_then(Value::as_str) == Some(SHORT_PRESS))?;
    let block = items(flow.get("blocks")).iter().find(|block| text(block, "block") == "doAction")?;
    let args = block
        .get("args")
        .and_then(Value::as_object)
        .map(|args| args.iter().map(|(name, value)| (name.clone(), value_text(value))).collect())
        .unwrap_or_default();
    Some((text(block, "plugin").to_string(), text(block, "action").to_string(), args))
}

pub fn active_spec<'a>(catalogue: &'a Value, plugin: &str, action: &str) -> Option<&'a Value> {
    items(catalogue.get("plugins"))
        .iter()
        .filter(|entry| text(entry, "id") == plugin)
        .flat_map(|entry| items(entry.get("actions")).iter())
        .find(|entry| text(entry, "action") == action)?
        .get("active")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lit {
    Value { variable: String, value: String },
    When(String),
}

fn arg_value(args: &std::collections::BTreeMap<String, String>, name: &str) -> Option<String> {
    let value = args.get(name).map(|v| v.trim()).unwrap_or_default();
    (!value.is_empty() && !value.contains("{{")).then(|| value.to_string())
}

fn placeholder_at(chars: &[char], i: usize) -> Option<(String, usize)> {
    if chars.get(i) != Some(&'{') || chars.get(i + 1) != Some(&'{') {
        return None;
    }
    let mut j = i + 2;
    while chars.get(j).is_some_and(|c| c.is_whitespace()) {
        j += 1;
    }
    let start = j;
    while chars.get(j).is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}') {
        j += 1;
    }
    if j == start {
        return None;
    }
    let name: String = chars[start..j].iter().collect();
    while chars.get(j).is_some_and(|c| c.is_whitespace()) {
        j += 1;
    }
    (chars.get(j) == Some(&'}') && chars.get(j + 1) == Some(&'}')).then_some((name, j + 2 - i))
}

fn replace_placeholders(text: &str, mut fill: impl FnMut(&str) -> String) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some((name, len)) = placeholder_at(&chars, i) {
            out.push_str(&fill(&name));
            i += len;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '$')
}

const RESERVED: [&str; 5] = ["and", "or", "not", "true", "false"];

fn quote_name(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_alphabetic() || matches!(c, '_' | '@' | '$'))
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '$' | '.'));
    if plain && !RESERVED.contains(&name) && !name.ends_with('.') {
        name.to_string()
    } else {
        format!("`{name}`")
    }
}

fn literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn fill_placeholders(when: &str, args: &std::collections::BTreeMap<String, String>) -> Option<String> {
    let chars: Vec<char> = when.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            let quoted: String = chars[i..(j + 1).min(chars.len())].iter().collect();
            if quoted.contains("{{") {
                return None;
            }
            out.push_str(&quoted);
            i = j + 1;
        } else if c == '`' {
            let end = chars[i + 1..].iter().position(|ch| *ch == '`').map(|offset| i + 1 + offset)?;
            let inner: String = chars[i + 1..end].iter().collect();
            let mut failed = false;
            let name = replace_placeholders(&inner, |arg| match arg_value(args, arg) {
                Some(value) if !value.contains('`') => value,
                _ => {
                    failed = true;
                    String::new()
                }
            });
            if failed {
                return None;
            }
            out.push('`');
            out.push_str(&name);
            out.push('`');
            i = end + 1;
        } else if is_name_char(c) || (c == '{' && chars.get(i + 1) == Some(&'{')) {
            let mut name = String::new();
            let mut filled = false;
            let mut parts = 0;
            while i < chars.len() {
                if let Some((hole, len)) = placeholder_at(&chars, i) {
                    name.push_str(&arg_value(args, &hole)?);
                    filled = true;
                    parts += 1;
                    i += len;
                } else if is_name_char(chars[i]) {
                    name.push(chars[i]);
                    parts += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            if parts == 0 {
                out.push(c);
                i += 1;
            } else if !filled {
                out.push_str(&name);
            } else if parts == 1 {
                out.push_str(&literal(&name));
            } else if name.contains('`') {
                return None;
            } else {
                out.push_str(&quote_name(&name));
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    Some(out)
}

pub fn active_case(active: Option<&Value>, args: &std::collections::BTreeMap<String, String>) -> Option<Lit> {
    let active = active?;
    if let Some(when) = active.get("when").and_then(Value::as_str) {
        let when = fill_placeholders(when, args)?;
        return (!when.trim().is_empty()).then_some(Lit::When(when));
    }
    let variable = text(active, "variable");
    if variable.is_empty() {
        return None;
    }
    let mut missing = false;
    let variable = replace_placeholders(variable, |name| {
        let value = args.get(name).map(|v| v.trim().to_string()).unwrap_or_default();
        if value.is_empty() || value.contains("{{") {
            missing = true;
        }
        value
    });
    let value = match active.get("equals").and_then(Value::as_str) {
        Some(equals) => args.get(equals).map(|v| v.trim().to_string()).unwrap_or_default(),
        None => active.get("is").and_then(Value::as_str).unwrap_or("true").to_string(),
    };
    if missing || value.is_empty() || value.contains("{{") {
        return None;
    }
    Some(Lit::Value { variable: collapse_plugin_prefix(&variable), value })
}

fn collapse_plugin_prefix(variable: &str) -> String {
    let Some((first, rest)) = variable.split_once('.') else {
        return variable.to_string();
    };
    let shaped = first.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && first.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    match rest.strip_prefix(first).and_then(|tail| tail.strip_prefix('.')) {
        Some(tail) if shaped => format!("{first}.{tail}"),
        _ => variable.to_string(),
    }
}

fn has_looks(widget: &Value) -> bool {
    !items(widget.get("states").and_then(|s| s.get("looks"))).is_empty()
}

fn is_active_states(widget: &Value) -> bool {
    let Some(states) = widget.get("states") else {
        return false;
    };
    let looks = items(states.get("looks"));
    looks.len() == 2
        && text(&looks[0], "id") == "off"
        && text(&looks[1], "id") == "on"
        && looks
            .iter()
            .all(|look| ["bgColor", "fgColor", "label", "icon", "face"].iter().all(|key| !truthy(look.get(*key))))
        && items(states.get("rules")).is_empty()
        && items(states.get("variable").and_then(|v| v.get("cases"))).len() == 1
}

pub fn light_while_active(widget: &mut Value, catalogue: &Value) {
    let lit = press_action(widget).and_then(|(plugin, action, args)| active_case(active_spec(catalogue, &plugin, &action), &args));
    let ours = !has_looks(widget) || is_active_states(widget);
    let Some(object) = widget.as_object_mut() else { return };
    match lit {
        Some(lit) if ours => {
            if let Some(props) = object.get_mut("props").and_then(Value::as_object_mut) {
                props.remove("noAutoLight");
                if props.is_empty() {
                    object.remove("props");
                }
            }
            let variable = match lit {
                Lit::When(when) => json!({"variable": "", "cases": [{"when": when, "state": "on"}], "otherwise": "off"}),
                Lit::Value { variable, value } => {
                    json!({"variable": variable, "cases": [{"value": value, "state": "on"}], "otherwise": "off"})
                }
            };
            object.insert("states".into(), json!({"looks": [{"id": "off"}, {"id": "on"}], "variable": variable}));
        }
        Some(_) => {}
        None => {
            if is_active_states(&Value::Object(object.clone())) {
                object.remove("states");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck() -> Value {
        json!({"schema": 4, "rev": 3, "profiles": [{"id": "default", "grid": {"cols": 4, "rows": 3}, "pages": [
            {"id": "home", "widgets": [{"id": "widget-1", "type": BUTTON, "col": 0, "row": 0, "w": 2, "h": 1}]},
            {"id": "two", "widgets": [{"id": "widget-2", "type": BUTTON, "col": 0, "row": 0, "w": 1, "h": 1}]}
        ]}]})
    }

    #[test]
    fn a_new_widget_lands_in_the_first_free_cell_with_an_unused_id() {
        let mut config = deck();
        let id = upsert(&mut config, None, json!({"props": {"label": "Hi"}})).unwrap();
        assert_eq!(id, "widget-3");
        let added = &config["profiles"][0]["pages"][0]["widgets"][1];
        assert_eq!((added["col"].as_i64(), added["row"].as_i64(), added["w"].as_i64()), (Some(2), Some(0), Some(1)));
        assert_eq!(added["type"], BUTTON);
    }

    #[test]
    fn an_existing_widget_is_replaced_where_it_stands() {
        let mut config = deck();
        upsert(&mut config, None, json!({"id": "widget-2", "props": {"label": "New"}})).unwrap();
        let (p, g, _) = locate(&config, "widget-2").unwrap();
        assert_eq!((p, g), (0, 1));
        assert_eq!(config["profiles"][0]["pages"][1]["widgets"].as_array().unwrap().len(), 1);
        assert_eq!(config["profiles"][0]["pages"][1]["widgets"][0]["props"]["label"], "New");
    }

    #[test]
    fn placement_refusals_say_why() {
        let mut config = deck();
        let overlap = upsert(&mut config, Some("home"), json!({"col": 1, "row": 0})).unwrap_err();
        assert!(overlap.contains("another widget"), "{overlap}");
        let outside = upsert(&mut config, Some("home"), json!({"col": 3, "row": 0, "w": 2})).unwrap_err();
        assert!(outside.contains("outside"), "{outside}");
        let large = upsert(&mut config, Some("home"), json!({"col": 0, "row": 1, "w": 5, "h": 1})).unwrap_err();
        assert!(large.contains("larger"), "{large}");
        assert_eq!(config, deck(), "a refused edit leaves the deck alone");
    }

    #[test]
    fn a_full_page_is_reported_rather_than_overfilled() {
        let mut config = json!({"profiles": [{"id": "p", "grid": {"cols": 1, "rows": 1}, "pages": [{"id": "a", "widgets": [
            {"id": "x", "type": BUTTON, "col": 0, "row": 0, "w": 1, "h": 1}]}]}]});
        assert!(upsert(&mut config, None, json!({})).unwrap_err().contains("no free space"));
    }

    #[test]
    fn moving_checks_the_target_page() {
        let mut config = deck();
        move_resize(&mut config, "widget-2", 0, 1, None, None, Some("home")).unwrap();
        assert_eq!(locate(&config, "widget-2").map(|(_, g, _)| g), Some(0));
        assert!(move_resize(&mut config, "widget-2", 0, 0, None, None, None).is_err());
    }

    #[test]
    fn pages_keep_unique_ids_and_at_least_one_remains() {
        let mut config = deck();
        assert_eq!(add_page(&mut config, None, None, Some("Three")).unwrap(), "page-1");
        assert!(add_page(&mut config, None, Some("home"), None).is_err());
        rename_page(&mut config, "page-1", " ").unwrap();
        assert!(config["profiles"][0]["pages"][2].get("name").is_none());
        remove_page(&mut config, "page-1").unwrap();
        remove_page(&mut config, "two").unwrap();
        assert!(remove_page(&mut config, "home").is_err());
    }

    #[test]
    fn the_schema_follows_what_the_deck_holds_and_never_drops() {
        let mut config = json!({"schema": 2, "profiles": []});
        raise_schema(&mut config);
        assert_eq!(config["schema"], 4);
        let gesture = json!({"profiles": [{"pages": [{"widgets": [{"props": {"hideDot": true}}]}]}]});
        assert_eq!(min_schema_for(&gesture), 5);
        let when = json!({"profiles": [{"pages": [{"widgets": [{"states": {"variable": {"cases": [{"when": "x > 1"}]}}}]}]}]});
        assert_eq!(min_schema_for(&when), 6);
        let mut newer = json!({"schema": 6, "profiles": []});
        raise_schema(&mut newer);
        assert_eq!(newer["schema"], 6);
    }

    #[test]
    fn only_problems_an_edit_brings_are_reported() {
        let before = json!({"profiles": [{"id": "p", "grid": {"cols": 1, "rows": 1}, "pages": [{"id": "a", "widgets": [
            {"id": "x", "col": 3, "row": 0}]}]}]});
        let mut after = before.clone();
        assert!(new_problems(&before, &after).is_empty());
        after["profiles"][0]["pages"].as_array_mut().unwrap().push(json!({"id": "a"}));
        assert_eq!(new_problems(&before, &after), vec!["two pages are called \"a\"".to_string()]);
    }

    fn catalogue() -> Value {
        json!({"plugins": [{"id": "obs", "actions": [
            {"action": "scene", "active": {"variable": "obs.obs.scene", "equals": "scene"}},
            {"action": "record", "active": {"variable": "obs.recording"}},
            {"action": "mute", "active": {"when": "{{input}} == true"}}
        ]}]})
    }

    fn pressing(action: &str, args: Value) -> Value {
        json!({"id": "k", "flows": [{"trigger": {"gesture": SHORT_PRESS}, "blocks": [
            {"block": "doAction", "plugin": "obs", "action": action, "args": args}]}]})
    }

    #[test]
    fn a_key_whose_action_can_tell_is_lit_while_active() {
        let mut widget = pressing("scene", json!({"scene": {"text": "Live"}}));
        light_while_active(&mut widget, &catalogue());
        assert_eq!(widget["states"]["variable"], json!({"variable": "obs.scene", "cases": [{"value": "Live", "state": "on"}], "otherwise": "off"}));

        let mut record = pressing("record", json!({}));
        light_while_active(&mut record, &catalogue());
        assert_eq!(record["states"]["variable"]["cases"][0]["value"], "true");

        let mut mute = pressing("mute", json!({"input": {"text": "Mic"}}));
        light_while_active(&mut mute, &catalogue());
        assert_eq!(mute["states"]["variable"]["cases"][0]["when"], "\"Mic\" == true");
    }

    #[test]
    fn looks_set_by_hand_are_left_alone_and_a_blank_argument_lights_nothing() {
        let mut custom = pressing("scene", json!({"scene": {"text": "Live"}}));
        custom["states"] = json!({"looks": [{"id": "a", "bgColor": "#f00"}]});
        let before = custom.clone();
        light_while_active(&mut custom, &catalogue());
        assert_eq!(custom, before);

        let mut blank = pressing("scene", json!({"scene": {"text": " "}}));
        light_while_active(&mut blank, &catalogue());
        assert!(blank.get("states").is_none());
    }

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contract/fixtures/edit").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).expect("run pnpm gen:edit")).unwrap()
    }

    fn span(value: &Value) -> Span {
        Span {
            id: value["id"].as_str().unwrap().to_string(),
            col: value["col"].as_i64().unwrap(),
            row: value["row"].as_i64().unwrap(),
            w: value["w"].as_i64().unwrap(),
            h: value["h"].as_i64().unwrap(),
        }
    }

    #[test]
    fn placement_answers_match_the_editor() {
        let fixture = fixture("placement.json");
        let (cols, rows) = (fixture["grid"]["cols"].as_i64().unwrap(), fixture["grid"]["rows"].as_i64().unwrap());
        let page: Vec<Span> = fixture["page"].as_array().unwrap().iter().map(span).collect();
        for case in fixture["placements"].as_array().unwrap() {
            let problem = placement_problem(cols, rows, &page, &span(&case["span"]), limits_for(case["type"].as_str()));
            assert_eq!(problem, case["problem"].as_str(), "{case}");
        }
        for case in fixture["freeCells"].as_array().unwrap() {
            let cell = first_free_cell(cols, rows, &page, case["w"].as_i64().unwrap(), case["h"].as_i64().unwrap());
            let expected = case["cell"].as_object().map(|cell| (cell["col"].as_i64().unwrap(), cell["row"].as_i64().unwrap()));
            assert_eq!(cell, expected, "{case}");
        }
    }

    #[test]
    fn schema_answers_match_the_editor() {
        for case in fixture("schema.json").as_array().unwrap() {
            let mut deck = case["deck"].clone();
            raise_schema(&mut deck);
            assert_eq!(deck["schema"], case["schema"], "{}", case["name"]);
        }
    }

    #[test]
    fn lighting_answers_match_the_editor() {
        for case in fixture("active.json").as_array().unwrap() {
            let mut actions = json!({"action": "act"});
            if !case["active"].is_null() {
                actions["active"] = case["active"].clone();
            }
            let catalogue = json!({"plugins": [{"id": "obs", "actions": [actions]}]});
            let mut widget = case["widget"].clone();
            light_while_active(&mut widget, &catalogue);
            assert_eq!(widget, case["lit"], "{}", case["name"]);
        }
    }

    #[test]
    fn placeholders_in_a_condition_are_filled_and_quoted() {
        let args: std::collections::BTreeMap<String, String> =
            [("a".to_string(), "x y".to_string()), ("b".to_string(), "Mic".to_string())].into();
        assert_eq!(fill_placeholders("obs.{{b}}.muted", &args).as_deref(), Some("obs.Mic.muted"));
        assert_eq!(fill_placeholders("{{a}}.on", &args).as_deref(), Some("`x y.on`"));
        assert_eq!(fill_placeholders("'{{a}}'", &args), None);
        assert_eq!(fill_placeholders("{{missing}} > 1", &args), None);
        assert_eq!(fill_placeholders("`{{b}} level` > 1", &args).as_deref(), Some("`Mic level` > 1"));
    }
}

//! The tools a coding agent calls. Reads return what the portal's editor would
//! show; every write names the revision it was made from and is refused if the
//! deck moved since, so two editors never silently undo each other.

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use super::edit;
use crate::access::Authority;
use crate::agent::{Agent, AgentError};

const GUIDE: &str = include_str!("../../../../docs/mcp.md");
const SECRETS_IN_PORTAL: &str =
    "secret arguments (tokens, keys) are set in the FlexDeck portal, never through MCP; leave them out";

pub enum Failure {
    UnknownTool,
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Message(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Failure::Message(message.to_string())
    }
}

type Outcome = Result<Value, Failure>;

fn rev_arg() -> Value {
    json!({"type": "integer", "minimum": 0, "description": "The deck revision you last read (deck_overview, get_widget or the last write returned it)."})
}

fn widget_schema() -> Value {
    json!({
        "type": "object",
        "description": "A widget as the deck stores it. Call get_guide for the full shape and an example.",
        "properties": {
            "id": {"type": "string", "description": "Omit to add a new widget; give an existing id to replace that widget."},
            "type": {"type": "string", "enum": ["WIDGET_TYPE_BUTTON", "WIDGET_TYPE_SLIDER"]},
            "col": {"type": "integer", "minimum": 0},
            "row": {"type": "integer", "minimum": 0},
            "w": {"type": "integer", "minimum": 1},
            "h": {"type": "integer", "minimum": 1},
            "props": {"type": "object", "properties": {
                "label": {"type": "string"}, "icon": {"type": "string"},
                "bgColor": {"type": "string"}, "fgColor": {"type": "string"},
                "labelTemplate": {"type": "string"}, "iconTemplate": {"type": "string"},
                "hideDot": {"type": "boolean"}, "noAutoLight": {"type": "boolean"}
            }},
            "flows": {"type": "array", "items": {"type": "object", "properties": {
                "trigger": {"type": "object", "description": "{\"gesture\": \"GESTURE_SHORT_PRESS\"} and the like."},
                "name": {"type": "string"},
                "blocks": {"type": "array", "items": {"type": "object"}}
            }}},
            "states": {"type": "object"},
            "slider": {"type": "object"},
            "face": {"type": "object"},
            "sources": {"type": "array", "items": {"type": "object", "properties": {
                "source": {"type": "string"}, "args": {"type": "object", "additionalProperties": {"type": "string"}}
            }}}
        }
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": properties, "required": required, "additionalProperties": false},
        "annotations": {"readOnlyHint": read_only, "destructiveHint": !read_only && name.starts_with("remove"), "openWorldHint": false}
    })
}

pub fn list() -> Vec<Value> {
    vec![
        tool("deck_overview", "The user's live FlexDeck deck at a glance (their widgets, keys, buttons and pages): its revision, and every profile's grid and pages with each widget's id, place, size, label and press action. Call this first.", json!({}), &[], true),
        tool("get_deck", "Part of the deck's JSON, by JSON Pointer (\"/profiles/0/pages/1\"); the whole deck without a path. For writing patch_deck operations.", json!({"path": {"type": "string"}}), &[], true),
        tool("get_widget", "One widget as stored, where it is, and why it cannot work right now if a plugin it uses is missing.", json!({"widgetId": {"type": "string"}}), &["widgetId"], true),
        tool("get_guide", "How a deck is built: widget shape, flows, blocks, arguments, looks and states, with a complete example. Read before building a widget.", json!({}), &[], true),
        tool("list_actions", "The actions plugins offer, with their arguments, for doAction blocks.", json!({"plugin": {"type": "string", "description": "Only this plugin's actions."}}), &[], true),
        tool("list_choices", "The values an action's arguments can take right now (scenes, devices, apps), as the portal's pickers show them.", json!({
            "plugin": {"type": "string"}, "action": {"type": "string"},
            "args": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Arguments chosen so far, for choices that depend on them."}
        }), &["plugin", "action"], true),
        tool("list_variables", "Variables widgets can show or react to, with their current values. Search with query rather than listing everything: a plugin can have hundreds.", json!({
            "query": {"type": "string", "description": "Words that must all appear in the name or description, in any case: \"preview scene\"."},
            "prefix": {"type": "string", "description": "Only names starting with this: \"obs.\"."},
            "limit": {"type": "integer", "minimum": 1, "maximum": VARIABLES_MAX, "description": "At most this many (default 50); matched says how many there were."}
        }), &[], true),
        tool("preview_widget", "How a widget would look right now, without saving it.", json!({"widget": widget_schema(), "look": {"type": "string", "description": "A look id from states.looks; the current one if omitted."}}), &["widget"], true),
        tool("upsert_widget", "Add a widget, or replace the one with the same id. Without col/row it takes the first free cell on the page. A press action that can tell when it is active also lights the key while it is, unless lit is \"none\".", json!({
            "widget": widget_schema(),
            "pageId": {"type": "string", "description": "Page to put it on. Default: where it already is, else the first page."},
            "rev": rev_arg(),
            "lit": {"type": "string", "enum": ["auto", "none"]}
        }), &["widget", "rev"], false),
        tool("move_widget", "Move or resize a widget, on its page or onto another.", json!({
            "widgetId": {"type": "string"}, "col": {"type": "integer", "minimum": 0}, "row": {"type": "integer", "minimum": 0},
            "w": {"type": "integer", "minimum": 1}, "h": {"type": "integer", "minimum": 1}, "pageId": {"type": "string"}, "rev": rev_arg()
        }), &["widgetId", "col", "row", "rev"], false),
        tool("remove_widget", "Remove a widget. undo brings it back.", json!({"widgetId": {"type": "string"}, "rev": rev_arg()}), &["widgetId", "rev"], false),
        tool("add_page", "Add an empty page to a profile (the first by default).", json!({
            "profileId": {"type": "string"}, "pageId": {"type": "string"}, "name": {"type": "string"}, "rev": rev_arg()
        }), &["rev"], false),
        tool("rename_page", "Rename a page; an empty name clears it.", json!({"pageId": {"type": "string"}, "name": {"type": "string"}, "rev": rev_arg()}), &["pageId", "name", "rev"], false),
        tool("remove_page", "Remove a page and its widgets. A profile keeps at least one page.", json!({"pageId": {"type": "string"}, "rev": rev_arg()}), &["pageId", "rev"], false),
        tool("patch_deck", "Apply RFC 6902 JSON Patch operations to the deck, for edits the other tools do not cover. Checked like any other write.", json!({
            "patch": {"type": "array", "items": {"type": "object"}}, "rev": rev_arg()
        }), &["patch", "rev"], false),
        tool("undo", "Step the deck back one saved change.", json!({"rev": rev_arg()}), &["rev"], false),
        tool("redo", "Step the deck forward again after undo.", json!({"rev": rev_arg()}), &["rev"], false),
        tool("get_automations", "The automations document and its own revision, automationsRev.", json!({}), &[], true),
        tool("put_automations", "Replace the automations document.", json!({
            "document": {"type": "object"},
            "automationsRev": {"type": "integer", "minimum": 0, "description": "The automationsRev you last read."}
        }), &["document", "automationsRev"], false),
        tool("dry_run_automation", "Run an automation against a scratch copy and return what it would do, without doing it.", json!({"automationId": {"type": "string"}}), &["automationId"], true),
    ]
}

pub async fn call(agent: &Agent, authority: &Authority, name: &str, args: &Value) -> Outcome {
    match name {
        "deck_overview" => overview(agent),
        "get_deck" => get_deck(agent, args),
        "get_widget" => get_widget(agent, args),
        "get_guide" => Ok(Value::String(GUIDE.to_string())),
        "list_actions" => Ok(list_actions(agent, opt_str(args, "plugin"))),
        "list_choices" => list_choices(agent, args).await,
        "list_variables" => list_variables(agent, args),
        "preview_widget" => {
            let widget = args.get("widget").ok_or("widget is required")?;
            Ok(crate::http::preview_one(agent, widget, opt_str(args, "look").unwrap_or_default()))
        }
        "upsert_widget" => upsert_widget(agent, authority, args).await,
        "move_widget" => {
            let widget_id = req_str(args, "widgetId")?.to_string();
            let (col, row) = (req_int(args, "col")?, req_int(args, "row")?);
            let (w, h) = (opt_int(args, "w")?, opt_int(args, "h")?);
            let page = opt_str(args, "pageId").map(str::to_string);
            write_deck(agent, authority, args, |deck| {
                edit::move_resize(deck, &widget_id, col, row, w, h, page.as_deref())?;
                Ok(Some(widget_id.clone()))
            })
            .await
        }
        "remove_widget" => {
            let widget_id = req_str(args, "widgetId")?.to_string();
            write_deck(agent, authority, args, |deck| edit::remove_widget(deck, &widget_id).map(|()| None)).await
        }
        "add_page" => {
            let (profile, page, page_name) =
                (opt_str(args, "profileId"), opt_str(args, "pageId"), opt_str(args, "name"));
            let outcome = write_with(agent, authority, args, |deck| edit::add_page(deck, profile, page, page_name)).await?;
            Ok(outcome)
        }
        "rename_page" => {
            let (page, page_name) = (req_str(args, "pageId")?, req_str(args, "name")?);
            write_deck(agent, authority, args, |deck| edit::rename_page(deck, page, page_name).map(|()| None)).await
        }
        "remove_page" => {
            let page = req_str(args, "pageId")?;
            write_deck(agent, authority, args, |deck| edit::remove_page(deck, page).map(|()| None)).await
        }
        "patch_deck" => {
            let patch: json_patch::Patch = serde_json::from_value(args.get("patch").cloned().unwrap_or(Value::Null))
                .map_err(|error| format!("patch is not a JSON Patch: {error}"))?;
            write_deck(agent, authority, args, |deck| {
                json_patch::patch(deck, &patch).map_err(|error| error.to_string())?;
                Ok(None)
            })
            .await
        }
        "undo" => step(agent, authority, args, false),
        "redo" => step(agent, authority, args, true),
        "get_automations" => {
            let document = agent.behavior().document_json();
            let rev = agent.behavior().automations_rev().map_err(|error| error.to_string())?;
            Ok(json!({"automationsRev": rev, "document": document}))
        }
        "put_automations" => put_automations(agent, authority, args).await,
        "dry_run_automation" => {
            let run = agent
                .behavior()
                .dry_run(req_str(args, "automationId")?)
                .map_err(|error| error.to_string())?;
            Ok(serde_json::to_value(run).map_err(|error| error.to_string())?)
        }
        _ => Err(Failure::UnknownTool),
    }
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(args, key).ok_or_else(|| format!("{key} is required and must be a string"))
}

fn opt_int(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_i64().map(Some).ok_or_else(|| format!("{key} must be a whole number")),
    }
}

fn req_int(args: &Value, key: &str) -> Result<i64, String> {
    opt_int(args, key)?.ok_or_else(|| format!("{key} is required"))
}

fn req_rev(args: &Value, key: &str) -> Result<u64, String> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{key} is required: the revision you last read"))
}

fn deck(agent: &Agent) -> Result<(u64, Value), String> {
    match agent.config().map_err(|error| error.to_string())? {
        Some((rev, json)) => Ok((rev, serde_json::from_str(&json).map_err(|error| error.to_string())?)),
        None => Ok((0, Value::Null)),
    }
}

fn conflict(current: u64) -> String {
    format!("the deck changed (now rev {current}); call deck_overview and retry with that rev")
}

fn describe(error: AgentError) -> String {
    match error {
        AgentError::Conflict { current, .. } => conflict(current),
        other => other.to_string(),
    }
}

fn label_of(widget: &Value) -> Value {
    widget.get("props").and_then(|props| props.get("label")).cloned().unwrap_or(Value::Null)
}

fn overview(agent: &Agent) -> Outcome {
    let (rev, deck) = deck(agent)?;
    let profiles: Vec<Value> = deck
        .get("profiles")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|profile| {
            let (cols, rows) = edit::grid_of(profile);
            let pages: Vec<Value> = profile
                .get("pages")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|page| {
                    let widgets: Vec<Value> = page
                        .get("widgets")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                        .iter()
                        .map(|widget| {
                            let span = edit::span_of(widget);
                            let mut entry = json!({
                                "id": span.id, "type": widget.get("type").cloned().unwrap_or(Value::Null),
                                "col": span.col, "row": span.row, "w": span.w, "h": span.h,
                                "label": label_of(widget),
                            });
                            if let Some(icon) = widget.get("props").and_then(|props| props.get("icon")) {
                                entry["icon"] = icon.clone();
                            }
                            if let Some((plugin, action, _)) = edit::press_action(widget) {
                                entry["action"] = json!(format!("{plugin}:{action}"));
                            }
                            entry
                        })
                        .collect();
                    json!({"id": page.get("id"), "name": page.get("name"), "widgets": widgets})
                })
                .collect();
            json!({"id": profile.get("id"), "name": profile.get("name"), "grid": {"cols": cols, "rows": rows}, "pages": pages})
        })
        .collect();
    Ok(json!({"rev": rev, "schema": deck.get("schema"), "profiles": profiles}))
}

fn get_deck(agent: &Agent, args: &Value) -> Outcome {
    let (rev, deck) = deck(agent)?;
    let path = opt_str(args, "path").unwrap_or_default();
    let value = deck.pointer(path).cloned().ok_or_else(|| format!("nothing at {path}"))?;
    Ok(json!({"rev": rev, "path": path, "value": value}))
}

fn get_widget(agent: &Agent, args: &Value) -> Outcome {
    let widget_id = req_str(args, "widgetId")?;
    let (rev, deck) = deck(agent)?;
    let (p, g, w) = edit::locate(&deck, widget_id).ok_or_else(|| format!("no widget \"{widget_id}\""))?;
    let profile = &deck["profiles"][p];
    let page = &profile["pages"][g];
    let mut answer = json!({"rev": rev, "profileId": profile.get("id"), "pageId": page.get("id"), "widget": page["widgets"][w]});
    if let Some(reason) = unavailable(agent, &deck, widget_id) {
        answer["unavailable"] = json!(reason);
    }
    Ok(answer)
}

fn unavailable(agent: &Agent, deck: &Value, widget_id: &str) -> Option<String> {
    let json = serde_json::to_string(deck).ok()?;
    let plans = crate::widgets::parse(&json, &flexdeck_automation::Limits::default()).ok()?;
    let plan = plans.iter().find(|plan| plan.widget_id == widget_id)?;
    Some(agent.behavior().unavailable_for(plan)).filter(|reason| !reason.is_empty())
}

fn list_actions(agent: &Agent, plugin: Option<&str>) -> Value {
    let mut catalogue = agent.behavior().plugin_catalogue();
    if let (Some(plugin), Some(plugins)) = (plugin, catalogue.get_mut("plugins").and_then(Value::as_array_mut)) {
        plugins.retain(|entry| entry.get("id").and_then(Value::as_str) == Some(plugin));
    }
    catalogue
}

const VARIABLES_SHOWN: usize = 50;
const VARIABLES_MAX: usize = 500;

fn list_variables(agent: &Agent, args: &Value) -> Outcome {
    let prefix = opt_str(args, "prefix").unwrap_or_default();
    let words: Vec<String> = opt_str(args, "query").unwrap_or_default().split_whitespace().map(str::to_lowercase).collect();
    let limit = match opt_int(args, "limit")? {
        Some(limit) => usize::try_from(limit).unwrap_or(0).clamp(1, VARIABLES_MAX),
        None => VARIABLES_SHOWN,
    };
    let Value::Array(entries) = agent.behavior().variables_catalogue() else { return Ok(json!({"variables": [], "matched": 0})) };
    let found: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            let field = |key: &str| entry.get("spec").and_then(|spec| spec.get(key)).and_then(Value::as_str).unwrap_or_default();
            let name = field("name");
            let text = format!("{} {}", name, field("description")).to_lowercase();
            name.starts_with(prefix) && words.iter().all(|word| text.contains(word.as_str()))
        })
        .collect();
    let matched = found.len();
    Ok(json!({"variables": found.into_iter().take(limit).collect::<Vec<_>>(), "matched": matched}))
}

async fn list_choices(agent: &Agent, args: &Value) -> Outcome {
    let plugin = req_str(args, "plugin")?.to_string();
    let action = req_str(args, "action")?.to_string();
    let chosen: HashMap<String, String> = args
        .get("args")
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(name, value)| value.as_str().map(|value| (name.clone(), value.to_string())))
                .collect()
        })
        .unwrap_or_default();
    match crate::http::choices_for(agent, plugin, action, chosen).await {
        Ok(Ok(choices)) => Ok(choices),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => Err(error.message().to_string().into()),
    }
}

async fn upsert_widget(agent: &Agent, authority: &Authority, args: &Value) -> Outcome {
    let mut widget = args.get("widget").cloned().ok_or("widget is required")?;
    if !widget.is_object() {
        return Err("widget must be a JSON object".into());
    }
    if opt_str(args, "lit").unwrap_or("auto") != "none" {
        edit::light_while_active(&mut widget, &agent.behavior().plugin_catalogue());
    }
    let page = opt_str(args, "pageId");
    write_deck(agent, authority, args, |deck| edit::upsert(deck, page, widget.clone()).map(Some)).await
}

async fn write_with(
    agent: &Agent,
    authority: &Authority,
    args: &Value,
    apply: impl FnOnce(&mut Value) -> Result<String, String>,
) -> Outcome {
    let mut page_id = None;
    let mut answer = write_deck(agent, authority, args, |deck| {
        page_id = Some(apply(deck)?);
        Ok(None)
    })
    .await?;
    answer["pageId"] = json!(page_id);
    Ok(answer)
}

async fn write_deck(
    agent: &Agent,
    authority: &Authority,
    args: &Value,
    apply: impl FnOnce(&mut Value) -> Result<Option<String>, String>,
) -> Outcome {
    let rev = req_rev(args, "rev")?;
    let (current_rev, current) = deck(agent)?;
    if rev != current_rev {
        return Err(conflict(current_rev).into());
    }
    let mut next = current.clone();
    let widget_id = apply(&mut next)?;
    check(agent, &current, &mut next)?;
    crate::launches::approve(agent, authority, &current, &next).await?;
    let committed = agent
        .edit_config(Some(rev), |document| {
            *document = next;
            Ok(())
        })
        .map_err(describe)?;
    let mut answer = json!({"rev": committed});
    if let Some(widget_id) = widget_id {
        let (_, saved) = deck(agent)?;
        if let Some(reason) = unavailable(agent, &saved, &widget_id) {
            answer["unavailable"] = json!(reason);
        }
        answer["widgetId"] = json!(widget_id);
    }
    Ok(answer)
}

fn check(agent: &Agent, before: &Value, after: &mut Value) -> Result<(), String> {
    if !after.is_object() {
        return Err("the deck must stay a JSON object".into());
    }
    let problems = edit::new_problems(before, after);
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    if holds_secret(agent, after) {
        return Err(SECRETS_IN_PORTAL.into());
    }
    edit::raise_schema(after);
    let old: BTreeMap<&str, &Value> =
        edit::widgets(before).map(|widget| (widget.get("id").and_then(Value::as_str).unwrap_or_default(), widget)).collect();
    for widget in edit::widgets(after) {
        let id = widget.get("id").and_then(Value::as_str).unwrap_or_default();
        if old.get(id) != Some(&widget) {
            crate::widgets::check_widget(widget).map_err(|error| format!("widget \"{id}\": {error}"))?;
        }
    }
    Ok(())
}

fn holds_secret(agent: &Agent, value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|item| holds_secret(agent, item)),
        Value::Object(object) => {
            let in_sources = object.get("sources").and_then(Value::as_array).is_some_and(|uses| {
                uses.iter().any(|used| {
                    let (Some(source), Some(args)) =
                        (used.get("source").and_then(Value::as_str), used.get("args").and_then(Value::as_object))
                    else {
                        return false;
                    };
                    agent.behavior().sources().secret_args(source).iter().any(|name| args.contains_key(name))
                })
            });
            in_sources || object.values().any(|item| holds_secret(agent, item))
        }
        _ => false,
    }
}

fn step(agent: &Agent, authority: &Authority, args: &Value, forward: bool) -> Outcome {
    let rev = req_rev(args, "rev")?;
    if authority.needs_launch_approval() {
        if let Some((head, current, target)) = agent.step_preview(forward).map_err(describe)? {
            if head != rev {
                return Err(conflict(head).into());
            }
            let current: Value = serde_json::from_str(&current).map_err(|error| error.to_string())?;
            let target: Value = serde_json::from_str(&target).map_err(|error| error.to_string())?;
            if !crate::launches::new_launches(&current, &target, &agent.behavior().sources().doc()).is_empty() {
                return Err(crate::http::STEP_RESTORES_LAUNCH.into());
            }
        }
    }
    let result = if forward { agent.redo_if(Some(rev)) } else { agent.undo_if(Some(rev)) };
    Ok(json!({"rev": result.map_err(describe)?}))
}

async fn put_automations(agent: &Agent, authority: &Authority, args: &Value) -> Outcome {
    let expected = req_rev(args, "automationsRev")?;
    let document = args.get("document").filter(|document| document.is_object()).ok_or("document must be a JSON object")?;
    let behavior = agent.behavior();
    let current_rev = behavior.automations_rev().map_err(|error| error.to_string())?;
    if expected != current_rev {
        return Err(format!("the automations changed (now automationsRev {current_rev}); call get_automations and retry").into());
    }
    if holds_secret(agent, document) {
        return Err(SECRETS_IN_PORTAL.into());
    }
    crate::launches::approve(agent, authority, &behavior.document_json(), document).await?;
    let json = serde_json::to_string(document).map_err(|error| error.to_string())?;
    match behavior.replace_document_json_if(Some(expected), &json) {
        Ok(rev) => Ok(json!({"automationsRev": rev})),
        Err(crate::behavior::BehaviorError::Conflict { current, .. }) => Err(format!(
            "the automations changed (now automationsRev {current}); call get_automations and retry"
        )
        .into()),
        Err(error) => Err(error.to_string().into()),
    }
}

//! What a deck or automation edit would let this computer start.
//!
//! The launcher plugin runs any program, and a key's own run of a command
//! source runs that source's program with the key's arguments. An automation
//! that presses keys or types on its own can open a program just as well. Any
//! of them, added by someone not sitting at this computer, is code execution here, so
//! the routes that take such edits ask the person at the desktop first. This
//! module only finds them: [`new_launches`] lists what `next` would start that
//! `current` does not already start exactly as written.

use std::collections::{BTreeMap, BTreeSet};

use flexdeck_actions::CORE_PLUGIN;
use flexdeck_plugin::builtin::launcher::LAUNCHER_PLUGIN;
use serde_json::Value;

use crate::access::{Authority, APPROVAL_TIMEOUT};
use crate::agent::Agent;
use crate::sources::SourcesDoc;

pub const LAUNCH_ACTION: &str = "Let the deck start programs";

pub const DYNAMIC_LAUNCH: &str =
    "a program launch whose program or arguments are read from a variable at run time can only be added at the desktop";

const KEYSTROKES: [&str; 2] = ["hotkey", "text"];

struct Launch {
    key: String,
    line: String,
    dynamic: bool,
    unattended: bool,
}

pub async fn approve(agent: &Agent, authority: &Authority, current: &Value, next: &Value) -> Result<(), String> {
    if !authority.needs_launch_approval() {
        return Ok(());
    }
    let added = added(current, next, &agent.behavior().sources().doc());
    if added.is_empty() {
        return Ok(());
    }
    if added.iter().any(|launch| launch.dynamic) {
        return Err(DYNAMIC_LAUNCH.to_string());
    }
    let lines: Vec<String> = added.into_iter().map(|launch| launch.line).collect();
    agent
        .approvals()
        .ask(LAUNCH_ACTION, &lines.join("\n"), authority.requester(), APPROVAL_TIMEOUT)
        .await
        .map_err(|error| error.to_string())
}

pub fn new_launches(current: &Value, next: &Value, sources: &SourcesDoc) -> Vec<String> {
    added(current, next, sources).into_iter().map(|launch| launch.line).collect()
}

fn added(current: &Value, next: &Value, sources: &SourcesDoc) -> Vec<Launch> {
    let mut known: BTreeMap<String, bool> = BTreeMap::new();
    for launch in launches(current, sources) {
        *known.entry(launch.key).or_default() |= launch.unattended;
    }
    let mut seen = BTreeSet::new();
    launches(next, sources)
        .into_iter()
        .filter(|launch| {
            let new = match known.get(&launch.key) {
                None => true,
                Some(unattended) => launch.unattended && !unattended,
            };
            new && seen.insert((launch.key.clone(), launch.unattended))
        })
        .collect()
}

fn launches(document: &Value, sources: &SourcesDoc) -> Vec<Launch> {
    let mut found = Vec::new();
    walk(document, sources, false, &mut found);
    found
}

fn walk(value: &Value, sources: &SourcesDoc, unattended: bool, found: &mut Vec<Launch>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| walk(item, sources, unattended, found)),
        Value::Object(object) => {
            let unattended = unattended || starts_on_its_own(object);
            if object.get("block").and_then(Value::as_str) == Some("doAction") {
                let plugin = object.get("plugin").and_then(Value::as_str).unwrap_or_default();
                let action = object.get("action").and_then(Value::as_str).unwrap_or_default();
                let args = object.get("args").cloned().unwrap_or(Value::Null);
                let key = canonical(&serde_json::json!({ "plugin": plugin, "action": action, "args": args }));
                if plugin == LAUNCHER_PLUGIN {
                    found.push(Launch { key, line: describe_action(action, &args), dynamic: is_dynamic(&args), unattended });
                } else if unattended && plugin == CORE_PLUGIN && KEYSTROKES.contains(&action) {
                    let line = format!("press on its own: {}", describe_args(&args)).trim_end().to_string();
                    found.push(Launch { key, line, dynamic: false, unattended: true });
                }
            }
            if let Some(Value::Array(uses)) = object.get("sources") {
                for used in uses {
                    let Some(id) = used.get("source").and_then(Value::as_str) else { continue };
                    let Some(source) = sources.sources.iter().find(|source| source.id == id && source.runs_program()) else {
                        continue;
                    };
                    let args = used.get("args").cloned().unwrap_or(Value::Null);
                    let key = canonical(&serde_json::json!({ "source": id, "args": args }));
                    let command = source.command_line().unwrap_or_else(|| id.to_string());
                    let line = format!("run on a schedule: {command} {}", describe_args(&args)).trim_end().to_string();
                    found.push(Launch { key, line, dynamic: false, unattended: true });
                }
            }
            for (name, item) in object {
                walk(item, sources, unattended || name == "automations", found);
            }
        }
        _ => {}
    }
}

fn starts_on_its_own(flow: &serde_json::Map<String, Value>) -> bool {
    flow.contains_key("blocks") && flow.get("trigger").is_some_and(|trigger| trigger.get("gesture").is_none())
}

fn is_dynamic(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("{{"),
        Value::Array(items) => items.iter().any(is_dynamic),
        Value::Object(object) => {
            object.contains_key("variable") || object.contains_key("input") || object.values().any(is_dynamic)
        }
        _ => false,
    }
}

fn describe_action(action: &str, args: &Value) -> String {
    let arg = |name: &str| args.get(name).map(describe_value).unwrap_or_default();
    match action {
        "run" => format!("run: {} {}", arg("program"), arg("arguments")).trim_end().to_string(),
        "open" => format!("open: {}", arg("target")),
        "open_url" => format!("open: {}", arg("url")),
        "open_app" => format!("open: {}", arg("app")),
        "close_app" => format!("close: {}", arg("app")),
        other => format!("{other}: {}", describe_args(args)).trim_end().to_string(),
    }
}

fn describe_args(args: &Value) -> String {
    match args {
        Value::Object(object) => object
            .iter()
            .map(|(name, value)| format!("{name}={}", describe_value(value)))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn describe_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(object) if object.len() == 1 && object.get("text").is_some_and(Value::is_string) => {
            object["text"].as_str().unwrap_or_default().to_string()
        }
        other => canonical(other),
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let inner: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{}:{}", Value::String(key.clone()), canonical(value)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(canonical).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deck(blocks: Value) -> Value {
        json!({"profiles": [{"pages": [{"widgets": [{"id": "w1", "flows": [{"trigger": {"gesture": "GESTURE_SHORT_PRESS"}, "blocks": blocks}]}]}]}]})
    }

    fn run(program: &str) -> Value {
        json!({"block": "doAction", "plugin": "launcher", "action": "run", "args": {"program": {"text": program}, "arguments": {"text": "-x"}}})
    }

    fn command_sources() -> SourcesDoc {
        serde_json::from_value(json!({"sources": [
            {"id": "ping", "kind": "command", "command": {"program": "ping", "args": ["{{args.host}}"]}},
            {"id": "weather", "kind": "http", "http": {"url": "https://example.com"}}
        ]}))
        .unwrap()
    }

    #[test]
    fn a_new_launcher_action_is_listed_and_an_unchanged_one_is_not() {
        let none = SourcesDoc::default();
        let before = deck(json!([run("notepad")]));
        assert!(new_launches(&before, &before, &none).is_empty());
        let after = deck(json!([run("notepad"), run("calc")]));
        assert_eq!(new_launches(&before, &after, &none), vec!["run: calc -x".to_string()]);
    }

    #[test]
    fn changed_arguments_count_as_a_new_launch() {
        let none = SourcesDoc::default();
        let before = deck(json!([run("notepad")]));
        let mut after = before.clone();
        after["profiles"][0]["pages"][0]["widgets"][0]["flows"][0]["blocks"][0]["args"]["arguments"] = json!({"text": "-y"});
        assert_eq!(new_launches(&before, &after, &none), vec!["run: notepad -y".to_string()]);
    }

    #[test]
    fn launches_nested_in_branches_and_automations_are_found() {
        let none = SourcesDoc::default();
        let empty = json!({"automations": []});
        let next = json!({"automations": [{"id": "a", "body": [{"block": "if", "then": [run("cmd")], "otherwise": []}]}]});
        assert_eq!(new_launches(&empty, &next, &none), vec!["run: cmd -x".to_string()]);
    }

    #[test]
    fn other_plugins_are_not_launches() {
        let none = SourcesDoc::default();
        let next = deck(json!([{"block": "doAction", "plugin": "timer", "action": "toggle", "args": {}}]));
        assert!(new_launches(&deck(json!([])), &next, &none).is_empty());
    }

    #[test]
    fn a_key_running_a_command_source_with_new_arguments_is_a_launch() {
        let sources = command_sources();
        let with = |args: Value| json!({"profiles": [{"pages": [{"widgets": [{"id": "w1", "sources": [{"source": "ping", "args": args}]}]}]}]});
        let web = json!({"profiles": [{"pages": [{"widgets": [{"id": "w1", "sources": [{"source": "weather", "args": {"city": "x"}}]}]}]}]});
        assert!(new_launches(&json!({}), &web, &sources).is_empty());
        let before = with(json!({"host": "a"}));
        assert!(new_launches(&before, &before, &sources).is_empty());
        let lines = new_launches(&before, &with(json!({"host": "b"})), &sources);
        assert_eq!(lines, vec!["run on a schedule: ping {{args.host}} host=b".to_string()]);
    }

    #[test]
    fn keystrokes_count_in_automations_but_not_on_keys() {
        let none = SourcesDoc::default();
        let press = json!({"block": "doAction", "plugin": "core", "action": "hotkey", "args": {"keys": {"text": "win+r"}}});
        let typing = json!({"block": "doAction", "plugin": "core", "action": "text", "args": {"text": {"text": "cmd"}}});
        let next = json!({"automations": [{"id": "a", "trigger": {"on": "schedule"}, "body": [press, typing]}]});
        assert_eq!(
            new_launches(&json!({"automations": []}), &next, &none),
            vec!["press on its own: keys=win+r".to_string(), "press on its own: text=cmd".to_string()]
        );
        assert!(new_launches(&next, &next, &none).is_empty());
        assert!(new_launches(&deck(json!([])), &deck(json!([press, typing])), &none).is_empty());
    }

    #[test]
    fn keystrokes_in_a_flow_started_by_an_event_count() {
        let none = SourcesDoc::default();
        let press = json!({"block": "doAction", "plugin": "core", "action": "hotkey", "args": {"keys": {"text": "win+r"}}});
        let flow = |trigger: Value| {
            json!({"profiles": [{"pages": [{"widgets": [{"id": "k", "flows": [{"trigger": trigger, "blocks": [press]}]}]}]}]})
        };
        let empty = flow(json!({"gesture": "GESTURE_SHORT_PRESS"}));
        assert!(new_launches(&json!({}), &empty, &none).is_empty());
        let event = flow(json!({"event": {"plugin": "timer", "event": "finished"}}));
        assert_eq!(new_launches(&empty, &event, &none), vec!["press on its own: keys=win+r".to_string()]);
        assert_eq!(new_launches(&empty, &flow(json!({"value": {}})), &none).len(), 1);
    }

    #[test]
    fn a_pressed_launch_moved_to_run_on_its_own_is_new() {
        let none = SourcesDoc::default();
        let flow = |trigger: Value| {
            json!({"profiles": [{"pages": [{"widgets": [{"id": "k", "flows": [{"trigger": trigger, "blocks": [run("calc")]}]}]}]}]})
        };
        let pressed = flow(json!({"gesture": "GESTURE_SHORT_PRESS"}));
        let on_its_own = flow(json!({"event": {"plugin": "timer", "event": "finished"}}));
        assert_eq!(new_launches(&pressed, &on_its_own, &none), vec!["run: calc -x".to_string()]);
        assert!(new_launches(&on_its_own, &pressed, &none).is_empty());
        assert!(new_launches(&on_its_own, &on_its_own, &none).is_empty());
    }

    #[test]
    fn a_launch_read_from_a_variable_is_dynamic() {
        let none = SourcesDoc::default();
        let launch = |program: Value| deck(json!([{"block": "doAction", "plugin": "launcher", "action": "run", "args": {"program": program}}]));
        let dynamic = |program: Value| added(&deck(json!([])), &launch(program), &none).iter().any(|found| found.dynamic);
        assert!(!dynamic(json!({"text": "calc"})));
        assert!(!dynamic(json!("calc")));
        assert!(dynamic(json!({"variable": "vars.p"})));
        assert!(dynamic(json!({"input": "value"})));
        assert!(dynamic(json!({"text": "{{vars.p}}"})));
        assert!(dynamic(json!("{{vars.p}}")));
        assert!(dynamic(json!([{"text": "a"}, {"variable": "vars.p"}])));
        assert!(!dynamic(json!({"number": 5})));
        assert!(!dynamic(json!({"bool": true})));
    }
}

//! The `vars` plugin: values of the person's own.
//!
//! `vars.mode`, `vars.count`, `vars.city` — made by setting them, read by any
//! label or rule as `{{vars.count}}`, kept by the agent across restarts. A
//! "Remember a value" step makes them too; these actions are the same thing
//! with the arithmetic and the toggling done for you.

use std::collections::HashMap;

use flexdeck_actions::{ActionSpec, ArgumentSpec};
use flexdeck_vars::{is_user_name, Value, VariableSpec, USER_PREFIX};

use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const VARS_PLUGIN: &str = "vars";

static VARS_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: VARS_PLUGIN,
        action: "set",
        description: "Set a value of your own",
        arguments: &[
            ArgumentSpec::text("name", "Its name, after vars.", "mode"),
            ArgumentSpec::text("value", "A number, on/off, or text", "Gaming"),
        ],
        slider: None,
    },
    ActionSpec {
        plugin: VARS_PLUGIN,
        action: "add",
        description: "Add to a number of your own (take away with a minus)",
        arguments: &[
            ArgumentSpec::text("name", "Its name, after vars.", "count"),
            ArgumentSpec::number("by", "How much", "1"),
        ],
        slider: None,
    },
    ActionSpec {
        plugin: VARS_PLUGIN,
        action: "toggle",
        description: "Turn an on/off value of your own the other way",
        arguments: &[ArgumentSpec::text("name", "Its name, after vars.", "focus")],
        slider: None,
    },
];

static VARS_MANIFEST: PluginManifest = PluginManifest {
    id: VARS_PLUGIN,
    name: "Your values",
    version: "1.0.0",
    description: "Values of your own, kept across restarts: {{vars.count}}.",
    actions: VARS_ACTIONS,
    settings: &[],
};

#[derive(Debug, Default)]
pub struct Vars;

impl Vars {
    pub fn new() -> Self {
        Self
    }
}

/// `count` or `vars.count` → `vars.count`, refusing a name that is not one.
fn full_name(raw: &str) -> Result<String, PluginError> {
    let raw = raw.trim();
    let name = if raw.starts_with(USER_PREFIX) { raw.to_string() } else { format!("{USER_PREFIX}{raw}") };
    if is_user_name(&name) {
        Ok(name)
    } else {
        Err(PluginError::Failed(format!(
            "'{raw}' cannot be a name: use letters, digits, - and _, with dots between parts"
        )))
    }
}

/// Text as the value it reads as: a number, on/off (true/false), or text.
pub fn parse_value(text: &str) -> Value {
    let trimmed = text.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "on" => return Value::Bool(true),
        "false" | "off" => return Value::Bool(false),
        _ => {}
    }
    match trimmed.parse::<f64>() {
        Ok(n) if n.is_finite() && !trimmed.is_empty() => Value::Number(n),
        _ => Value::Text(text.to_string()),
    }
}

impl Plugin for Vars {
    fn manifest(&self) -> crate::abi::ManifestDto {
        VARS_MANIFEST.to_dto()
    }

    /// None up front: they are the person's, declared as they are made.
    fn variables(&self) -> Vec<VariableSpec> {
        Vec::new()
    }

    fn sample(&mut self, _wanted: &[&str], _ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        let name = full_name(argument(args, "name"))?;
        let next = match action {
            "set" => parse_value(argument(args, "value")),
            "add" => {
                let by = argument(args, "by")
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| PluginError::Failed(format!("'{}' is not a number", argument(args, "by"))))?;
                match ctx.vars.get(&name) {
                    None => Value::Number(by),
                    Some(Value::Number(n)) => Value::Number(n + by),
                    Some(_) => return Err(PluginError::Failed(format!("{name} is not a number"))),
                }
            }
            "toggle" => match ctx.vars.get(&name) {
                None => Value::Bool(true),
                Some(Value::Bool(b)) => Value::Bool(!b),
                Some(_) => return Err(PluginError::Failed(format!("{name} is not on/off"))),
            },
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: VARS_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        };
        ctx.vars.set(&name, next)?;
        Ok(Outcome::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    fn run(vars: &VariableStore, action: &str, pairs: &[(&str, &str)]) -> Result<(), PluginError> {
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx { input: &mut input, vars, notifier: &mut notifier, settings: &settings, now_ms: 0 };
        let args = pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
        Vars::new().invoke(action, &args, &mut ctx).map(drop)
    }

    #[test]
    fn values_are_set_counted_and_toggled() {
        let vars = VariableStore::new();
        run(&vars, "set", &[("name", "mode"), ("value", "Gaming")]).unwrap();
        assert_eq!(vars.get("vars.mode"), Some(Value::Text("Gaming".into())));
        run(&vars, "add", &[("name", "vars.count"), ("by", "2")]).unwrap();
        run(&vars, "add", &[("name", "count"), ("by", "-0.5")]).unwrap();
        assert_eq!(vars.get("vars.count"), Some(Value::Number(1.5)));
        run(&vars, "toggle", &[("name", "focus")]).unwrap();
        run(&vars, "toggle", &[("name", "focus")]).unwrap();
        assert_eq!(vars.get("vars.focus"), Some(Value::Bool(false)));
        assert!(run(&vars, "toggle", &[("name", "mode")]).is_err(), "text has no other way");
        assert!(run(&vars, "set", &[("name", "a b"), ("value", "1")]).is_err());
    }

    #[test]
    fn text_reads_as_the_value_it_looks_like() {
        assert_eq!(parse_value("42"), Value::Number(42.0));
        assert_eq!(parse_value("On"), Value::Bool(true));
        assert_eq!(parse_value("off"), Value::Bool(false));
        assert_eq!(parse_value("Berlin"), Value::Text("Berlin".into()));
        assert_eq!(parse_value(""), Value::Text(String::new()));
    }
}

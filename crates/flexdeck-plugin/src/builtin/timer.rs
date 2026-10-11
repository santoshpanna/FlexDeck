//! The `timer` plugin: countdowns that live on the agent.
//!
//! A timer is a handful of variables — `timer.tea.running`, `.left`, `.ends`,
//! `.length`, `.done` — so a key's face, a rule or an automation reads it like
//! anything else. The agent's own tick ([`finish_due`]) ends a timer whose time
//! is up and announces `timer/finished` with its name, which any flow or
//! automation can be started by: a pomodoro's break, a tea's chime.
//!
//! The tablet never asks for the time: a timer element is sent the seconds
//! left when something changes, and counts down from when that arrived. Two
//! clocks never have to agree.

use std::collections::HashMap;

use flexdeck_actions::{ActionSpec, ArgumentSpec};
use flexdeck_vars::{Kind, Value, VariableSpec, VariableStore};

use crate::{abi, argument, Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const TIMER_PLUGIN: &str = "timer";
/// What it announces when a timer's time is up; the payload's `name` says which.
pub const FINISHED: &str = "finished";

const NAME: ArgumentSpec = ArgumentSpec::text("name", "Which timer: any name, like tea or focus", "focus");

static TIMER_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: TIMER_PLUGIN,
        action: "start",
        description: "Start a timer from the top",
        arguments: &[NAME, ArgumentSpec::number("minutes", "How long, in minutes", "25")],
        slider: None,
    },
    ActionSpec {
        plugin: TIMER_PLUGIN,
        action: "toggle",
        description: "Start a timer, or pause it, or carry on — one key for all three",
        arguments: &[NAME, ArgumentSpec::number("minutes", "How long, in minutes, when it starts", "25")],
        slider: None,
    },
    ActionSpec {
        plugin: TIMER_PLUGIN,
        action: "pause",
        description: "Pause a timer",
        arguments: &[NAME],
        slider: None,
    },
    ActionSpec {
        plugin: TIMER_PLUGIN,
        action: "resume",
        description: "Carry on with a paused timer",
        arguments: &[NAME],
        slider: None,
    },
    ActionSpec {
        plugin: TIMER_PLUGIN,
        action: "reset",
        description: "Stop a timer and put it back to its full length",
        arguments: &[NAME],
        slider: None,
    },
    ActionSpec {
        plugin: TIMER_PLUGIN,
        action: "add",
        description: "Give a timer more time (take some away with a minus)",
        arguments: &[NAME, ArgumentSpec::number("minutes", "How many minutes", "5")],
        slider: None,
    },
];

static TIMER_MANIFEST: PluginManifest = PluginManifest {
    id: TIMER_PLUGIN,
    name: "Timers",
    version: "1.0.0",
    description: "Countdowns: a pomodoro, a tea, a break. {{timer.focus.left}} seconds to go.",
    actions: TIMER_ACTIONS,
    settings: &[],
};

/// Each variable of one timer: suffix, kind, description.
const PARTS: &[(&str, Kind, &str)] = &[
    ("running", Kind::Bool, "Whether it is counting down"),
    ("left", Kind::Number, "Seconds left when it last started, paused or ended"),
    ("ends", Kind::Number, "When it ends, in milliseconds since 1970; 0 while stopped"),
    ("length", Kind::Number, "Its full length, in seconds"),
    ("done", Kind::Bool, "Its time is up (until it is started or reset)"),
];

#[derive(Debug, Default)]
pub struct Timers;

impl Timers {
    pub fn new() -> Self {
        Self
    }
}

/// A timer's name, checked: letters, digits, - and _.
fn timer_name(raw: &str) -> Result<String, PluginError> {
    let name = raw.trim();
    let ok = !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(name.to_string())
    } else {
        Err(PluginError::Failed(format!("'{raw}' cannot be a timer's name: use letters, digits, - and _")))
    }
}

fn minutes(args: &HashMap<String, String>) -> Result<f64, PluginError> {
    let raw = argument(args, "minutes");
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && n.abs() <= 24.0 * 60.0 * 7.0)
        .ok_or_else(|| PluginError::Failed(format!("'{raw}' is not a number of minutes")))
}

/// One timer's variables, read and written.
struct Timer<'a> {
    vars: &'a VariableStore,
    base: String,
}

impl<'a> Timer<'a> {
    fn new(vars: &'a VariableStore, name: &str) -> Self {
        let base = format!("{TIMER_PLUGIN}.{name}");
        for (part, kind, description) in PARTS {
            let full = format!("{base}.{part}");
            if vars.spec(&full).is_none() {
                vars.declare(VariableSpec::new(&full, *kind, &format!("Timer {name}: {}", description.to_lowercase())));
            }
        }
        Self { vars, base }
    }

    fn number(&self, part: &str) -> f64 {
        match self.vars.get(&format!("{}.{part}", self.base)) {
            Some(Value::Number(n)) => n,
            _ => 0.0,
        }
    }

    fn running(&self) -> bool {
        self.vars.get(&format!("{}.running", self.base)) == Some(Value::Bool(true))
    }

    fn put(&self, part: &str, value: Value) -> Result<(), PluginError> {
        self.vars.refresh(&format!("{}.{part}", self.base), value)?;
        Ok(())
    }

    /// Seconds left now.
    fn left_at(&self, now_ms: i64) -> f64 {
        if self.running() {
            ((self.number("ends") - now_ms as f64) / 1000.0).max(0.0)
        } else {
            self.number("left")
        }
    }

    fn run_from(&self, left: f64, now_ms: i64) -> Result<(), PluginError> {
        self.put("left", Value::Number(left))?;
        self.put("ends", Value::Number(now_ms as f64 + left * 1000.0))?;
        self.put("done", Value::Bool(false))?;
        self.put("running", Value::Bool(true))
    }

    fn stop_at(&self, left: f64) -> Result<(), PluginError> {
        self.put("left", Value::Number(left))?;
        self.put("ends", Value::Number(0.0))?;
        self.put("running", Value::Bool(false))
    }

    fn start(&self, seconds: f64, now_ms: i64) -> Result<(), PluginError> {
        if seconds <= 0.0 {
            return Err(PluginError::Failed("a timer needs some time: more than 0 minutes".into()));
        }
        self.put("length", Value::Number(seconds))?;
        self.run_from(seconds, now_ms)
    }
}

/// End every running timer whose time is up — marking it done — and say
/// which, for the agent to announce. Called on the agent's tick.
pub fn finish_due(vars: &VariableStore, now_ms: i64) -> Vec<String> {
    let prefix = format!("{TIMER_PLUGIN}.");
    let mut finished = Vec::new();
    for spec in vars.specs() {
        let Some(name) = spec.name.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(".running")) else { continue };
        let timer = Timer::new(vars, name);
        if timer.running() && timer.number("ends") <= now_ms as f64 {
            if timer.stop_at(0.0).is_ok() && timer.put("done", Value::Bool(true)).is_ok() {
                finished.push(name.to_string());
            }
        }
    }
    finished.sort();
    finished
}

impl Plugin for Timers {
    fn manifest(&self) -> abi::ManifestDto {
        let mut manifest = TIMER_MANIFEST.to_dto();
        manifest.events = vec![abi::EventDto {
            name: FINISHED.to_string(),
            description: "A timer's time is up".to_string(),
            fields: vec!["name".to_string()],
        }];
        manifest
    }

    /// None up front: each timer's are declared when it is first used.
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
        let name = timer_name(argument(args, "name"))?;
        let timer = Timer::new(ctx.vars, &name);
        let now = ctx.now_ms;
        match action {
            "start" => timer.start(minutes(args)? * 60.0, now)?,
            "toggle" => {
                let left = timer.left_at(now);
                if timer.running() {
                    timer.stop_at(left)?;
                } else if left > 0.0 {
                    timer.run_from(left, now)?;
                } else {
                    timer.start(minutes(args)? * 60.0, now)?;
                }
            }
            "pause" => {
                if timer.running() {
                    timer.stop_at(timer.left_at(now))?;
                }
            }
            "resume" => {
                let left = timer.left_at(now);
                if !timer.running() && left > 0.0 {
                    timer.run_from(left, now)?;
                }
            }
            "reset" => {
                timer.stop_at(timer.number("length"))?;
                timer.put("done", Value::Bool(false))?;
            }
            "add" => {
                let left = (timer.left_at(now) + minutes(args)? * 60.0).max(0.0);
                if timer.running() {
                    timer.run_from(left, now)?;
                } else {
                    timer.stop_at(left)?;
                }
            }
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: TIMER_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        }
        Ok(Outcome::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use std::collections::BTreeMap;

    fn run(timers: &mut Timers, vars: &VariableStore, now_ms: i64, action: &str, args: &[(&str, &str)]) -> Result<Outcome, PluginError> {
        let mut input = RecordingExecutor::default();
        let mut notifier = RecordingNotifier::default();
        let settings = BTreeMap::new();
        let mut ctx = Ctx { input: &mut input, vars, notifier: &mut notifier, settings: &settings, now_ms };
        let args: HashMap<String, String> = args.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        timers.invoke(action, &args, &mut ctx)
    }

    #[test]
    fn a_timer_counts_down_pauses_carries_on_and_finishes_once() {
        let vars = VariableStore::new();
        let mut timers = Timers::new();
        run(&mut timers, &vars, 0, "start", &[("name", "tea"), ("minutes", "1")]).unwrap();
        assert_eq!(vars.get("timer.tea.running"), Some(Value::Bool(true)));
        assert_eq!(vars.get("timer.tea.ends"), Some(Value::Number(60_000.0)));

        // Paused 20 s in: 40 left, and it stays 40 however long it waits.
        run(&mut timers, &vars, 20_000, "pause", &[("name", "tea")]).unwrap();
        assert_eq!(vars.get("timer.tea.left"), Some(Value::Number(40.0)));
        assert!(finish_due(&vars, 500_000).is_empty());

        // Carried on at 100 s: ends 40 s later.
        run(&mut timers, &vars, 100_000, "toggle", &[("name", "tea"), ("minutes", "1")]).unwrap();
        assert_eq!(vars.get("timer.tea.ends"), Some(Value::Number(140_000.0)));
        assert!(finish_due(&vars, 139_000).is_empty());
        assert_eq!(finish_due(&vars, 140_000), vec!["tea".to_string()]);
        assert_eq!(vars.get("timer.tea.done"), Some(Value::Bool(true)));
        assert!(finish_due(&vars, 141_000).is_empty(), "announced once");

        // Done, a toggle starts it afresh; a reset puts it back to its length.
        run(&mut timers, &vars, 200_000, "toggle", &[("name", "tea"), ("minutes", "2")]).unwrap();
        assert_eq!(vars.get("timer.tea.length"), Some(Value::Number(120.0)));
        run(&mut timers, &vars, 210_000, "add", &[("name", "tea"), ("minutes", "1")]).unwrap();
        assert_eq!(vars.get("timer.tea.ends"), Some(Value::Number(380_000.0)));
        run(&mut timers, &vars, 220_000, "reset", &[("name", "tea")]).unwrap();
        assert_eq!(vars.get("timer.tea.running"), Some(Value::Bool(false)));
        assert_eq!(vars.get("timer.tea.left"), Some(Value::Number(120.0)));
    }

    #[test]
    fn a_bad_name_or_length_is_refused() {
        let vars = VariableStore::new();
        let mut timers = Timers::new();
        assert!(run(&mut timers, &vars, 0, "start", &[("name", "tea time"), ("minutes", "1")]).is_err());
        assert!(run(&mut timers, &vars, 0, "start", &[("name", "tea"), ("minutes", "soon")]).is_err());
        assert!(run(&mut timers, &vars, 0, "start", &[("name", "tea"), ("minutes", "0")]).is_err());
    }
}

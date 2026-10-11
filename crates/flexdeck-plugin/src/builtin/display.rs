//! The `display` plugin: screen brightness.
//!
//! A laptop's own panel and an external monitor are dimmed in entirely
//! different ways on Windows — the first through WMI, the second by talking
//! DDC/CI down the video cable — so [`DisplayBackend`] hides which is which and
//! names every screen the same way. `display_windows.rs` is the live backend;
//! [`RecordingDisplay`] and [`UnavailableDisplay`] are the usual pair.
//!
//! # Which screen
//!
//! `monitor` is optional everywhere. Blank (or `all`) means every screen that
//! can be dimmed, which is what "brightness up" on a keyboard means; a name
//! picks one, matched forgivingly like an audio device.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use flexdeck_actions::{ActionSpec, ArgumentSpec, SliderSpec};
use flexdeck_vars::{Kind, Value, VariableSpec};

use super::audio::{pick, Flow};
use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest, SettingSpec};

pub const DISPLAY_PLUGIN: &str = "display";

/// The first screen's brightness — the one a laptop user means.
pub const BRIGHTNESS: &str = "display.brightness";
/// `display.monitor.<name>.brightness`: one screen.
pub const MONITOR_PREFIX: &str = "display.monitor.";

const MONITOR_ARGUMENT: ArgumentSpec = ArgumentSpec::text(
    "monitor",
    "Which screen, by name; blank for all of them",
    "",
)
.optional();

static DISPLAY_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: DISPLAY_PLUGIN,
        action: "set_brightness",
        description: "Set the screen brightness",
        arguments: &[
            MONITOR_ARGUMENT,
            ArgumentSpec::number("percent", "Brightness as a percentage, 0 to 100", "70"),
        ],
        slider: Some(SliderSpec::percent(
            "percent",
            "display.monitor.{{monitor}}.brightness",
        )),
    },
    ActionSpec {
        plugin: DISPLAY_PLUGIN,
        action: "brightness_up",
        description: "Turn the screen brightness up",
        arguments: &[
            MONITOR_ARGUMENT,
            ArgumentSpec::number("step", "How many percent to add", "10"),
        ],
        slider: None,
    },
    ActionSpec {
        plugin: DISPLAY_PLUGIN,
        action: "brightness_down",
        description: "Turn the screen brightness down",
        arguments: &[
            MONITOR_ARGUMENT,
            ArgumentSpec::number("step", "How many percent to remove", "10"),
        ],
        slider: None,
    },
];

static DISPLAY_SETTINGS: &[SettingSpec] = &[];

static DISPLAY_MANIFEST: PluginManifest = PluginManifest {
    id: DISPLAY_PLUGIN,
    name: "Display",
    version: "1.0.0",
    description: "Screen brightness, for a laptop's own screen and for external monitors.",
    actions: DISPLAY_ACTIONS,
    settings: DISPLAY_SETTINGS,
};

/// Reads and sets brightness, 0-100, per screen.
pub trait DisplayBackend: Send + std::fmt::Debug {
    /// Every screen that can be dimmed, first the one a laptop user means.
    fn monitors(&mut self) -> Result<Vec<String>, PluginError>;
    fn brightness(&mut self, monitor: &str) -> Result<f64, PluginError>;
    fn set_brightness(&mut self, monitor: &str, percent: f64) -> Result<(), PluginError>;
}

const NO_DISPLAY: &str = "this build cannot change screen brightness on this platform";

#[derive(Debug, Default)]
pub struct UnavailableDisplay;

impl DisplayBackend for UnavailableDisplay {
    fn monitors(&mut self) -> Result<Vec<String>, PluginError> {
        Err(PluginError::Unavailable(NO_DISPLAY.into()))
    }
    fn brightness(&mut self, _monitor: &str) -> Result<f64, PluginError> {
        Err(PluginError::Unavailable(NO_DISPLAY.into()))
    }
    fn set_brightness(&mut self, _monitor: &str, _percent: f64) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_DISPLAY.into()))
    }
}

/// Two screens, remembered: a laptop panel and a monitor.
#[derive(Debug)]
pub struct RecordingDisplay {
    levels: Vec<(String, f64)>,
    calls: Vec<String>,
}

impl Default for RecordingDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingDisplay {
    pub fn new() -> Self {
        Self {
            levels: vec![
                ("Built-in display".into(), 60.0),
                ("DELL U2720Q".into(), 30.0),
            ],
            calls: Vec::new(),
        }
    }

    pub fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl DisplayBackend for RecordingDisplay {
    fn monitors(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(self.levels.iter().map(|(name, _)| name.clone()).collect())
    }

    fn brightness(&mut self, monitor: &str) -> Result<f64, PluginError> {
        self.levels
            .iter()
            .find(|(name, _)| name == monitor)
            .map(|(_, level)| *level)
            .ok_or_else(|| PluginError::Failed(format!("no screen called '{monitor}'")))
    }

    fn set_brightness(&mut self, monitor: &str, percent: f64) -> Result<(), PluginError> {
        let level = self
            .levels
            .iter_mut()
            .find(|(name, _)| name == monitor)
            .ok_or_else(|| PluginError::Failed(format!("no screen called '{monitor}'")))?;
        level.1 = percent;
        self.calls.push(format!("set_brightness {monitor} {percent}"));
        Ok(())
    }
}

pub struct Display {
    backend: Mutex<Box<dyn DisplayBackend>>,
}

impl std::fmt::Debug for Display {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Display").finish_non_exhaustive()
    }
}

impl Display {
    pub fn new(backend: Box<dyn DisplayBackend>) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }

    fn backend(&self) -> MutexGuard<'_, Box<dyn DisplayBackend>> {
        self.backend.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The screens `monitor` means: one by name, or all of them.
    fn targets(&self, monitor: &str) -> Result<Vec<String>, PluginError> {
        let all = self.backend().monitors()?;
        if all.is_empty() {
            return Err(PluginError::Failed(
                "no screen here can have its brightness changed (an external monitor needs \
                 DDC/CI switched on in its own menu)"
                    .into(),
            ));
        }
        let wanted = monitor.trim();
        if wanted.is_empty() || wanted.eq_ignore_ascii_case("all") {
            return Ok(all);
        }
        // Screens are named like devices, so they are picked the same way.
        Ok(vec![pick(&all, wanted, Flow::Output)
            .map_err(|_| {
                PluginError::Failed(format!(
                    "no single screen matches '{wanted}' (there are: {})",
                    all.join(", ")
                ))
            })?
            .to_string()])
    }

    /// Publish the brightness of `monitors`, under the name the action was
    /// given (so a slider's feedback template finds it) and the first screen's
    /// under [`BRIGHTNESS`].
    fn publish(&self, ctx: &mut Ctx<'_>, given: &str, monitors: &[String]) -> Result<(), PluginError> {
        let all = self.backend().monitors()?;
        for monitor in monitors {
            let level = self.backend().brightness(monitor)?;
            if all.first() == Some(monitor) {
                ctx.vars.refresh(BRIGHTNESS, Value::Number(level))?;
            }
            let label = if monitors.len() == 1 && !given.trim().is_empty() {
                given.trim()
            } else {
                monitor.as_str()
            };
            let name = format!("{MONITOR_PREFIX}{label}.brightness");
            if ctx.vars.spec(&name).is_none() {
                ctx.vars.declare(
                    VariableSpec::new(&name, Kind::Number, "That screen's brightness")
                        .unit("%")
                        .threshold(1.0)
                        .read_only(),
                );
            }
            ctx.vars.refresh(&name, Value::Number(level))?;
        }
        Ok(())
    }
}

impl Plugin for Display {
    fn manifest(&self) -> crate::abi::ManifestDto {
        DISPLAY_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        vec![
            VariableSpec::new(BRIGHTNESS, Kind::Number, "Screen brightness")
                .unit("%")
                .threshold(1.0)
                .read_only(),
        ]
    }

    fn choices(
        &self,
        _action: &str,
        _args: &HashMap<String, String>,
    ) -> Result<crate::abi::ChoicesResponse, PluginError> {
        let mut answer = crate::abi::ChoicesResponse::default();
        match self.backend().monitors() {
            Ok(names) => {
                answer.arguments.insert(
                    "monitor".into(),
                    names
                        .into_iter()
                        .map(|value| crate::abi::ChoiceDto { value, label: None })
                        .collect(),
                );
            }
            Err(error) => answer.error = Some(error.to_string()),
        }
        Ok(answer)
    }

    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        for name in wanted {
            if *name == BRIGHTNESS {
                let first = self.backend().monitors()?.into_iter().take(1).collect::<Vec<_>>();
                self.publish(ctx, "", &first)?;
            } else if let Some(monitor) = name
                .strip_prefix(MONITOR_PREFIX)
                .and_then(|rest| rest.strip_suffix(".brightness"))
            {
                let targets = self.targets(monitor)?;
                self.publish(ctx, monitor, &targets)?;
            } else {
                return Err(PluginError::Failed(format!(
                    "the display plugin does not own '{name}'"
                )));
            }
        }
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        let monitor = argument(args, "monitor");
        let targets = self.targets(monitor)?;
        let number = |name: &str| {
            let raw = argument(args, name);
            raw.parse::<f64>()
                .ok()
                .filter(|value| (0.0..=100.0).contains(value))
                .ok_or_else(|| {
                    PluginError::Failed(format!("'{name}' must be between 0 and 100, not '{raw}'"))
                })
        };
        match action {
            "set_brightness" => {
                let level = number("percent")?;
                for target in &targets {
                    self.backend().set_brightness(target, level)?;
                }
            }
            "brightness_up" | "brightness_down" => {
                let step = number("step")?;
                let step = if action == "brightness_up" { step } else { -step };
                for target in &targets {
                    let now = self.backend().brightness(target)?;
                    self.backend()
                        .set_brightness(target, (now + step).clamp(0.0, 100.0))?;
                }
            }
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: DISPLAY_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        }
        self.publish(ctx, monitor, &targets)?;
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

    fn run(
        plugin: &mut Display,
        vars: &VariableStore,
        action: &str,
        pairs: &[(&str, &str)],
    ) -> Result<(), PluginError> {
        let args = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx {
            input: &mut input,
            vars,
            notifier: &mut notifier,
            settings: &settings,
            now_ms: 0,
        };
        plugin.invoke(action, &args, &mut ctx).map(drop)
    }

    fn world() -> (Display, VariableStore) {
        let plugin = Display::new(Box::new(RecordingDisplay::new()));
        let vars = VariableStore::new();
        vars.declare_all(plugin.variables());
        (plugin, vars)
    }

    #[test]
    fn up_and_down_move_every_screen_and_stop_at_the_ends() {
        let (mut plugin, vars) = world();
        run(&mut plugin, &vars, "brightness_up", &[("step", "50")]).unwrap();
        assert_eq!(vars.get(BRIGHTNESS), Some(Value::Number(100.0)), "clamped");
        assert_eq!(
            vars.get("display.monitor.DELL U2720Q.brightness"),
            Some(Value::Number(80.0))
        );
        run(&mut plugin, &vars, "brightness_down", &[("monitor", "all"), ("step", "90")]).unwrap();
        assert_eq!(
            vars.get("display.monitor.DELL U2720Q.brightness"),
            Some(Value::Number(0.0))
        );
    }

    #[test]
    fn one_screen_is_picked_by_part_of_its_name() {
        let (mut plugin, vars) = world();
        run(&mut plugin, &vars, "set_brightness", &[("monitor", "dell"), ("percent", "45")]).unwrap();
        assert_eq!(
            vars.get("display.monitor.dell.brightness"),
            Some(Value::Number(45.0)),
            "published under the name the slider was given"
        );
        assert_eq!(vars.get(BRIGHTNESS), None, "the laptop screen was not touched");
        let error = run(&mut plugin, &vars, "set_brightness", &[("monitor", "lg"), ("percent", "45")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("DELL U2720Q"), "{error}");
        assert!(run(&mut plugin, &vars, "set_brightness", &[("percent", "140")]).is_err());
    }

    #[test]
    fn a_slider_bound_to_one_screen_is_filled_in_on_demand() {
        let (mut plugin, vars) = world();
        let name = "display.monitor.Built-in display.brightness";
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx {
            input: &mut input,
            vars: &vars,
            notifier: &mut notifier,
            settings: &settings,
            now_ms: 0,
        };
        plugin.sample(&[name, BRIGHTNESS], &mut ctx).unwrap();
        assert_eq!(vars.get(name), Some(Value::Number(60.0)));
        assert_eq!(vars.get(BRIGHTNESS), Some(Value::Number(60.0)));
    }
}

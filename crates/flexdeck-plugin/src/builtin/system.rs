//! The `system` plugin: audio volume, mute, output device, and CPU usage.
//!
//! # Why there is no Windows code here yet
//!
//! Every method goes through [`SystemBackend`], and the real implementation is
//! *not* in this crate. That is a deliberate consequence of the jury's most
//! concrete finding: the only CI job is `workflow_dispatch`-only, runs on Linux,
//! and has no Tauri system dependencies, so an OS-gated backend would be
//! verified on neither platform. Writing it anyway would be unverifiable code in
//! a repository whose rules forbid exactly that.
//!
//! So this plugin declares the capability, the actions and the variables, and
//! takes the backend as an argument. [`UnavailableSystem`] is the honest default
//! and [`RecordingSystem`] is what the tests use — the same pair, and the same
//! reasoning, as `EnigoExecutor` versus `RecordingExecutor`.
//!
//! # Why the variables are read-only and thresholded
//!
//! `system.audio.volume` belongs to the operating system, so an automation
//! changing it directly would make the automation its own source of truth. It is
//! `read_only`, and the **actions** are how anyone changes it. Both audio
//! variables carry a threshold, so a jittering reading does not wake the engine
//! on every sample.

use std::collections::HashMap;

use std::sync::{Mutex, MutexGuard, PoisonError};

use flexdeck_actions::{ActionSpec, ActiveSpec, ArgumentSpec, SliderSpec};
use flexdeck_vars::{Kind, Value, VariableSpec};

use super::audio::{AudioBackend, Flow};
use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest, SettingSpec, SystemBackend};

pub const SYSTEM_PLUGIN: &str = "system";

pub const VOLUME: &str = "system.audio.volume";
pub const MUTED: &str = "system.audio.muted";
pub const OUTPUT_DEVICE: &str = "system.audio.output_device";
pub const CPU_USAGE: &str = "system.cpu.usage";
pub const INPUT_VOLUME: &str = "system.audio.input.volume";
pub const INPUT_MUTED: &str = "system.audio.input.muted";
pub const INPUT_DEVICE: &str = "system.audio.input_device";

/// `system.audio.device.<name>.volume` / `.muted`: one output device.
pub const DEVICE_PREFIX: &str = "system.audio.device.";
/// `system.audio.app.<exe>.volume` / `.muted`: one application.
pub const APP_PREFIX: &str = "system.audio.app.";
/// `system.audio.input_device.<name>.volume` / `.muted`: one microphone.
pub const INPUT_DEVICE_PREFIX: &str = "system.audio.input_device.";

const DEVICE_ARGUMENT: ArgumentSpec = ArgumentSpec::text(
    "device",
    "The device's name, or enough of it to pick one",
    "Headphones",
);
const MIC_ARGUMENT: ArgumentSpec = ArgumentSpec::text(
    "device",
    "The microphone's name, or enough of it to pick one",
    "Microphone",
);
const APP_ARGUMENT: ArgumentSpec =
    ArgumentSpec::text("app", "The program, by its executable", "spotify.exe");
const PERCENT_ARGUMENT: ArgumentSpec =
    ArgumentSpec::number("percent", "Volume as a percentage, 0 to 100", "50");
const MUTED_ARGUMENT: ArgumentSpec =
    ArgumentSpec::choice("muted", "Whether to mute", "true", &["true", "false"]);

static SYSTEM_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_volume",
        description: "Set the output volume",
        arguments: &[PERCENT_ARGUMENT],
        slider: Some(SliderSpec::percent("percent", VOLUME).with_mute("toggle_mute", MUTED)),
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "volume_up",
        description: "Turn the output volume up",
        arguments: &[ArgumentSpec::number("step", "How many percent to add", "5")],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "volume_down",
        description: "Turn the output volume down",
        arguments: &[ArgumentSpec::number(
            "step",
            "How many percent to remove",
            "5",
        )],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_muted",
        description: "Mute or unmute the output",
        arguments: &[ArgumentSpec::choice(
            "muted",
            "Whether to mute",
            "true",
            &["true", "false"],
        )],
        slider: None,
        active: Some(ActiveSpec::equals(MUTED, "muted")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "toggle_mute",
        description: "Mute if unmuted, unmute if muted",
        arguments: &[],
        slider: None,
        active: Some(ActiveSpec::while_true(MUTED)),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_output_device",
        description: "Switch the audio output to another device",
        arguments: &[ArgumentSpec::text(
            "device",
            "The device name, as listed by the output devices",
            "speaker",
        )],
        slider: None,
        active: Some(ActiveSpec::equals(OUTPUT_DEVICE, "device")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_device_volume",
        description: "Set one output device's volume, default or not",
        arguments: &[DEVICE_ARGUMENT, PERCENT_ARGUMENT],
        slider: Some(
            SliderSpec::percent("percent", "system.audio.device.{{device}}.volume")
                .with_mute("toggle_device_mute", "system.audio.device.{{device}}.muted"),
        ),
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "toggle_device_mute",
        description: "Mute one output device if it is live, unmute it if muted",
        arguments: &[DEVICE_ARGUMENT],
        slider: None,
        active: Some(ActiveSpec::while_true("system.audio.device.{{device}}.muted")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_app_volume",
        description: "Set one program's volume, as in the Volume Mixer",
        arguments: &[APP_ARGUMENT, PERCENT_ARGUMENT],
        slider: Some(
            SliderSpec::percent("percent", "system.audio.app.{{app}}.volume")
                .with_mute("toggle_app_mute", "system.audio.app.{{app}}.muted"),
        ),
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_app_muted",
        description: "Mute or unmute one program",
        arguments: &[APP_ARGUMENT, MUTED_ARGUMENT],
        slider: None,
        active: Some(ActiveSpec::equals("system.audio.app.{{app}}.muted", "muted")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "toggle_app_mute",
        description: "Mute one program if it is playing, unmute it if muted",
        arguments: &[APP_ARGUMENT],
        slider: None,
        active: Some(ActiveSpec::while_true("system.audio.app.{{app}}.muted")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_input_volume",
        description: "Set the microphone's volume",
        arguments: &[PERCENT_ARGUMENT],
        slider: Some(
            SliderSpec::percent("percent", INPUT_VOLUME).with_mute("toggle_input_mute", INPUT_MUTED),
        ),
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_input_device_volume",
        description: "Set one microphone's volume, default or not",
        arguments: &[MIC_ARGUMENT, PERCENT_ARGUMENT],
        slider: Some(
            SliderSpec::percent("percent", "system.audio.input_device.{{device}}.volume").with_mute(
                "toggle_input_device_mute",
                "system.audio.input_device.{{device}}.muted",
            ),
        ),
        active: None,
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "toggle_input_device_mute",
        description: "Mute one microphone if it is live, unmute it if muted",
        arguments: &[MIC_ARGUMENT],
        slider: None,
        active: Some(ActiveSpec::while_true("system.audio.input_device.{{device}}.muted")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "toggle_input_mute",
        description: "Mute the microphone if it is live, unmute it if muted",
        arguments: &[],
        slider: None,
        active: Some(ActiveSpec::while_true(INPUT_MUTED)),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_input_muted",
        description: "Mute or unmute the microphone",
        arguments: &[MUTED_ARGUMENT],
        slider: None,
        active: Some(ActiveSpec::equals(INPUT_MUTED, "muted")),
    },
    ActionSpec {
        plugin: SYSTEM_PLUGIN,
        action: "set_input_device",
        description: "Switch the default microphone",
        arguments: &[ArgumentSpec::text(
            "device",
            "The microphone's name, or enough of it to pick one",
            "Microphone",
        )],
        slider: None,
        active: Some(ActiveSpec::equals(INPUT_DEVICE, "device")),
    },
];

static SYSTEM_SETTINGS: &[SettingSpec] = &[];

static SYSTEM_MANIFEST: PluginManifest = PluginManifest {
    id: SYSTEM_PLUGIN,
    name: "System",
    version: "1.0.0",
    description: "Volume for every device and program, the microphone, and processor usage.",
    actions: SYSTEM_ACTIONS,
    settings: SYSTEM_SETTINGS,
};

pub struct System {
    backend: Box<dyn SystemBackend>,
    /// Behind a lock because `choices` only gets `&self`, and the editor's
    /// suggestions should be the devices and programs there are *now*.
    audio: Mutex<Box<dyn AudioBackend>>,
}

impl std::fmt::Debug for System {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("System").finish_non_exhaustive()
    }
}

impl System {
    pub fn new(backend: Box<dyn SystemBackend>, audio: Box<dyn AudioBackend>) -> Self {
        Self {
            backend,
            audio: Mutex::new(audio),
        }
    }

    fn audio(&self) -> MutexGuard<'_, Box<dyn AudioBackend>> {
        self.audio.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Declare a per-device or per-app variable the first time it is wanted,
    /// then write it. They cannot be declared up front: nobody knows which
    /// devices and programs a deck will care about until it asks.
    fn refresh_dynamic(&self, ctx: &mut Ctx<'_>, name: &str, value: Value) -> Result<(), PluginError> {
        if ctx.vars.spec(name).is_none() {
            let (kind, description) = match value {
                Value::Bool(_) => (Kind::Bool, "Whether it is muted"),
                _ => (Kind::Number, "Its volume"),
            };
            let mut spec = VariableSpec::new(name, kind, description).read_only();
            if kind == Kind::Number {
                spec = spec.unit("%").threshold(1.0);
            }
            ctx.vars.declare(spec);
        }
        ctx.vars.refresh(name, value)?;
        Ok(())
    }

    fn refresh_app(&mut self, ctx: &mut Ctx<'_>, app: &str) -> Result<(), PluginError> {
        let raw = app.trim();
        let volume = self.audio().app_volume(raw)?;
        let muted = self.audio().app_muted(raw)?;
        self.refresh_dynamic(ctx, &format!("{APP_PREFIX}{raw}.volume"), Value::Number(volume))?;
        self.refresh_dynamic(ctx, &format!("{APP_PREFIX}{raw}.muted"), Value::Bool(muted))
    }

    fn refresh_device(&mut self, ctx: &mut Ctx<'_>, flow: Flow, device: &str) -> Result<(), PluginError> {
        let raw = device.trim();
        let prefix = match flow {
            Flow::Output => DEVICE_PREFIX,
            Flow::Input => INPUT_DEVICE_PREFIX,
        };
        let volume = self.audio().device_volume(flow, raw)?;
        let muted = self.audio().device_muted(flow, raw)?;
        self.refresh_dynamic(ctx, &format!("{prefix}{raw}.volume"), Value::Number(volume))?;
        self.refresh_dynamic(ctx, &format!("{prefix}{raw}.muted"), Value::Bool(muted))
    }

    fn refresh_input(&mut self, ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let volume = self.audio().device_volume(Flow::Input, "")?;
        let muted = self.audio().device_muted(Flow::Input, "")?;
        ctx.vars.refresh(INPUT_VOLUME, Value::Number(volume))?;
        ctx.vars.refresh(INPUT_MUTED, Value::Bool(muted))?;
        Ok(())
    }

    fn refresh_input_device(&mut self, ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let name = self
            .audio()
            .devices(Flow::Input)?
            .into_iter()
            .find(|device| device.default)
            .map(|device| device.name)
            .unwrap_or_default();
        ctx.vars.refresh(INPUT_DEVICE, Value::Text(name))?;
        Ok(())
    }

    fn refresh_volume(&mut self, ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let volume = self.backend.volume()?;
        ctx.vars.refresh(VOLUME, Value::Number(volume))?;
        Ok(())
    }

    fn refresh_muted(&mut self, ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let muted = self.backend.muted()?;
        ctx.vars.refresh(MUTED, Value::Bool(muted))?;
        Ok(())
    }

    fn refresh_output_device(&mut self, ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let device = self.backend.output_device()?;
        ctx.vars.refresh(OUTPUT_DEVICE, Value::Text(device))?;
        Ok(())
    }
}

impl Plugin for System {
    fn manifest(&self) -> crate::abi::ManifestDto {
        SYSTEM_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        vec![
            VariableSpec::new(VOLUME, Kind::Number, "Output volume")
                .unit("%")
                .threshold(1.0)
                .read_only(),
            VariableSpec::new(MUTED, Kind::Bool, "Whether the output is muted").read_only(),
            VariableSpec::new(OUTPUT_DEVICE, Kind::Text, "The current output device").read_only(),
            VariableSpec::new(CPU_USAGE, Kind::Number, "Processor usage")
                .unit("%")
                .threshold(5.0)
                .read_only(),
            VariableSpec::new(INPUT_VOLUME, Kind::Number, "Microphone volume")
                .unit("%")
                .threshold(1.0)
                .read_only(),
            VariableSpec::new(INPUT_MUTED, Kind::Bool, "Whether the microphone is muted")
                .read_only(),
            VariableSpec::new(INPUT_DEVICE, Kind::Text, "The current microphone").read_only(),
        ]
    }

    fn choices(
        &self,
        action: &str,
        _args: &HashMap<String, String>,
    ) -> Result<crate::abi::ChoicesResponse, PluginError> {
        let mut audio = self.audio();
        let (argument, found) = match action {
            "set_output_device" | "set_device_volume" | "toggle_device_mute" => {
                ("device", device_choices(audio.devices(Flow::Output)))
            }
            "set_input_device" | "set_input_device_volume" | "toggle_input_device_mute" => {
                ("device", device_choices(audio.devices(Flow::Input)))
            }
            "set_app_volume" | "set_app_muted" | "toggle_app_mute" => (
                "app",
                audio.apps().map(|apps| {
                    apps.into_iter()
                        .map(|app| crate::abi::ChoiceDto {
                            label: app.title,
                            value: app.exe,
                        })
                        .collect()
                }),
            ),
            _ => return Ok(crate::abi::ChoicesResponse::default()),
        };
        let mut answer = crate::abi::ChoicesResponse::default();
        match found {
            Ok(choices) => {
                answer.arguments.insert(argument.to_string(), choices);
            }
            Err(error) => answer.error = Some(error.to_string()),
        }
        Ok(answer)
    }

    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        for name in wanted {
            match *name {
                VOLUME => self.refresh_volume(ctx)?,
                MUTED => self.refresh_muted(ctx)?,
                OUTPUT_DEVICE => self.refresh_output_device(ctx)?,
                CPU_USAGE => {
                    let usage = self.backend.cpu_usage()?;
                    ctx.vars.refresh(CPU_USAGE, Value::Number(usage))?;
                }
                INPUT_VOLUME | INPUT_MUTED => self.refresh_input(ctx)?,
                INPUT_DEVICE => self.refresh_input_device(ctx)?,
                other => match dynamic(other) {
                    Some(Dynamic::App(app)) => self.refresh_app(ctx, app)?,
                    Some(Dynamic::Device(device)) => self.refresh_device(ctx, Flow::Output, device)?,
                    Some(Dynamic::Input(device)) => self.refresh_device(ctx, Flow::Input, device)?,
                    None => {
                        return Err(PluginError::Failed(format!(
                            "the system plugin does not own '{other}'"
                        )))
                    }
                },
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
        match action {
            "set_volume" => {
                self.backend
                    .set_volume(percent(number(args, "percent")?, "percent")?)?;
                self.refresh_volume(ctx)?;
            }
            "volume_up" => {
                let step = percent(number(args, "step")?, "step")?;
                let now = self.backend.volume()?;
                self.backend.set_volume((now + step).min(100.0))?;
                self.refresh_volume(ctx)?;
            }
            "volume_down" => {
                let step = percent(number(args, "step")?, "step")?;
                let now = self.backend.volume()?;
                self.backend.set_volume((now - step).max(0.0))?;
                self.refresh_volume(ctx)?;
            }
            "set_muted" => {
                self.backend.set_muted(argument(args, "muted") == "true")?;
                self.refresh_muted(ctx)?;
            }
            "toggle_mute" => {
                let now = self.backend.muted()?;
                self.backend.set_muted(!now)?;
                self.refresh_muted(ctx)?;
            }
            "set_output_device" => {
                self.backend.set_output_device(argument(args, "device"))?;
                self.refresh_output_device(ctx)?;
            }
            "set_device_volume" => {
                let device = argument(args, "device");
                self.audio().set_device_volume(
                    Flow::Output,
                    device,
                    percent(number(args, "percent")?, "percent")?,
                )?;
                self.refresh_device(ctx, Flow::Output, device)?;
            }
            "toggle_device_mute" => {
                let device = argument(args, "device");
                let now = self.audio().device_muted(Flow::Output, device)?;
                self.audio().set_device_muted(Flow::Output, device, !now)?;
                self.refresh_device(ctx, Flow::Output, device)?;
            }
            "set_input_device_volume" => {
                let device = argument(args, "device");
                self.audio().set_device_volume(
                    Flow::Input,
                    device,
                    percent(number(args, "percent")?, "percent")?,
                )?;
                self.refresh_device(ctx, Flow::Input, device)?;
            }
            "toggle_input_device_mute" => {
                let device = argument(args, "device");
                let now = self.audio().device_muted(Flow::Input, device)?;
                self.audio().set_device_muted(Flow::Input, device, !now)?;
                self.refresh_device(ctx, Flow::Input, device)?;
            }
            "set_app_volume" => {
                let app = argument(args, "app");
                self.audio()
                    .set_app_volume(app, percent(number(args, "percent")?, "percent")?)?;
                self.refresh_app(ctx, app)?;
            }
            "set_app_muted" => {
                let app = argument(args, "app");
                self.audio().set_app_muted(app, argument(args, "muted") == "true")?;
                self.refresh_app(ctx, app)?;
            }
            "toggle_app_mute" => {
                let app = argument(args, "app");
                let now = self.audio().app_muted(app)?;
                self.audio().set_app_muted(app, !now)?;
                self.refresh_app(ctx, app)?;
            }
            "set_input_volume" => {
                self.audio().set_device_volume(
                    Flow::Input,
                    "",
                    percent(number(args, "percent")?, "percent")?,
                )?;
                self.refresh_input(ctx)?;
            }
            "set_input_muted" => {
                self.audio()
                    .set_device_muted(Flow::Input, "", argument(args, "muted") == "true")?;
                self.refresh_input(ctx)?;
            }
            "toggle_input_mute" => {
                let now = self.audio().device_muted(Flow::Input, "")?;
                self.audio().set_device_muted(Flow::Input, "", !now)?;
                self.refresh_input(ctx)?;
            }
            "set_input_device" => {
                self.audio()
                    .set_default_device(Flow::Input, argument(args, "device"))?;
                self.refresh_input_device(ctx)?;
            }
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: SYSTEM_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        }

        // The actions change system state rather than injecting input, so there
        // is no `CoreAction` to report.
        Ok(Outcome::default())
    }
}

/// A per-app or per-device variable, by what it names.
#[derive(Debug, PartialEq, Eq)]
enum Dynamic<'a> {
    App(&'a str),
    Device(&'a str),
    Input(&'a str),
}

/// Read `system.audio.app.<exe>.volume` and its kin. The name in the middle is
/// the argument exactly as the person typed it — a device name may hold dots —
/// so only the known prefix and suffix are taken off.
fn dynamic(name: &str) -> Option<Dynamic<'_>> {
    let middle = |prefix: &str| {
        let rest = name.strip_prefix(prefix)?;
        let rest = rest
            .strip_suffix(".volume")
            .or_else(|| rest.strip_suffix(".muted"))?;
        (!rest.trim().is_empty()).then_some(rest)
    };
    middle(APP_PREFIX)
        .map(Dynamic::App)
        .or_else(|| middle(DEVICE_PREFIX).map(Dynamic::Device))
        .or_else(|| middle(INPUT_DEVICE_PREFIX).map(Dynamic::Input))
}

/// Devices as choices, the default one saying so.
fn device_choices(
    devices: Result<Vec<super::audio::AudioDevice>, PluginError>,
) -> Result<Vec<crate::abi::ChoiceDto>, PluginError> {
    devices.map(|list| {
        list.into_iter()
            .map(|device| crate::abi::ChoiceDto {
                label: device.default.then(|| format!("{} (default)", device.name)),
                value: device.name,
            })
            .collect()
    })
}

fn number(args: &HashMap<String, String>, name: &str) -> Result<f64, PluginError> {
    let raw = argument(args, name);
    raw.parse::<f64>()
        .map_err(|_| PluginError::Failed(format!("'{name}' must be a number, not '{raw}'")))
}

/// Refuse a percentage outside `0..=100` rather than clamping it.
///
/// Clamping would silently obey a different instruction than the one written,
/// and `volume_up 500` reading as `volume_up 100` is the kind of quiet
/// substitution that makes a deck feel broken rather than misconfigured.
fn percent(value: f64, name: &str) -> Result<f64, PluginError> {
    if (0.0..=100.0).contains(&value) {
        Ok(value)
    } else {
        Err(PluginError::Failed(format!(
            "'{name}' must be between 0 and 100, not {value}"
        )))
    }
}

/// A system backend that remembers what it was told, for tests.
#[derive(Debug)]
pub struct RecordingSystem {
    volume: f64,
    muted: bool,
    device: String,
    devices: Vec<String>,
    cpu: f64,
    calls: Vec<String>,
}

impl Default for RecordingSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingSystem {
    /// Starts at a plausible midpoint rather than zero, so a test that reads a
    /// value cannot pass simply because everything is zero.
    pub fn new() -> Self {
        Self {
            volume: 40.0,
            muted: false,
            device: "speaker".to_string(),
            devices: vec!["speaker".to_string(), "headphones".to_string()],
            cpu: 12.0,
            calls: Vec::new(),
        }
    }

    pub fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl SystemBackend for RecordingSystem {
    fn volume(&mut self) -> Result<f64, PluginError> {
        Ok(self.volume)
    }

    fn set_volume(&mut self, percent: f64) -> Result<(), PluginError> {
        self.calls.push(format!("set_volume {percent}"));
        self.volume = percent;
        Ok(())
    }

    fn muted(&mut self) -> Result<bool, PluginError> {
        Ok(self.muted)
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), PluginError> {
        self.calls.push(format!("set_muted {muted}"));
        self.muted = muted;
        Ok(())
    }

    fn output_device(&mut self) -> Result<String, PluginError> {
        Ok(self.device.clone())
    }

    fn set_output_device(&mut self, device: &str) -> Result<(), PluginError> {
        if !self.devices.iter().any(|known| known == device) {
            return Err(PluginError::Failed(format!(
                "there is no output device called '{device}'"
            )));
        }
        self.calls.push(format!("set_output_device {device}"));
        self.device = device.to_string();
        Ok(())
    }

    fn output_devices(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(self.devices.clone())
    }

    fn cpu_usage(&mut self) -> Result<f64, PluginError> {
        Ok(self.cpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_and_its_lights_are_valid() {
        let manifest = SYSTEM_MANIFEST.to_dto();
        manifest.validate().expect("valid");
        assert!(manifest.uses(crate::abi::capability::TOGGLES), "its toggles light up");
    }
    use crate::builtin::audio::RecordingAudio;
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    struct World {
        plugin: System,
        vars: VariableStore,
    }

    fn world() -> World {
        let plugin = System::new(
            Box::new(RecordingSystem::new()),
            Box::new(RecordingAudio::new()),
        );
        let vars = VariableStore::new();
        vars.declare_all(plugin.variables());
        World { plugin, vars }
    }

    impl World {
        fn run(&mut self, action: &str, pairs: &[(&str, &str)]) -> Result<(), PluginError> {
            let args: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect();
            let mut input = RecordingExecutor::new();
            let mut notifier = RecordingNotifier::new();
            let settings = BTreeMap::new();
            let mut ctx = Ctx {
                input: &mut input,
                vars: &self.vars,
                notifier: &mut notifier,
                settings: &settings,
                now_ms: 0,
            };
            self.plugin.invoke(action, &args, &mut ctx).map(drop)
        }

        fn sample(&mut self, names: &[&str]) -> Result<(), PluginError> {
            let mut input = RecordingExecutor::new();
            let mut notifier = RecordingNotifier::new();
            let settings = BTreeMap::new();
            let mut ctx = Ctx {
                input: &mut input,
                vars: &self.vars,
                notifier: &mut notifier,
                settings: &settings,
                now_ms: 0,
            };
            self.plugin.sample(names, &mut ctx)
        }
    }

    #[test]
    fn a_programs_volume_is_set_and_reported_under_the_name_it_was_given() {
        let mut world = world();
        world
            .run("set_app_volume", &[("app", "spotify.exe"), ("percent", "25")])
            .expect("runs");
        assert_eq!(
            world.vars.get("system.audio.app.spotify.exe.volume"),
            Some(Value::Number(25.0)),
            "the slider's feedback variable follows"
        );
        world.run("toggle_app_mute", &[("app", "Spotify")]).expect("runs");
        assert_eq!(
            world.vars.get("system.audio.app.Spotify.muted"),
            Some(Value::Bool(true))
        );
        assert!(world
            .run("set_app_volume", &[("app", "winamp.exe"), ("percent", "5")])
            .is_err());
    }

    #[test]
    fn a_device_that_is_not_the_default_has_its_own_volume() {
        let mut world = world();
        world
            .run(
                "set_device_volume",
                &[("device", "Headphones"), ("percent", "15")],
            )
            .expect("runs");
        assert_eq!(
            world.vars.get("system.audio.device.Headphones.volume"),
            Some(Value::Number(15.0))
        );
        world.sample(&[VOLUME]).expect("samples");
        assert_eq!(
            world.vars.get(VOLUME),
            Some(Value::Number(40.0)),
            "the default output did not move"
        );
    }

    #[test]
    fn the_microphone_has_its_volume_mute_and_device() {
        let mut world = world();
        world.run("set_input_volume", &[("percent", "33")]).unwrap();
        world.run("toggle_input_mute", &[]).unwrap();
        assert_eq!(world.vars.get(INPUT_VOLUME), Some(Value::Number(33.0)));
        assert_eq!(world.vars.get(INPUT_MUTED), Some(Value::Bool(true)));
        world.sample(&[INPUT_DEVICE]).unwrap();
        assert_eq!(
            world.vars.get(INPUT_DEVICE),
            Some(Value::Text("Microphone (Yeti)".into()))
        );
        assert!(world
            .run("set_input_device", &[("device", "webcam")])
            .is_err());
    }

    #[test]
    fn a_watched_dynamic_variable_is_declared_and_read_on_demand() {
        // A slider bound to Discord's volume asks for it before anyone has
        // touched it: the sampler must be able to fill it in from nothing.
        let mut world = world();
        let name = "system.audio.app.discord.exe.volume";
        assert!(world.vars.spec(name).is_none());
        world.sample(&[name]).expect("samples");
        assert_eq!(world.vars.get(name), Some(Value::Number(100.0)));
        assert!(world.vars.spec(name).is_some_and(|spec| spec.read_only));
        assert!(world.sample(&["system.audio.app..volume"]).is_err());
    }

    #[test]
    fn any_microphone_and_any_output_has_its_own_volume_and_mute() {
        let mut world = world();
        world
            .run("set_input_device_volume", &[("device", "Yeti"), ("percent", "21")])
            .unwrap();
        world.run("toggle_input_device_mute", &[("device", "Yeti")]).unwrap();
        assert_eq!(
            world.vars.get("system.audio.input_device.Yeti.volume"),
            Some(Value::Number(21.0))
        );
        assert_eq!(
            world.vars.get("system.audio.input_device.Yeti.muted"),
            Some(Value::Bool(true))
        );
        world.run("toggle_device_mute", &[("device", "Headphones")]).unwrap();
        assert_eq!(
            world.vars.get("system.audio.device.Headphones.muted"),
            Some(Value::Bool(true))
        );
        let name = "system.audio.input_device.Yeti.volume";
        world.sample(&[name]).expect("a watched microphone is read on demand");
    }

    #[test]
    fn dynamic_names_keep_dots_inside_the_middle() {
        assert_eq!(
            dynamic("system.audio.device.Speakers (Realtek(R) 2.0).volume"),
            Some(Dynamic::Device("Speakers (Realtek(R) 2.0)"))
        );
        assert_eq!(
            dynamic("system.audio.app.spotify.exe.muted"),
            Some(Dynamic::App("spotify.exe"))
        );
        assert_eq!(dynamic("system.audio.app.spotify.exe"), None);
    }

    #[test]
    fn the_editor_is_offered_the_devices_and_programs_there_are() {
        let world = world();
        let apps = world
            .plugin
            .choices("set_app_volume", &HashMap::new())
            .unwrap();
        let values: Vec<&str> = apps.arguments["app"].iter().map(|c| c.value.as_str()).collect();
        assert_eq!(values, ["spotify.exe", "discord.exe"]);
        assert_eq!(apps.arguments["app"][0].label.as_deref(), Some("Spotify"), "named as people know it");
        let outputs = world
            .plugin
            .choices("set_device_volume", &HashMap::new())
            .unwrap();
        assert_eq!(
            outputs.arguments["device"][0].label.as_deref(),
            Some("Speakers (Realtek Audio) (default)")
        );
        assert_eq!(outputs.arguments["device"][1].label, None);
        let mics = world
            .plugin
            .choices("set_input_device", &HashMap::new())
            .unwrap();
        assert_eq!(mics.arguments["device"].len(), 1);
    }

    #[test]
    fn every_volume_action_a_slider_can_drive_says_so() {
        let manifest = SYSTEM_MANIFEST.to_dto();
        let slidable: Vec<&str> = manifest
            .actions
            .iter()
            .filter(|action| action.slider.is_some())
            .map(|action| action.action.as_str())
            .collect();
        assert_eq!(
            slidable,
            [
                "set_volume",
                "set_device_volume",
                "set_app_volume",
                "set_input_volume",
                "set_input_device_volume"
            ]
        );
        manifest.validate().expect("valid at ABI 3");
        // Every slider's mute names an action that exists, taking no more
        // than the slider action's own other arguments.
        for action in manifest.actions.iter().filter(|a| a.slider.is_some()) {
            let slider = action.slider.as_ref().unwrap();
            let mute = slider.mute.as_ref().expect("every volume can be muted");
            let toggle = manifest
                .actions
                .iter()
                .find(|a| a.action == mute.action)
                .unwrap_or_else(|| panic!("{} names a missing {}", action.action, mute.action));
            for argument in &toggle.arguments {
                assert!(
                    action.arguments.iter().any(|a| a.name == argument.name),
                    "{} needs {}",
                    mute.action,
                    argument.name
                );
            }
        }
    }
}

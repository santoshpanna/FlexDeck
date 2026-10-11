//! The `sensors` plugin: how the machine is doing.
//!
//! Memory, disk, network, each CPU core, the GPU, the battery, uptime — the
//! values a stats key or a history graph wants. `system.cpu.usage` stays in
//! the system plugin; this adds the rest.
//!
//! Read through an injected [`SensorBackend`], so a test pins the readings and
//! another platform answers "not available" rather than inventing numbers.

use std::collections::HashMap;

use flexdeck_vars::{Kind, Value, VariableSpec};

use crate::{Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const SENSORS_PLUGIN: &str = "sensors";

/// One reading of everything, as far as the machine can tell.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings {
    pub memory_used_percent: Option<f64>,
    pub memory_used_gb: Option<f64>,
    pub memory_total_gb: Option<f64>,
    pub disk_used_percent: Option<f64>,
    pub disk_free_gb: Option<f64>,
    /// Bytes a second, since the last reading.
    pub network_down: Option<f64>,
    pub network_up: Option<f64>,
    pub cores: Vec<f64>,
    pub gpu_percent: Option<f64>,
    pub battery_percent: Option<f64>,
    pub battery_charging: Option<bool>,
    pub uptime_seconds: Option<f64>,
}

pub trait SensorBackend: Send + std::fmt::Debug {
    fn read(&mut self) -> Result<Readings, PluginError>;
}

/// Every read refuses with a reason: no sensors on this platform yet.
#[derive(Debug, Default)]
pub struct UnavailableSensors;

impl SensorBackend for UnavailableSensors {
    fn read(&mut self) -> Result<Readings, PluginError> {
        Err(PluginError::Unavailable("sensors are not available on this platform yet".into()))
    }
}

/// Fixed readings. For tests.
#[derive(Debug, Default, Clone)]
pub struct FixedSensors(pub Readings);

impl SensorBackend for FixedSensors {
    fn read(&mut self) -> Result<Readings, PluginError> {
        Ok(self.0.clone())
    }
}

/// The fixed variables: name, kind, description, unit, threshold.
const VARIABLES: &[(&str, Kind, &str, &str, f64)] = &[
    ("sensors.memory.used", Kind::Number, "Memory in use", "%", 1.0),
    ("sensors.memory.used_gb", Kind::Number, "Memory in use", "GB", 0.1),
    ("sensors.memory.total_gb", Kind::Number, "Memory in all", "GB", 0.1),
    ("sensors.disk.used", Kind::Number, "System drive in use", "%", 0.5),
    ("sensors.disk.free_gb", Kind::Number, "System drive free", "GB", 0.5),
    ("sensors.network.down", Kind::Number, "Downloading, bytes a second", "B/s", 1024.0),
    ("sensors.network.up", Kind::Number, "Uploading, bytes a second", "B/s", 1024.0),
    ("sensors.gpu.usage", Kind::Number, "Graphics card busy", "%", 1.0),
    ("sensors.battery.level", Kind::Number, "Battery charge", "%", 1.0),
    ("sensors.battery.charging", Kind::Bool, "Whether the battery is charging", "", 0.0),
    ("sensors.uptime", Kind::Number, "Seconds since the machine started", "s", 1.0),
];

const CORE_PREFIX: &str = "sensors.cpu.core.";

static SENSORS_MANIFEST: PluginManifest = PluginManifest {
    id: SENSORS_PLUGIN,
    name: "Sensors",
    version: "1.0.0",
    description: "Memory, disk, network, each CPU core, the GPU and the battery.",
    actions: &[],
    settings: &[],
};

#[derive(Debug)]
pub struct Sensors {
    backend: Box<dyn SensorBackend>,
    /// How many cores were declared, so they are declared once.
    cores: usize,
}

impl Sensors {
    pub fn new(backend: Box<dyn SensorBackend>) -> Self {
        Self { backend, cores: 0 }
    }
}

impl Plugin for Sensors {
    fn manifest(&self) -> crate::abi::ManifestDto {
        SENSORS_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        let mut specs: Vec<VariableSpec> = VARIABLES
            .iter()
            .map(|(name, kind, description, unit, threshold)| {
                let mut spec = VariableSpec::new(*name, *kind, *description).read_only();
                if !unit.is_empty() {
                    spec = spec.unit(*unit);
                }
                if *threshold > 0.0 {
                    spec = spec.threshold(*threshold);
                }
                spec
            })
            .collect();
        specs.extend((0..self.cores).map(|n| {
            VariableSpec::new(format!("{CORE_PREFIX}{n}"), Kind::Number, format!("CPU core {n} busy"))
                .unit("%")
                .threshold(1.0)
                .read_only()
        }));
        specs
    }

    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let readings = self.backend.read()?;
        // Cores are known only once read: declared then, so a picker lists them.
        if readings.cores.len() != self.cores {
            for n in 0..readings.cores.len() {
                let name = format!("{CORE_PREFIX}{n}");
                if ctx.vars.spec(&name).is_none() {
                    ctx.vars.declare(
                        VariableSpec::new(&name, Kind::Number, format!("CPU core {n} busy")).unit("%").threshold(1.0).read_only(),
                    );
                }
            }
            self.cores = readings.cores.len();
        }
        let number = |value: Option<f64>| value.filter(|v| v.is_finite()).map(Value::Number);
        for name in wanted {
            let value = match *name {
                "sensors.memory.used" => number(readings.memory_used_percent),
                "sensors.memory.used_gb" => number(readings.memory_used_gb),
                "sensors.memory.total_gb" => number(readings.memory_total_gb),
                "sensors.disk.used" => number(readings.disk_used_percent),
                "sensors.disk.free_gb" => number(readings.disk_free_gb),
                "sensors.network.down" => number(readings.network_down),
                "sensors.network.up" => number(readings.network_up),
                "sensors.gpu.usage" => number(readings.gpu_percent),
                "sensors.battery.level" => number(readings.battery_percent),
                "sensors.battery.charging" => readings.battery_charging.map(Value::Bool),
                "sensors.uptime" => number(readings.uptime_seconds),
                other => match other.strip_prefix(CORE_PREFIX).and_then(|n| n.parse::<usize>().ok()) {
                    Some(n) => number(readings.cores.get(n).copied()),
                    None => return Err(PluginError::Failed(format!("the sensors plugin does not own '{other}'"))),
                },
            };
            // A reading the machine does not have (no battery) stays unknown.
            if let Some(value) = value {
                ctx.vars.refresh(name, value)?;
            }
        }
        Ok(())
    }

    fn invoke(&mut self, action: &str, _args: &HashMap<String, String>, _ctx: &mut Ctx<'_>) -> Result<Outcome, PluginError> {
        Err(PluginError::UnknownAction { plugin: SENSORS_PLUGIN.to_string(), action: action.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    fn sample(plugin: &mut Sensors, vars: &VariableStore, names: &[&str]) -> Result<(), PluginError> {
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx { input: &mut input, vars, notifier: &mut notifier, settings: &settings, now_ms: 0 };
        plugin.sample(names, &mut ctx)
    }

    #[test]
    fn readings_become_values_and_cores_are_declared_once_seen() {
        let mut plugin = Sensors::new(Box::new(FixedSensors(Readings {
            memory_used_percent: Some(62.5),
            cores: vec![10.0, 90.0],
            battery_percent: None,
            network_down: Some(2048.0),
            ..Readings::default()
        })));
        let vars = VariableStore::new();
        vars.declare_all(plugin.variables());
        sample(&mut plugin, &vars, &["sensors.memory.used", "sensors.network.down", "sensors.battery.level"]).unwrap();
        assert_eq!(vars.get("sensors.memory.used"), Some(Value::Number(62.5)));
        assert_eq!(vars.get("sensors.network.down"), Some(Value::Number(2048.0)));
        assert_eq!(vars.get("sensors.battery.level"), None, "no battery: unknown, not zero");

        assert!(vars.spec("sensors.cpu.core.1").is_some(), "declared once read");
        sample(&mut plugin, &vars, &["sensors.cpu.core.1"]).unwrap();
        assert_eq!(vars.get("sensors.cpu.core.1"), Some(Value::Number(90.0)));
        assert_eq!(plugin.variables().len(), VARIABLES.len() + 2);
    }

    #[test]
    fn without_sensors_it_says_so() {
        let mut plugin = Sensors::new(Box::new(UnavailableSensors));
        let vars = VariableStore::new();
        vars.declare_all(plugin.variables());
        assert!(sample(&mut plugin, &vars, &["sensors.memory.used"]).is_err());
    }
}

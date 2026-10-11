//! The `time` plugin: the clock, as variables.
//!
//! A label template such as `"{{time.hhmm}}"` is then a digital clock with no
//! plugin to install. Values are read from `Ctx::now_ms`, the same clock the
//! rest of the agent runs on, in the machine's local time zone; the zone is
//! injected so a test pins it.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, FixedOffset, Offset, TimeZone, Timelike};
use flexdeck_vars::{Kind, Value, VariableSpec};

use crate::{Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const TIME_PLUGIN: &str = "time";

static TIME_MANIFEST: PluginManifest = PluginManifest {
    id: TIME_PLUGIN,
    name: "Time",
    version: "1.0.0",
    description: "The date and time, for labels: \"{{time.hhmm}}\".",
    actions: &[],
    settings: &[],
};

/// Every variable: its name, kind and description.
const VARIABLES: &[(&str, Kind, &str)] = &[
    ("time.hhmm", Kind::Text, "24-hour time, \"09:41\""),
    ("time.hhmmss", Kind::Text, "24-hour time with seconds, \"09:41:07\""),
    ("time.h12mm", Kind::Text, "12-hour time, \"9:41\""),
    ("time.ampm", Kind::Text, "\"AM\" or \"PM\""),
    ("time.hh", Kind::Text, "The hour, 00-23"),
    ("time.mm", Kind::Text, "The minute, 00-59"),
    ("time.ss", Kind::Text, "The second, 00-59"),
    ("time.hour", Kind::Number, "The hour as a number, 0-23"),
    ("time.minute", Kind::Number, "The minute as a number, 0-59"),
    ("time.second", Kind::Number, "The second as a number, 0-59"),
    ("time.date", Kind::Text, "The date, \"2026-10-11\""),
    ("time.day", Kind::Number, "The day of the month, 1-31"),
    ("time.month", Kind::Text, "The month's name, \"October\""),
    ("time.year", Kind::Number, "The year"),
    ("time.weekday", Kind::Text, "The day's name, \"Sunday\""),
    ("time.iso", Kind::Text, "Local date and time with its offset, ISO 8601"),
    ("time.epoch", Kind::Number, "Seconds since 1970, UTC"),
];

/// Where the local offset comes from.
#[derive(Debug, Clone, Copy)]
enum Zone {
    /// The machine's zone, looked up at each sample so a DST change or a
    /// changed system zone shows without a restart.
    Local,
    Fixed(FixedOffset),
}

#[derive(Debug)]
pub struct Time {
    zone: Zone,
}

impl Time {
    pub fn new() -> Self {
        Self { zone: Zone::Local }
    }

    /// A clock pinned to one offset. For tests.
    pub fn fixed(offset: FixedOffset) -> Self {
        Self {
            zone: Zone::Fixed(offset),
        }
    }

    fn at(&self, now_ms: i64) -> Result<DateTime<FixedOffset>, PluginError> {
        let utc = chrono::Utc
            .timestamp_millis_opt(now_ms)
            .single()
            .ok_or_else(|| PluginError::Failed(format!("{now_ms} is not a time")))?;
        let offset = match self.zone {
            Zone::Local => chrono::Local
                .offset_from_utc_datetime(&utc.naive_utc())
                .fix(),
            Zone::Fixed(offset) => offset,
        };
        Ok(utc.with_timezone(&offset))
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

/// The value of `name` at `at`, or `None` when `name` is not a time variable.
fn value(name: &str, at: &DateTime<FixedOffset>) -> Option<Value> {
    let text = |format: &str| Some(Value::Text(at.format(format).to_string()));
    let number = |n: i64| Some(Value::Number(n as f64));
    match name {
        "time.hhmm" => text("%H:%M"),
        "time.hhmmss" => text("%H:%M:%S"),
        "time.h12mm" => text("%-I:%M"),
        "time.ampm" => text("%p"),
        "time.hh" => text("%H"),
        "time.mm" => text("%M"),
        "time.ss" => text("%S"),
        "time.hour" => number(at.hour().into()),
        "time.minute" => number(at.minute().into()),
        "time.second" => number(at.second().into()),
        "time.date" => text("%Y-%m-%d"),
        "time.day" => number(at.day().into()),
        "time.month" => text("%B"),
        "time.year" => number(at.year().into()),
        "time.weekday" => text("%A"),
        "time.iso" => text("%Y-%m-%dT%H:%M:%S%:z"),
        "time.epoch" => number(at.timestamp()),
        _ => None,
    }
}

impl Plugin for Time {
    fn manifest(&self) -> crate::abi::ManifestDto {
        TIME_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        VARIABLES
            .iter()
            .map(|(name, kind, description)| {
                VariableSpec::new(*name, *kind, *description).read_only()
            })
            .collect()
    }

    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let at = self.at(ctx.now_ms)?;
        for name in wanted {
            let value = value(name, &at).ok_or_else(|| {
                PluginError::Failed(format!("the time plugin does not own '{name}'"))
            })?;
            ctx.vars.refresh(name, value)?;
        }
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        _args: &HashMap<String, String>,
        _ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        Err(PluginError::UnknownAction {
            plugin: TIME_PLUGIN.to_string(),
            action: action.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    /// 2026-10-11 21:05:09 UTC, a Sunday.
    const NOW_MS: i64 = 1_791_752_709_000;

    fn sample(plugin: &mut Time, names: &[&str]) -> VariableStore {
        let vars = VariableStore::new();
        vars.declare_all(plugin.variables());
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx {
            input: &mut input,
            vars: &vars,
            notifier: &mut notifier,
            settings: &settings,
            now_ms: NOW_MS,
        };
        plugin.sample(names, &mut ctx).unwrap();
        vars
    }

    fn text(vars: &VariableStore, name: &str) -> String {
        vars.get(name).unwrap().as_text().unwrap().to_string()
    }

    #[test]
    fn every_variable_samples_in_the_given_zone() {
        // UTC+05:30 makes it Monday 02:35:09 — the date rolls over too.
        let mut plugin = Time::fixed(FixedOffset::east_opt(5 * 3600 + 1800).unwrap());
        let names: Vec<&str> = VARIABLES.iter().map(|(name, _, _)| *name).collect();
        let vars = sample(&mut plugin, &names);

        assert_eq!(text(&vars, "time.hhmm"), "02:35");
        assert_eq!(text(&vars, "time.hhmmss"), "02:35:09");
        assert_eq!(text(&vars, "time.h12mm"), "2:35");
        assert_eq!(text(&vars, "time.ampm"), "AM");
        assert_eq!(text(&vars, "time.date"), "2026-10-12");
        assert_eq!(text(&vars, "time.weekday"), "Monday");
        assert_eq!(text(&vars, "time.month"), "October");
        assert_eq!(text(&vars, "time.iso"), "2026-10-12T02:35:09+05:30");
        assert_eq!(vars.get("time.hour").unwrap().as_f64(), Some(2.0));
        assert_eq!(vars.get("time.epoch").unwrap().as_f64(), Some(1_791_752_709.0));
    }

    #[test]
    fn afternoon_reads_pm_on_the_twelve_hour_clock() {
        let mut plugin = Time::fixed(FixedOffset::east_opt(0).unwrap());
        let vars = sample(&mut plugin, &["time.h12mm", "time.ampm", "time.weekday"]);
        assert_eq!(text(&vars, "time.h12mm"), "9:05");
        assert_eq!(text(&vars, "time.ampm"), "PM");
        assert_eq!(text(&vars, "time.weekday"), "Sunday");
    }

    #[test]
    fn a_name_it_does_not_own_is_refused() {
        let mut plugin = Time::fixed(FixedOffset::east_opt(0).unwrap());
        let vars = VariableStore::new();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx {
            input: &mut input,
            vars: &vars,
            notifier: &mut notifier,
            settings: &settings,
            now_ms: NOW_MS,
        };
        assert!(plugin.sample(&["time.fortnight"], &mut ctx).is_err());
    }
}

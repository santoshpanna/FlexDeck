//! The document: what an automation *is*, and what may be stored.
//!
//! Nothing here runs. The model is separate from the evaluator so that a
//! document can be parsed, checked and refused **before** anything executes —
//! including the structural checks that stop a hostile or merely broken
//! document from being a memory problem the moment it is deserialised.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Limits;

/// The document version this build understands.
///
/// Bump this whenever a field or block kind is added. The reader refuses a
/// *newer* schema with a sentence a person can act on, which is the rule this
/// repository already wrote down for the config contract: "say 'this needs a
/// newer agent' instead of failing obscurely". It matters more here, because
/// this document is hand-built JSON rather than a pbjson-checked message.
///
/// History: `2` added the `pluginEvent` and `schedule` triggers; `3` added the
/// `input` value reference (a slider's position, an event's payload). Older
/// documents are still read as-is, since everything in them means the same.
pub const AUTOMATION_SCHEMA: u32 = 3;

/// A value in a document: either written down, or read from a variable.
///
/// Variable references are the whole reason this type exists. A widget's action
/// arguments are plain strings in the config contract, so a button can only ever
/// press `ctrl+shift+m`. An automation can say "switch to whatever
/// `system.audio.output_device` currently is", which is not expressible as a
/// string and is exactly what the feature is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValueRef {
    Bool(bool),
    Number(f64),
    Text(String),
    /// The name of a variable to read at run time.
    Variable(String),
    /// A value this run was handed when it started — `value` for a slider,
    /// a payload field for a plugin event. Unknown in a run without it.
    Input(String),
}

/// How two values are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompareOp {
    Eq,
    NotEq,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    /// Text only: does the left side contain the right?
    Contains,
}

/// A test, in the shape the visual editor draws it.
///
/// The tag is `kind`, not `condition`: `Not { condition }` would collide with an
/// internal tag of that name, which serde rejects outright.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Condition {
    Compare {
        left: ValueRef,
        op: CompareOp,
        right: ValueRef,
    },
    /// Every condition must hold.
    All {
        conditions: Vec<Condition>,
    },
    /// At least one condition must hold.
    Any {
        conditions: Vec<Condition>,
    },
    Not {
        condition: Box<Condition>,
    },
}

/// One "else if" arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub condition: Condition,
    pub blocks: Vec<Block>,
}

/// A step in an automation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "block", rename_all = "camelCase")]
pub enum Block {
    If {
        condition: Condition,
        #[serde(rename = "then")]
        then_blocks: Vec<Block>,
        #[serde(default, rename = "elseIf")]
        else_if: Vec<Branch>,
        #[serde(default)]
        otherwise: Vec<Block>,
    },
    Repeat {
        times: ValueRef,
        #[serde(default)]
        body: Vec<Block>,
    },
    Wait {
        ms: ValueRef,
    },
    SetVariable {
        name: String,
        value: ValueRef,
    },
    DoAction {
        plugin: String,
        action: String,
        #[serde(default)]
        args: BTreeMap<String, ValueRef>,
    },
}

/// What makes an automation start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "camelCase")]
pub enum Trigger {
    /// Fires when any of these variables changes.
    Variables { names: Vec<String> },
    /// Fires when this widget is pressed.
    Widget {
        #[serde(rename = "widgetId")]
        widget_id: String,
    },
    /// Fires only when asked to, which is what the editor's Run button does.
    Manual,
    /// Fires when a plugin announces an event — OBS's `scene_changed`, say.
    ///
    /// Every key in `filter` must equal the same key in the event's payload,
    /// so "when the scene changes *to Live*" is `{"scene": "Live"}`.
    PluginEvent {
        plugin: String,
        event: String,
        #[serde(default)]
        filter: BTreeMap<String, String>,
    },
    /// Fires on a clock: every `everySeconds`, or daily at `at` ("HH:MM",
    /// local time) on `days` (`mon`..`sun`; empty means every day).
    Schedule {
        #[serde(
            default,
            rename = "everySeconds",
            skip_serializing_if = "Option::is_none"
        )]
        every_seconds: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        days: Vec<String>,
    },
}

/// The shortest schedule interval. Anything quicker is a loop, and the
/// variable trigger is the right tool for "as soon as it changes".
pub const MIN_SCHEDULE_SECONDS: u64 = 5;

/// The wall clock, as the schedule trigger needs it. Passed in rather than
/// read, so the trigger is testable at any moment without waiting for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// Seconds since the Unix epoch.
    pub epoch_secs: i64,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
}

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

fn parse_at(at: &str) -> Option<(u8, u8)> {
    let (hour, minute) = at.trim().split_once(':')?;
    let hour: u8 = hour.parse().ok()?;
    let minute: u8 = minute.parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

impl Trigger {
    /// The variables this trigger needs watched, so the sampler knows to keep
    /// them fresh. A trigger that watches nothing costs nothing.
    pub fn watched(&self) -> Vec<String> {
        match self {
            Trigger::Variables { names } => names.clone(),
            _ => Vec::new(),
        }
    }

    pub fn watches(&self, name: &str) -> bool {
        matches!(self, Trigger::Variables { names } if names.iter().any(|watched| watched == name))
    }

    pub fn fires_for_widget(&self, widget_id: &str) -> bool {
        matches!(self, Trigger::Widget { widget_id: watched } if watched == widget_id)
    }

    pub fn fires_for_event(
        &self,
        plugin: &str,
        event: &str,
        payload: &BTreeMap<String, String>,
    ) -> bool {
        match self {
            Trigger::PluginEvent {
                plugin: wanted_plugin,
                event: wanted_event,
                filter,
            } => {
                wanted_plugin == plugin
                    && wanted_event == event
                    && filter
                        .iter()
                        .all(|(key, value)| payload.get(key) == Some(value))
            }
            _ => false,
        }
    }

    pub fn is_schedule(&self) -> bool {
        matches!(self, Trigger::Schedule { .. })
    }

    /// Whether a schedule should fire now, given when it last fired.
    ///
    /// An interval counts from the last firing, and from the moment it was
    /// first seen — so saving an automation does not fire it immediately. A
    /// daily time fires once in its minute, however often this is asked.
    pub fn schedule_due(&self, last_fired: Option<i64>, now: LocalTime) -> bool {
        let Trigger::Schedule {
            every_seconds,
            at,
            days,
        } = self
        else {
            return false;
        };
        if let Some(every) = every_seconds {
            return last_fired.is_some_and(|last| now.epoch_secs - last >= *every as i64);
        }
        let Some((hour, minute)) = at.as_deref().and_then(parse_at) else {
            return false;
        };
        let today = DAYS[usize::from(now.weekday.min(6))];
        let on_day = days.is_empty() || days.iter().any(|day| day.eq_ignore_ascii_case(today));
        let in_minute = now.hour == hour && now.minute == minute;
        let fired_this_minute = last_fired.is_some_and(|last| now.epoch_secs - last < 60);
        on_day && in_minute && !fired_this_minute
    }

    /// Refuse a trigger that could never fire, or would fire too often.
    pub fn validate(&self) -> Result<(), DocumentError> {
        match self {
            Trigger::Schedule {
                every_seconds,
                at,
                days,
            } => {
                match (every_seconds, at) {
                    (Some(every), None) if *every >= MIN_SCHEDULE_SECONDS => {}
                    (Some(every), None) => {
                        return Err(DocumentError::Invalid(format!(
                            "a schedule must be at least {MIN_SCHEDULE_SECONDS} seconds apart, not {every}"
                        )))
                    }
                    (None, Some(at)) if parse_at(at).is_some() => {}
                    (None, Some(at)) => {
                        return Err(DocumentError::Invalid(format!(
                            "'{at}' is not a time of day (use HH:MM)"
                        )))
                    }
                    _ => {
                        return Err(DocumentError::Invalid(
                            "a schedule needs exactly one of 'everySeconds' or 'at'".into(),
                        ))
                    }
                }
                if let Some(day) = days
                    .iter()
                    .find(|day| !DAYS.iter().any(|known| day.eq_ignore_ascii_case(known)))
                {
                    return Err(DocumentError::Invalid(format!(
                        "'{day}' is not a day (use {})",
                        DAYS.join(", ")
                    )));
                }
                Ok(())
            }
            Trigger::PluginEvent { plugin, event, .. }
                if plugin.trim().is_empty() || event.trim().is_empty() =>
            {
                Err(DocumentError::Invalid(
                    "a plugin-event trigger needs a plugin and an event".into(),
                ))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod trigger_tests {
    use super::*;

    fn at(epoch_secs: i64, weekday: u8, hour: u8, minute: u8) -> LocalTime {
        LocalTime {
            epoch_secs,
            weekday,
            hour,
            minute,
        }
    }

    #[test]
    fn an_event_trigger_matches_plugin_event_and_every_filter_key() {
        let trigger = Trigger::PluginEvent {
            plugin: "obs".into(),
            event: "scene_changed".into(),
            filter: [("scene".to_string(), "Live".to_string())].into(),
        };
        let live: BTreeMap<String, String> = [("scene".to_string(), "Live".to_string())].into();
        let other: BTreeMap<String, String> = [("scene".to_string(), "BRB".to_string())].into();
        assert!(trigger.fires_for_event("obs", "scene_changed", &live));
        assert!(!trigger.fires_for_event("obs", "scene_changed", &other));
        assert!(!trigger.fires_for_event("obs", "record_state_changed", &live));
        assert!(!trigger.fires_for_event("other", "scene_changed", &live));
    }

    #[test]
    fn an_interval_waits_a_full_interval_from_when_it_was_first_seen() {
        let trigger = Trigger::Schedule {
            every_seconds: Some(60),
            at: None,
            days: vec![],
        };
        assert!(
            !trigger.schedule_due(None, at(1_000, 0, 0, 0)),
            "never on sight"
        );
        assert!(!trigger.schedule_due(Some(1_000), at(1_059, 0, 0, 0)));
        assert!(trigger.schedule_due(Some(1_000), at(1_060, 0, 0, 0)));
    }

    #[test]
    fn a_daily_time_fires_once_in_its_minute_on_its_days() {
        let trigger = Trigger::Schedule {
            every_seconds: None,
            at: Some("09:30".into()),
            days: vec!["mon".into(), "fri".into()],
        };
        assert!(trigger.schedule_due(None, at(10_000, 0, 9, 30)));
        assert!(
            !trigger.schedule_due(Some(10_000), at(10_030, 0, 9, 30)),
            "once"
        );
        assert!(
            !trigger.schedule_due(None, at(10_000, 1, 9, 30)),
            "not tuesday"
        );
        assert!(!trigger.schedule_due(None, at(10_000, 0, 9, 31)));
    }

    #[test]
    fn an_impossible_or_runaway_schedule_is_refused() {
        let bad = [
            (Some(1), None, vec![]),
            (None, Some("25:00"), vec![]),
            (None, Some("9am"), vec![]),
            (None, None, vec![]),
            (Some(60), Some("09:00"), vec![]),
            (None, Some("09:00"), vec!["someday"]),
        ];
        for (every, time, days) in bad {
            let trigger = Trigger::Schedule {
                every_seconds: every,
                at: time.map(str::to_string),
                days: days.into_iter().map(str::to_string).collect(),
            };
            assert!(trigger.validate().is_err(), "{trigger:?} must be refused");
        }
    }

    #[test]
    fn the_new_triggers_have_the_documented_wire_shape() {
        let json = r#"{"on":"schedule","everySeconds":30}"#;
        let trigger: Trigger = serde_json::from_str(json).expect("parses");
        assert_eq!(serde_json::to_string(&trigger).unwrap(), json);
        let json = r#"{"on":"pluginEvent","plugin":"obs","event":"scene_changed","filter":{}}"#;
        let trigger: Trigger = serde_json::from_str(json).expect("parses");
        assert_eq!(serde_json::to_string(&trigger).unwrap(), json);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: String,
    pub name: String,
    /// A disabled automation is kept, listed, and never run — the state a user
    /// wants while editing one that is misbehaving.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub trigger: Trigger,
    #[serde(default)]
    pub body: Vec<Block>,
}

fn default_enabled() -> bool {
    true
}

/// The stored document: everything the user has authored.
///
/// `schema` is checked before deserialising, and `rev` is stamped by the agent —
/// never trusted from a client, for the reason the config already learned: two
/// sources of truth for one number always drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDocument {
    pub schema: u32,
    #[serde(default)]
    pub rev: u64,
    #[serde(default)]
    pub automations: Vec<Automation>,
}

impl Default for AutomationDocument {
    fn default() -> Self {
        Self {
            schema: AUTOMATION_SCHEMA,
            rev: 0,
            automations: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DocumentError {
    #[error("the automation document is not valid JSON: {0}")]
    Invalid(String),

    #[error(
        "this document needs a newer agent (it is schema {found}, this build reads {supported})"
    )]
    TooNew { found: u32, supported: u32 },

    #[error("the automation document is too big: {0}")]
    TooBig(String),
}

/// Read a document, refusing it clearly if this build is too old.
///
/// The schema is read from the raw JSON *before* the rest is deserialised,
/// because the interesting failure is a document written by a newer build whose
/// unknown block kind would otherwise fail as a parse error naming a field the
/// user never wrote.
pub fn parse_document(json: &str, limits: &Limits) -> Result<AutomationDocument, DocumentError> {
    let raw: serde_json::Value =
        serde_json::from_str(json).map_err(|error| DocumentError::Invalid(error.to_string()))?;

    let found = raw
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if found > AUTOMATION_SCHEMA {
        return Err(DocumentError::TooNew {
            found,
            supported: AUTOMATION_SCHEMA,
        });
    }

    let document: AutomationDocument =
        serde_json::from_value(raw).map_err(|error| DocumentError::Invalid(error.to_string()))?;

    document.validate(limits)?;
    Ok(document)
}

impl AutomationDocument {
    /// Refuse a document that would be a problem merely to *hold*.
    ///
    /// The run-time bounds cannot help here: a document with a hundred thousand
    /// nested conditions is a memory problem during deserialisation, long before
    /// any step budget is consulted.
    pub fn validate(&self, limits: &Limits) -> Result<(), DocumentError> {
        let mut count = 0;
        for automation in &self.automations {
            automation.trigger.validate()?;
            check_conditions_in_blocks(&automation.body, 1, limits, &mut count)?;
        }
        Ok(())
    }
}

/// Refuse a block list that would be a problem merely to hold — the same
/// bounds a whole automation document gets. A widget's flows are checked with
/// this, so a flow on a button and a flow in an automation obey one rule.
pub fn validate_blocks(blocks: &[Block], limits: &Limits) -> Result<(), DocumentError> {
    let mut count = 0;
    check_conditions_in_blocks(blocks, 1, limits, &mut count)
}

fn check_conditions_in_blocks(
    blocks: &[Block],
    depth: usize,
    limits: &Limits,
    count: &mut usize,
) -> Result<(), DocumentError> {
    if depth > limits.max_depth {
        return Err(DocumentError::TooBig(format!(
            "blocks are nested {} deep, and the limit is {}",
            depth, limits.max_depth
        )));
    }

    for block in blocks {
        *count += 1;
        if *count > limits.max_blocks {
            return Err(DocumentError::TooBig(format!(
                "more than {} blocks",
                limits.max_blocks
            )));
        }

        match block {
            Block::If {
                condition,
                then_blocks,
                else_if,
                otherwise,
            } => {
                check_condition(condition, depth + 1, limits)?;
                check_conditions_in_blocks(then_blocks, depth + 1, limits, count)?;
                for branch in else_if {
                    check_condition(&branch.condition, depth + 1, limits)?;
                    check_conditions_in_blocks(&branch.blocks, depth + 1, limits, count)?;
                }
                check_conditions_in_blocks(otherwise, depth + 1, limits, count)?;
            }
            Block::Repeat { body, .. } => {
                check_conditions_in_blocks(body, depth + 1, limits, count)?;
            }
            Block::Wait { .. } | Block::SetVariable { .. } | Block::DoAction { .. } => {}
        }
    }

    Ok(())
}

fn check_condition(
    condition: &Condition,
    depth: usize,
    limits: &Limits,
) -> Result<(), DocumentError> {
    if depth > limits.max_depth {
        return Err(DocumentError::TooBig(format!(
            "conditions are nested {} deep, and the limit is {}",
            depth, limits.max_depth
        )));
    }

    match condition {
        Condition::Compare { .. } => Ok(()),
        Condition::All { conditions } | Condition::Any { conditions } => {
            for nested in conditions {
                check_condition(nested, depth + 1, limits)?;
            }
            Ok(())
        }
        Condition::Not { condition } => check_condition(condition, depth + 1, limits),
    }
}

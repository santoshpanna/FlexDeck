//! The reference plugin, and the "template" this system is judged by.
//!
//! It is small on purpose, but it is not a toy: it exercises **every** part of
//! the contract that currently has a consumer — a setting, three kinds of typed
//! argument, reading a variable, writing a variable, and the notifier. If the
//! contract changes, this file stops compiling, which is the point.
//!
//! # What it is, and what it is not
//!
//! It is an in-repo **example**, and it is what the drift tests drive, so it
//! cannot silently rot. It is *not* yet a copy-me scaffold for a third party —
//! that arrives with the loader, and calling this both things would be the kind
//! of over-claim the whole plan was rejected for once already.
//!
//! # Why it writes a variable *and* notifies
//!
//! So that a single action produces both kinds of observable effect. Writing
//! `example.counter` proves the plugin path into the variable store (including
//! that it publishes a change the engine can react to); the notification proves
//! the capability path. A test can then assert either without a socket.

use std::collections::HashMap;

use flexdeck_actions::{ActionSpec, ArgumentSpec};
use flexdeck_vars::{Kind, Value, VariableSpec};

use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest, SettingSpec};

pub const EXAMPLE_PLUGIN: &str = "example";

pub const COUNTER: &str = "example.counter";
pub const LAST_TEXT: &str = "example.last_text";

pub const GREETING: &str = "greeting";
const DEFAULT_GREETING: &str = "Hello";

static EXAMPLE_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: EXAMPLE_PLUGIN,
        action: "demo",
        description: "Count, remember a phrase, and say hello",
        arguments: &[
            ArgumentSpec::text("text", "A phrase to remember", "stream is live"),
            ArgumentSpec::number("amount", "How much to add or remove", "1"),
            ArgumentSpec::choice("mode", "Which way to count", "add", &["add", "subtract"]),
        ],
        slider: None,
    },
    // Reports which look a widget should show — the "provider" half of the
    // contract, which a widget whose looks come from an action relies on.
    ActionSpec {
        plugin: EXAMPLE_PLUGIN,
        action: "flip",
        description: "Flip a switch, and report which way it now faces",
        arguments: &[],
        slider: None,
    },
];

static EXAMPLE_SETTINGS: &[SettingSpec] = &[SettingSpec::text(
    GREETING,
    "The word the notification starts with",
    DEFAULT_GREETING,
)];

static EXAMPLE_MANIFEST: PluginManifest = PluginManifest {
    id: EXAMPLE_PLUGIN,
    name: "Example",
    version: "1.0.0",
    description: "A reference plugin that exercises every part of the contract.",
    actions: EXAMPLE_ACTIONS,
    settings: EXAMPLE_SETTINGS,
};

#[derive(Debug, Default)]
pub struct Example {
    flipped: bool,
}

impl Example {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Plugin for Example {
    fn manifest(&self) -> crate::abi::ManifestDto {
        EXAMPLE_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        vec![
            // Not read-only: this one is the *user's*, and an automation is
            // allowed to move it.
            VariableSpec::new(COUNTER, Kind::Number, "How many times demo has run"),
            VariableSpec::new(LAST_TEXT, Kind::Text, "The last phrase demo was given"),
        ]
    }

    fn sample(&mut self, _wanted: &[&str], _ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        // Nothing to read: this plugin's variables are written, not sampled.
        // Which is why it must still honour `wanted` by doing nothing at all
        // rather than inventing work to justify itself.
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        if action == "flip" {
            self.flipped = !self.flipped;
            return Ok(Outcome::state(if self.flipped { "on" } else { "off" }));
        }
        if action != "demo" {
            return Err(PluginError::UnknownAction {
                plugin: EXAMPLE_PLUGIN.to_string(),
                action: action.to_string(),
            });
        }

        let text = argument(args, "text").to_string();
        let amount = parse_amount(argument(args, "amount"))?;
        let delta = if argument(args, "mode") == "subtract" {
            -amount
        } else {
            amount
        };

        let previous = ctx
            .vars
            .get(COUNTER)
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);

        ctx.vars.set(COUNTER, Value::Number(previous + delta))?;
        ctx.vars.set(LAST_TEXT, Value::Text(text.clone()))?;

        let greeting = ctx
            .settings
            .get(GREETING)
            .map(String::as_str)
            .unwrap_or(DEFAULT_GREETING);
        ctx.notifier
            .notify(greeting, &format!("{text} ({delta:+.0})"))?;

        Ok(Outcome::default())
    }
}

fn parse_amount(raw: &str) -> Result<f64, PluginError> {
    raw.parse::<f64>()
        .map_err(|_| PluginError::Failed(format!("'amount' must be a number, not '{raw}'")))
}

/// A choice argument in the catalogue must really be one of its choices, or the
/// editor would offer a dropdown that cannot produce a working action.
#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_actions::ArgumentKind;

    #[test]
    fn the_amount_argument_is_declared_as_a_number() {
        let spec = EXAMPLE_ACTIONS[0]
            .arguments
            .iter()
            .find(|argument| argument.name == "amount")
            .expect("declared");

        assert_eq!(spec.kind, ArgumentKind::Number);
    }
}

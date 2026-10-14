//! The flexdeck action engine.
//!
//! # Why this crate splits resolving from performing
//!
//! Turning `{plugin, action, args}` from the configuration into something
//! executable is pure logic: validation, parsing, error reporting. It can be
//! tested exhaustively and deterministically.
//!
//! *Performing* an action means injecting keystrokes into whatever window the
//! user currently has focused. That is neither safe nor possible in a test.
//!
//! So the two halves are separated by the [`Executor`] trait:
//!
//! * [`RecordingExecutor`] writes actions down. Tests use it.
//! * [`EnigoExecutor`] really types. Only the agent binary uses it.
//!
//! That boundary is deliberate: it means no test in this workspace can
//! accidentally type into a developer's desktop, no matter how it is run.

use flexdeck_protocol::Action;

pub mod enigo_executor;
pub mod hotkey;

pub use enigo_executor::EnigoExecutor;
pub use hotkey::{Hotkey, HotkeyParseError, KeyName, Modifier};

/// Plugin id for the actions built into the agent.
pub const CORE_PLUGIN: &str = "core";

/// What kind of value an argument carries.
///
/// The catalogue is what the editor builds its form from, so a number typed as
/// text means the user gets a free-text box for a volume and can type `loud`
/// into it. Declaring the kind is what makes a picker possible at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentKind {
    Text,
    Number,
    Bool,
    /// One of a fixed set. Carries `choices` on the [`ArgumentSpec`].
    Choice,
}

impl ArgumentKind {
    /// The name this kind travels under on the wire.
    ///
    /// Kept here rather than derived so the wire spelling is one decision in one
    /// place, and the REST layer cannot invent a second one.
    pub fn as_str(self) -> &'static str {
        match self {
            ArgumentKind::Text => "text",
            ArgumentKind::Number => "number",
            ArgumentKind::Bool => "bool",
            ArgumentKind::Choice => "choice",
        }
    }
}

/// One argument an action takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// A value that works, used to prefill the portal's form and to keep the
    /// catalogue honest — see the tests.
    pub example: &'static str,
    /// The value's type, so the editor can render the right control.
    pub kind: ArgumentKind,
    /// The permitted values when `kind` is [`ArgumentKind::Choice`].
    pub choices: &'static [&'static str],
    /// May be left blank. Required unless marked otherwise, because a blank
    /// required value is a mistake the editor should catch before a press.
    pub optional: bool,
}

impl ArgumentSpec {
    /// A free-text argument.
    pub const fn text(
        name: &'static str,
        description: &'static str,
        example: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            example,
            kind: ArgumentKind::Text,
            choices: &[],
            optional: false,
        }
    }

    /// A numeric argument.
    pub const fn number(
        name: &'static str,
        description: &'static str,
        example: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            example,
            kind: ArgumentKind::Number,
            choices: &[],
            optional: false,
        }
    }

    /// An argument constrained to one of a fixed set of strings.
    pub const fn choice(
        name: &'static str,
        description: &'static str,
        example: &'static str,
        choices: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            description,
            example,
            kind: ArgumentKind::Choice,
            choices,
            optional: false,
        }
    }

    /// The same argument, allowed to be blank.
    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// One action the built-in plugin offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec {
    pub plugin: &'static str,
    pub action: &'static str,
    pub description: &'static str,
    pub arguments: &'static [ArgumentSpec],
    /// Set when a slider widget can drive this action.
    pub slider: Option<SliderSpec>,
    /// Set when a key running it can show whether it is in effect.
    pub active: Option<ActiveSpec>,
}

/// When a key running an action is lit — the static form of the ABI's
/// `ActiveDto`. `variable` may hold `{{argument}}` placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveSpec {
    pub variable: &'static str,
    /// Lit while the variable holds this argument's value.
    pub equals: Option<&'static str>,
    /// Lit while the variable holds this value; `"true"` when neither is set.
    pub is: Option<&'static str>,
}

impl ActiveSpec {
    /// Lit while a true/false `variable` is true.
    pub const fn while_true(variable: &'static str) -> Self {
        Self { variable, equals: None, is: None }
    }

    /// Lit while `variable` holds what the `argument` was set to.
    pub const fn equals(variable: &'static str, argument: &'static str) -> Self {
        Self { variable, equals: Some(argument), is: None }
    }
}

/// How a slider drives an action — the static form of the ABI's `SliderDto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderSpec {
    /// The argument the slider's value is written into.
    pub argument: &'static str,
    pub min: i64,
    pub max: i64,
    pub step: i64,
    /// The variable reporting the live value, with `{{argument}}` placeholders
    /// filled from the action's other arguments.
    pub feedback: Option<&'static str>,
    /// How the same channel is muted: a toggle action, and the variable
    /// (a template, like `feedback`) saying whether it is.
    pub mute: Option<SliderMute>,
}

/// The static form of the ABI's `SliderMuteDto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderMute {
    pub action: &'static str,
    pub feedback: &'static str,
}

impl SliderSpec {
    /// A 0-100 slider writing `argument`, following `feedback`.
    pub const fn percent(argument: &'static str, feedback: &'static str) -> Self {
        Self {
            argument,
            min: 0,
            max: 100,
            step: 1,
            feedback: Some(feedback),
            mute: None,
        }
    }

    /// The same slider, mutable by `action`, muted while `feedback` is true.
    pub const fn with_mute(mut self, action: &'static str, feedback: &'static str) -> Self {
        self.mute = Some(SliderMute { action, feedback });
        self
    }
}

/// Everything the built-in plugin can do.
///
/// The portal builds its property form from this rather than hard-coding the
/// list, which is the same principle the plugin system needs for third-party
/// actions: an action added here becomes editable with no change to the portal.
///
/// It is data, not a trait, because it has to survive a trip over the API as
/// JSON — a description that can only be computed in Rust is one the portal
/// cannot show.
pub const CORE_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: CORE_PLUGIN,
        action: "hotkey",
        description: "Press a keyboard shortcut",
        arguments: &[ArgumentSpec::text(
            "keys",
            "Modifiers and a key, like ctrl+shift+m",
            "ctrl+shift+m",
        )],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: CORE_PLUGIN,
        action: "text",
        description: "Type some text",
        arguments: &[ArgumentSpec::text("text", "The text to type", "hello")],
        slider: None,
        active: None,
    },
];

/// What a resolved action should actually do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreAction {
    /// Press a keyboard accelerator.
    Hotkey(Hotkey),
    /// Type literal text.
    Text(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("action has no plugin configured")]
    MissingPlugin,

    #[error("plugin '{0}' is not available (only the built-in 'core' plugin is)")]
    UnknownPlugin(String),

    #[error("the 'core' plugin has no action called '{0}'")]
    UnknownAction(String),

    #[error("action '{action}' needs the argument '{argument}'")]
    MissingArgument {
        action: String,
        argument: &'static str,
    },

    #[error("could not parse the hotkey: {0}")]
    Hotkey(#[from] HotkeyParseError),

    #[error("could not perform the action: {0}")]
    Execution(String),
}

/// Resolve a configured [`Action`] into something executable.
///
/// This is the entire validation surface for actions. An unknown plugin, an
/// unknown action, or a missing or blank argument is a configuration mistake
/// the portal should show the user — not a button that silently does nothing
/// when pressed.
pub fn resolve(action: &Action) -> Result<CoreAction, ActionError> {
    if action.plugin.trim().is_empty() {
        return Err(ActionError::MissingPlugin);
    }
    if action.plugin != CORE_PLUGIN {
        return Err(ActionError::UnknownPlugin(action.plugin.clone()));
    }

    match action.action.as_str() {
        "hotkey" => {
            let keys = argument(action, "keys")?;
            Ok(CoreAction::Hotkey(Hotkey::parse(keys)?))
        }
        "text" => {
            let text = argument(action, "text")?;
            Ok(CoreAction::Text(text.to_string()))
        }
        other => Err(ActionError::UnknownAction(other.to_string())),
    }
}

/// Fetch a required argument, treating blank as absent.
///
/// A key that exists but is empty (`"keys": ""`) is the same mistake as a
/// missing key, and deserves the same error.
fn argument<'a>(action: &'a Action, name: &'static str) -> Result<&'a str, ActionError> {
    action
        .args
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ActionError::MissingArgument {
            action: action.action.clone(),
            argument: name,
        })
}

/// Performs resolved actions.
///
/// Implementations may inject real input. Nothing in a test should ever hold an
/// implementation that does.
pub trait Executor {
    fn perform(&mut self, action: &CoreAction) -> Result<(), ActionError>;
}

/// Records actions instead of performing them.
///
/// Useful in tests, and later for the portal's "test press" mode, which should
/// show what a button *would* do without doing it.
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    performed: Vec<CoreAction>,
}

impl RecordingExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn performed(&self) -> &[CoreAction] {
        &self.performed
    }

    pub fn clear(&mut self) {
        self.performed.clear();
    }
}

impl Executor for RecordingExecutor {
    fn perform(&mut self, action: &CoreAction) -> Result<(), ActionError> {
        self.performed.push(action.clone());
        Ok(())
    }
}

/// Resolve and perform a single action, returning what ran.
pub fn run(action: &Action, executor: &mut dyn Executor) -> Result<CoreAction, ActionError> {
    let resolved = resolve(action)?;
    executor.perform(&resolved)?;
    Ok(resolved)
}

/// Run actions in order, stopping at the first failure.
///
/// Returns everything that was performed, so a caller can report partial
/// progress rather than just "it failed".
pub fn run_sequence(
    actions: &[Action],
    executor: &mut dyn Executor,
) -> Result<Vec<CoreAction>, ActionError> {
    let mut performed = Vec::with_capacity(actions.len());
    for action in actions {
        performed.push(run(action, executor)?);
    }
    Ok(performed)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn action(plugin: &str, name: &str, pairs: &[(&str, &str)]) -> Action {
        Action {
            plugin: plugin.to_string(),
            action: name.to_string(),
            args: args(pairs),
        }
    }

    fn hotkey_action(keys: &str) -> Action {
        action(CORE_PLUGIN, "hotkey", &[("keys", keys)])
    }

    #[test]
    fn every_catalogued_action_resolves_with_its_own_example() {
        // The catalogue drives the portal's form. If it drifted from `resolve`,
        // the portal would offer an action that cannot work, and the user would
        // discover that by pressing the button.
        for spec in CORE_ACTIONS {
            let args: Vec<(&str, &str)> = spec
                .arguments
                .iter()
                .map(|argument| (argument.name, argument.example))
                .collect();

            let configured = action(spec.plugin, spec.action, &args);

            assert!(
                resolve(&configured).is_ok(),
                "'{}' is catalogued, so its own example must resolve, but it gave {:?}",
                spec.action,
                resolve(&configured).unwrap_err()
            );
        }
    }

    #[test]
    fn every_action_resolve_accepts_is_in_the_catalogue() {
        // The other direction: an action the agent can run but the portal cannot
        // offer is simply invisible to the user.
        for name in ["hotkey", "text"] {
            assert!(
                CORE_ACTIONS.iter().any(|spec| spec.action == name),
                "'{name}' resolves but is not catalogued"
            );
        }
    }

    #[test]
    fn the_catalogue_declares_every_argument_an_action_requires() {
        // Each catalogued action, called with no arguments at all, must fail on a
        // *missing argument* — and that argument has to be one the catalogue
        // declares. Otherwise the portal would render a form that cannot produce
        // a valid action.
        for spec in CORE_ACTIONS {
            let unconfigured = action(spec.plugin, spec.action, &[]);

            match resolve(&unconfigured) {
                Err(ActionError::MissingArgument { argument, .. }) => assert!(
                    spec.arguments
                        .iter()
                        .any(|declared| declared.name == argument),
                    "'{}' requires '{argument}' but the catalogue does not declare it",
                    spec.action
                ),
                other => panic!(
                    "expected '{}' to require an argument, but it gave {other:?}",
                    spec.action
                ),
            }
        }
    }

    #[test]
    fn a_catalogued_action_still_refuses_a_blank_argument() {
        // Cataloguing an argument must not weaken validation: the portal can
        // render an empty field, and that has to stay an error.
        let blank = action(CORE_PLUGIN, "hotkey", &[("keys", "   ")]);
        assert!(matches!(
            resolve(&blank),
            Err(ActionError::MissingArgument { .. })
        ));
    }

    #[test]
    fn the_hotkey_example_in_the_catalogue_parses() {
        let example = CORE_ACTIONS
            .iter()
            .find(|spec| spec.action == "hotkey")
            .and_then(|spec| spec.arguments.first())
            .expect("a hotkey argument");

        assert!(
            Hotkey::parse(example.example).is_ok(),
            "{} does not parse",
            example.example
        );
    }

    #[test]
    fn resolves_the_hotkey_example_from_the_prd() {
        let resolved = resolve(&hotkey_action("ctrl+shift+m")).expect("resolves");
        assert_eq!(
            resolved,
            CoreAction::Hotkey(Hotkey::new(
                vec![Modifier::Ctrl, Modifier::Shift],
                KeyName::Char('m')
            ))
        );
    }

    #[test]
    fn resolves_text_actions() {
        let resolved = resolve(&action(CORE_PLUGIN, "text", &[("text", "hello")]));
        assert_eq!(resolved, Ok(CoreAction::Text("hello".to_string())));
    }

    #[test]
    fn a_missing_plugin_is_reported() {
        assert_eq!(
            resolve(&action("", "hotkey", &[("keys", "m")])),
            Err(ActionError::MissingPlugin)
        );
    }

    #[test]
    fn a_blank_plugin_is_treated_as_missing() {
        assert_eq!(
            resolve(&action("   ", "hotkey", &[("keys", "m")])),
            Err(ActionError::MissingPlugin)
        );
    }

    #[test]
    fn an_uninstalled_plugin_is_named_in_the_error() {
        assert_eq!(
            resolve(&action("obs", "scene", &[])),
            Err(ActionError::UnknownPlugin("obs".to_string()))
        );
    }

    #[test]
    fn an_unknown_core_action_is_named_in_the_error() {
        assert_eq!(
            resolve(&action(CORE_PLUGIN, "launch_rocket", &[])),
            Err(ActionError::UnknownAction("launch_rocket".to_string()))
        );
    }

    #[test]
    fn a_missing_argument_is_reported_with_both_names() {
        assert_eq!(
            resolve(&action(CORE_PLUGIN, "hotkey", &[])),
            Err(ActionError::MissingArgument {
                action: "hotkey".to_string(),
                argument: "keys",
            })
        );
    }

    #[test]
    fn a_blank_argument_counts_as_missing() {
        // An empty string in the config is a mistake, not an instruction to
        // press the empty key.
        assert_eq!(
            resolve(&action(CORE_PLUGIN, "hotkey", &[("keys", "   ")])),
            Err(ActionError::MissingArgument {
                action: "hotkey".to_string(),
                argument: "keys",
            })
        );
    }

    #[test]
    fn an_unparseable_hotkey_surfaces_the_parser_error() {
        let error = resolve(&hotkey_action("ctrl+banana")).expect_err("should fail");
        assert_eq!(
            error,
            ActionError::Hotkey(HotkeyParseError::UnknownKey("banana".to_string()))
        );
    }

    #[test]
    fn running_performs_exactly_the_resolved_action() {
        let mut executor = RecordingExecutor::new();

        let performed = run(&hotkey_action("ctrl+shift+m"), &mut executor).expect("runs");

        assert_eq!(
            performed,
            CoreAction::Hotkey(Hotkey::new(
                vec![Modifier::Ctrl, Modifier::Shift],
                KeyName::Char('m')
            ))
        );
        assert_eq!(executor.performed().len(), 1);
    }

    #[test]
    fn a_failing_action_is_never_performed() {
        let mut executor = RecordingExecutor::new();

        let result = run(&hotkey_action("ctrl+banana"), &mut executor);

        assert!(result.is_err());
        // The important part: resolution failed, so nothing was executed.
        assert!(executor.performed().is_empty());
    }

    #[test]
    fn a_sequence_runs_in_order() {
        let mut executor = RecordingExecutor::new();
        let sequence = vec![
            hotkey_action("ctrl+m"),
            action(CORE_PLUGIN, "text", &[("text", "typed")]),
        ];

        let performed = run_sequence(&sequence, &mut executor).expect("runs");

        assert_eq!(
            performed,
            vec![
                CoreAction::Hotkey(Hotkey::new(vec![Modifier::Ctrl], KeyName::Char('m'))),
                CoreAction::Text("typed".to_string()),
            ]
        );
    }

    #[test]
    fn a_sequence_stops_at_the_first_failure_and_reports_what_ran() {
        let mut executor = RecordingExecutor::new();
        let sequence = vec![
            hotkey_action("ctrl+m"),
            hotkey_action("ctrl+banana"),
            hotkey_action("ctrl+n"),
        ];

        let error = run_sequence(&sequence, &mut executor).expect_err("should stop");

        assert_eq!(
            error,
            ActionError::Hotkey(HotkeyParseError::UnknownKey("banana".to_string()))
        );
        // Only the first action ran; the third never got a chance.
        assert_eq!(executor.performed().len(), 1);
    }

    #[test]
    fn recording_executor_can_be_cleared() {
        let mut executor = RecordingExecutor::new();
        run(&hotkey_action("ctrl+m"), &mut executor).expect("runs");
        assert_eq!(executor.performed().len(), 1);

        executor.clear();
        assert!(executor.performed().is_empty());
    }
}

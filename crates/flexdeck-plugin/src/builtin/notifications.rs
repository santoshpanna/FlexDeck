//! The `notifications` plugin: show the person at the machine a message.
//!
//! The smallest useful plugin, and deliberately so: it has no variables, no
//! settings and no state, so it exercises the contract with nothing else in the
//! way. Whether a notification actually *appears* is a property of the injected
//! [`Notifier`](crate::Notifier) — which is why this plugin never learns which
//! platform it is on, and why a test can assert on it.

use std::collections::HashMap;

use flexdeck_actions::{ActionSpec, ArgumentSpec};
use flexdeck_vars::VariableSpec;

use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const NOTIFICATIONS_PLUGIN: &str = "notifications";

static NOTIFICATIONS_ACTIONS: &[ActionSpec] = &[ActionSpec {
    plugin: NOTIFICATIONS_PLUGIN,
    action: "notify",
    description: "Show a notification",
    arguments: &[
        ArgumentSpec::text("title", "The heading", "Not going live"),
        ArgumentSpec::text("body", "The message", "CPU load is high"),
    ],
    slider: None,
    active: None,
}];

static NOTIFICATIONS_MANIFEST: PluginManifest = PluginManifest {
    id: NOTIFICATIONS_PLUGIN,
    name: "Notifications",
    version: "1.0.0",
    description: "Show a desktop notification.",
    actions: NOTIFICATIONS_ACTIONS,
    settings: &[],
};

#[derive(Debug, Default)]
pub struct Notifications;

impl Notifications {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for Notifications {
    fn manifest(&self) -> crate::abi::ManifestDto {
        NOTIFICATIONS_MANIFEST.to_dto()
    }

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
        match action {
            "notify" => {
                ctx.notifier
                    .notify(argument(args, "title"), argument(args, "body"))?;
                Ok(Outcome::default())
            }
            other => Err(PluginError::UnknownAction {
                plugin: NOTIFICATIONS_PLUGIN.to_string(),
                action: other.to_string(),
            }),
        }
    }
}

/// Shows a real desktop notification: a toast on Windows, the notification
/// centre on macOS, the freedesktop service on Linux.
///
/// Never constructed by a test, for the same reason as `EnigoExecutor`.
#[derive(Debug, Default)]
pub struct DesktopNotifier;

impl crate::Notifier for DesktopNotifier {
    fn notify(&mut self, title: &str, body: &str) -> Result<(), PluginError> {
        notify_rust::Notification::new()
            .appname("flexdeck")
            .summary(title)
            .body(body)
            .show()
            .map(drop)
            .map_err(|error| PluginError::Failed(format!("could not show a notification: {error}")))
    }
}

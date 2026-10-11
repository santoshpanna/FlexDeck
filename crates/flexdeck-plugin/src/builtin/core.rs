//! The `core` plugin: keyboard shortcuts and typed text.
//!
//! This is the action set that predates the plugin system, and it is wrapped
//! rather than rewritten. It still owns the only engine that injects input
//! ([`flexdeck_actions::Executor`]), and it still uses the same
//! `flexdeck_actions::CORE_ACTIONS` catalogue the REST surface has always
//! served — one list of core actions, not two that can drift.

use std::collections::HashMap;

use flexdeck_actions::{CORE_ACTIONS, CORE_PLUGIN};
use flexdeck_vars::VariableSpec;

use crate::{Ctx, Outcome, Plugin, PluginError, PluginManifest};

static CORE_MANIFEST: PluginManifest = PluginManifest {
    id: CORE_PLUGIN,
    name: "Core",
    version: "1.0.0",
    description: "Press keyboard shortcuts and type text.",
    actions: CORE_ACTIONS,
    settings: &[],
};

#[derive(Debug, Default)]
pub struct Core;

impl Core {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for Core {
    fn manifest(&self) -> crate::abi::ManifestDto {
        CORE_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        Vec::new()
    }

    /// Parse the accelerator here, so a mistyped shortcut fails the whole
    /// sequence rather than after an earlier action has already run.
    fn check(&self, action: &str, args: &HashMap<String, String>) -> Result<(), PluginError> {
        flexdeck_actions::resolve(&configured(action, args))?;
        Ok(())
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
        // The registry has already checked the action exists, its arguments are
        // present, and the value parses. This resolves once more to get the
        // concrete `CoreAction` to report; resolution is pure and cheap.
        let resolved = flexdeck_actions::resolve(&configured(action, args))?;
        ctx.input.perform(&resolved)?;
        Ok(Some(resolved).into())
    }
}

fn configured(action: &str, args: &HashMap<String, String>) -> flexdeck_protocol::Action {
    flexdeck_protocol::Action {
        plugin: CORE_PLUGIN.to_string(),
        action: action.to_string(),
        args: args.clone(),
    }
}

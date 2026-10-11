//! Recipes: plugins made without code.
//!
//! A recipe package (`"runtime": "recipe"`) is a `plugin.json` and a
//! `recipe.json` of web data sources and gallery presets. Nothing is run but
//! the agent's own source engine: no process, no Node, no program a recipe
//! could smuggle in — a recipe may only *read the web*, and must declare the
//! `network` permission to, which the person agrees to on install.
//!
//! To the rest of the agent a recipe is an ordinary installed plugin: it is
//! listed, enabled, disabled, updated and removed like one, and its presets
//! join the gallery. Removing it takes its sources with it.

use std::path::Path;
use std::sync::Arc;

use flexdeck_plugin::{abi, Ctx, Origin, Outcome, Plugin, PluginError};
use flexdeck_vars::VariableSpec;

use crate::sources::{validate_recipe, RecipeDoc, Sources};

#[derive(Debug)]
pub struct RecipePlugin {
    manifest: abi::ManifestDto,
    sources: Arc<Sources>,
}

impl RecipePlugin {
    /// Read, check and start the recipe in the package at `root`.
    pub fn load(root: &Path, package: &abi::PackageManifest, sources: Arc<Sources>) -> Result<Self, String> {
        let file = package.recipe.as_deref().ok_or("the package names no recipe file")?;
        if !abi::is_contained_path(file) {
            return Err(format!("'{file}' is not a path inside the package"));
        }
        let text = std::fs::read_to_string(root.join(file)).map_err(|error| format!("{file}: {error}"))?;
        let recipe = parse(&package.id, &text)?;
        if !recipe.sources.is_empty() && !package.permissions.iter().any(|p| p == abi::permission::NETWORK) {
            return Err("a recipe that reads the web must declare the network permission".into());
        }
        let manifest = abi::ManifestDto {
            abi: package.abi,
            id: package.id.clone(),
            name: package.name.clone(),
            version: package.version.clone(),
            description: package.description.clone(),
            author: package.author.clone(),
            actions: Vec::new(),
            settings: Vec::new(),
            events: Vec::new(),
            permissions: package.permissions.clone(),
            uses: if recipe.presets.is_empty() { Vec::new() } else { vec![abi::capability::PRESETS.to_string()] },
            presets: recipe.presets,
            feeds: Vec::new(),
        };
        manifest.validate().map_err(|error| error.to_string())?;
        sources.add_recipe(&package.id, recipe.sources);
        Ok(Self { manifest, sources })
    }
}

/// A recipe's text as a checked recipe: its sources could run, and each
/// preset is a widget this agent accepts.
pub fn parse(id: &str, text: &str) -> Result<RecipeDoc, String> {
    let recipe: RecipeDoc = serde_json::from_str(text).map_err(|error| format!("the recipe is not readable: {error}"))?;
    validate_recipe(id, &recipe)?;
    for preset in &recipe.presets {
        let mut widget = preset.widget.clone();
        widget["id"] = serde_json::Value::String(format!("{id}-{}", preset.id));
        crate::widgets::check_widget(&widget).map_err(|problem| format!("preset '{}': {problem}", preset.id))?;
    }
    Ok(recipe)
}

impl Plugin for RecipePlugin {
    fn manifest(&self) -> abi::ManifestDto {
        self.manifest.clone()
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
        _args: &std::collections::HashMap<String, String>,
        _ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        Err(PluginError::UnknownAction { plugin: self.manifest.id.clone(), action: action.to_string() })
    }

    fn origin(&self) -> Origin {
        Origin::Installed
    }

    fn shutdown(&mut self) {
        self.sources.remove_recipe(&self.manifest.id);
    }
}

impl Drop for RecipePlugin {
    fn drop(&mut self) {
        self.sources.remove_recipe(&self.manifest.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_vars::VariableStore;
    use serde_json::json;

    fn package(permissions: &[&str]) -> abi::PackageManifest {
        abi::PackageManifest {
            abi: 4,
            id: "crypto".into(),
            name: "Crypto".into(),
            version: "1.0.0".into(),
            description: String::new(),
            author: String::new(),
            homepage: None,
            runtime: abi::runtime::RECIPE.into(),
            entry: None,
            entries: Default::default(),
            permissions: permissions.iter().map(|p| p.to_string()).collect(),
            recipe: Some("recipe.json".into()),
        }
    }

    fn write(dir: &Path, recipe: serde_json::Value) {
        std::fs::write(dir.join("recipe.json"), recipe.to_string()).unwrap();
    }

    /// Every recipe the repository ships (`recipes/`) loads as an install would.
    #[test]
    fn every_shipped_recipe_loads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes");
        let mut loaded = 0;
        for entry in std::fs::read_dir(&root).expect("recipes/") {
            let dir = entry.unwrap().path();
            let text = std::fs::read_to_string(dir.join(abi::PACKAGE_MANIFEST)).unwrap();
            let package: abi::PackageManifest = serde_json::from_str(&text).unwrap();
            package.validate().unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            let sources = Arc::new(Sources::new(Arc::new(VariableStore::new())));
            let plugin = RecipePlugin::load(&dir, &package, Arc::clone(&sources)).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            assert!(!plugin.manifest().presets.is_empty(), "{}: a recipe brings a key to the gallery", dir.display());
            loaded += 1;
        }
        assert!(loaded >= 4, "weather, crypto, calendar, home-assistant");
    }

    #[test]
    fn a_recipe_brings_its_sources_and_presets_and_takes_them_away() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), json!({
            "sources": [{"id": "crypto", "http": {"url": "https://api.example.com/{{args.coin}}"}, "fields": [{"name": "price", "value": "json.price"}]}],
            "presets": [{"id": "ticker", "name": "Ticker", "widget": {"sources": [{"source": "crypto"}], "face": {"elements": [{"template": "{{ this.price | currency }}"}]}}}]
        }));
        let sources = Arc::new(Sources::new(Arc::new(VariableStore::new())));
        let plugin = RecipePlugin::load(dir.path(), &package(&["network"]), Arc::clone(&sources)).unwrap();
        assert_eq!(plugin.manifest().presets[0].name, "Ticker");
        assert_eq!(sources.recipes()["crypto"][0].id, "crypto");
        drop(plugin);
        assert!(sources.recipes().is_empty(), "gone with the recipe");
    }

    #[test]
    fn a_recipe_may_only_read_the_web_under_its_own_name_with_permission() {
        let dir = tempfile::tempdir().unwrap();
        let sources = Arc::new(Sources::new(Arc::new(VariableStore::new())));
        let refused = |recipe: serde_json::Value, permissions: &[&str]| {
            write(dir.path(), recipe);
            RecipePlugin::load(dir.path(), &package(permissions), Arc::clone(&sources)).unwrap_err()
        };
        let web = json!({"sources": [{"id": "crypto", "http": {"url": "https://x"}}]});
        assert!(refused(web, &[]).contains("network permission"));
        assert!(refused(json!({"sources": [{"id": "crypto", "kind": "command", "command": {"program": "calc"}}]}), &["network"]).contains("not run a program"));
        assert!(refused(json!({"sources": [{"id": "weather", "http": {"url": "https://x"}}]}), &["network"]).contains("named after it"));
        assert!(refused(json!({"presets": [{"id": "p", "name": "P", "widget": {"face": {"rules": [{"when": "x >"}]}}}]}), &[]).contains("preset 'p'"));
    }
}

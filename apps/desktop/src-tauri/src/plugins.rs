//! What the window can do with plugins: install, update, remove, configure,
//! browse the marketplace, and manage the repositories it lists.
//!
//! Everything runs in-process against the agent's own plugin manager, for the
//! same reason the rest of the window does: no bearer token or self-signed
//! certificate in the webview. Files arrive as bytes the webview read from a
//! file input, so the window needs no filesystem permission to install one.
//!
//! Installing runs new code on this machine, so it is a two-step affair: the
//! window asks what a package *is* (name, author, permissions), shows that to
//! the person, and only then installs with the permissions they accepted. A
//! package whose permissions were not all accepted is refused by the manager,
//! not merely by the UI.

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::Engine as _;
use serde_json::{json, Value};
use tauri::State;

use flexdeck_server::plugins::{PluginManager, PluginsError, Source};
use flexdeck_server::repos::Repositories;

use crate::state::ShellState;

fn manager(state: &ShellState) -> Result<Arc<PluginManager>, String> {
    state
        .agent
        .plugins()
        .cloned()
        .ok_or_else(|| "this agent does not run installed plugins".to_string())
}

fn repositories(state: &ShellState) -> Result<Arc<Repositories>, String> {
    state
        .agent
        .repositories()
        .cloned()
        .ok_or_else(|| "this agent does not run installed plugins".to_string())
}

fn installed(state: &ShellState) -> Vec<flexdeck_server::Installed> {
    state
        .agent
        .plugins()
        .map(|manager| manager.installed())
        .unwrap_or_default()
}

/// Run blocking plugin work off the window's thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

fn decode(bytes: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(bytes.trim())
        .map_err(|error| format!("the file could not be read: {error}"))
}

/// An install outcome the window can render: installed, or "ask first".
fn outcome(result: Result<flexdeck_server::Installed, PluginsError>) -> Result<Value, String> {
    match result {
        Ok(installed) => Ok(json!({ "installed": installed })),
        Err(PluginsError::Permissions {
            missing,
            package,
            sha256,
        }) => Ok(json!({
            "needsConsent": true,
            "missing": missing,
            "package": package,
            "sha256": sha256,
        })),
        Err(error) => Err(error.to_string()),
    }
}

/// What a package file is, before anything is installed.
#[tauri::command]
pub async fn plugin_inspect(state: State<'_, ShellState>, bytes: String) -> Result<Value, String> {
    let manager = manager(&state)?;
    blocking(move || {
        let inspected = manager
            .inspect(&decode(&bytes)?)
            .map_err(|e| e.to_string())?;
        serde_json::to_value(inspected).map_err(|e| e.to_string())
    })
    .await
}

/// Install a package file with the permissions the person accepted.
#[tauri::command]
pub async fn plugin_install_file(
    state: State<'_, ShellState>,
    bytes: String,
    accepted: Vec<String>,
) -> Result<Value, String> {
    let manager = manager(&state)?;
    let agent = Arc::clone(&state.agent);
    let result = blocking(move || {
        let bytes = decode(&bytes)?;
        outcome(manager.install(&bytes, None, &accepted, Source::File))
    })
    .await;
    agent.behavior().catalogue_changed();
    result
}

/// Install from one of this desktop's repositories. The checksum comes from
/// that repository's index.
#[tauri::command]
pub async fn plugin_install_repo(
    state: State<'_, ShellState>,
    repo: String,
    id: String,
    accepted: Vec<String>,
) -> Result<Value, String> {
    let manager = manager(&state)?;
    let repos = repositories(&state)?;
    let agent = Arc::clone(&state.agent);
    let result = blocking(move || {
        let (bytes, sha256, source) = repos.fetch_package(&repo, &id).map_err(|e| e.to_string())?;
        outcome(manager.install(&bytes, Some(&sha256), &accepted, source))
    })
    .await;
    agent.behavior().catalogue_changed();
    result
}

/// The marketplace: every plugin the enabled repositories list, refreshed
/// when due, with the repositories themselves.
#[tauri::command]
pub async fn plugin_catalog(state: State<'_, ShellState>, query: String) -> Result<Value, String> {
    let repos = repositories(&state)?;
    let installed = installed(&state);
    blocking(move || {
        repos.refresh(false);
        Ok(json!({
            "plugins": repos.catalog(&query, &installed),
            "repositories": repos.list(),
            "vettedError": repos.vetted_error(),
        }))
    })
    .await
}

/// Fetch every repository now, whatever their age.
#[tauri::command]
pub async fn repos_refresh(state: State<'_, ShellState>) -> Result<Value, String> {
    let repos = repositories(&state)?;
    blocking(move || {
        repos.refresh(true);
        Ok(json!({ "repositories": repos.list() }))
    })
    .await
}

/// What adding this address would add, before anything is saved.
#[tauri::command]
pub async fn repo_preview(state: State<'_, ShellState>, url: String) -> Result<Value, String> {
    let repos = repositories(&state)?;
    blocking(move || {
        let preview = repos.preview(&url).map_err(|e| e.to_string())?;
        serde_json::to_value(preview).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn repo_add(state: State<'_, ShellState>, url: String) -> Result<Value, String> {
    let repos = repositories(&state)?;
    blocking(move || {
        let row = repos.add(&url).map_err(|e| e.to_string())?;
        serde_json::to_value(row).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn repo_remove(state: State<'_, ShellState>, url: String) -> Result<(), String> {
    let repos = repositories(&state)?;
    blocking(move || repos.remove(&url).map_err(|e| e.to_string())).await
}

/// Turn a vetted repository on or off.
#[tauri::command]
pub async fn repo_toggle(
    state: State<'_, ShellState>,
    url: String,
    enabled: bool,
) -> Result<(), String> {
    let repos = repositories(&state)?;
    blocking(move || {
        repos
            .set_vetted_enabled(&url, enabled)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn plugin_uninstall(state: State<'_, ShellState>, id: String) -> Result<(), String> {
    let manager = manager(&state)?;
    let agent = Arc::clone(&state.agent);
    blocking(move || manager.uninstall(&id).map_err(|e| e.to_string())).await?;
    agent.behavior().catalogue_changed();
    Ok(())
}

#[tauri::command]
pub async fn plugin_enable(
    state: State<'_, ShellState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let manager = manager(&state)?;
    let agent = Arc::clone(&state.agent);
    blocking(move || manager.set_enabled(&id, enabled).map_err(|e| e.to_string())).await?;
    agent.behavior().catalogue_changed();
    Ok(())
}

#[tauri::command]
pub async fn plugin_reload(state: State<'_, ShellState>, id: String) -> Result<(), String> {
    let manager = manager(&state)?;
    let agent = Arc::clone(&state.agent);
    blocking(move || manager.reload(&id).map_err(|e| e.to_string())).await?;
    agent.behavior().catalogue_changed();
    Ok(())
}

/// Run a plugin from a folder, reloading it on every save.
#[tauri::command]
pub async fn plugin_link(state: State<'_, ShellState>, path: String) -> Result<(), String> {
    let manager = manager(&state)?;
    let agent = Arc::clone(&state.agent);
    blocking(move || {
        manager
            .link_dev(std::path::Path::new(path.trim()))
            .map(drop)
            .map_err(|e| e.to_string())
    })
    .await?;
    agent.behavior().catalogue_changed();
    Ok(())
}

/// Save a plugin's settings. A secret left blank keeps what was stored.
#[tauri::command]
pub async fn plugin_settings(
    state: State<'_, ShellState>,
    id: String,
    values: BTreeMap<String, String>,
) -> Result<(), String> {
    let agent = Arc::clone(&state.agent);
    blocking(move || {
        agent
            .set_plugin_settings(&id, values)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn plugin_log(state: State<'_, ShellState>, id: String) -> Result<Vec<String>, String> {
    Ok(manager(&state)?.log(&id))
}

/// Every plugin, as the window shows it: built-ins with their action counts,
/// and installed plugins with their state, permissions and settings form.
pub fn rows(agent: &flexdeck_server::Agent) -> Value {
    let catalogue = agent.behavior().plugin_catalogue();
    let entries: Vec<Value> = catalogue["plugins"].as_array().cloned().unwrap_or_default();
    let by_id = |id: &str| entries.iter().find(|entry| entry["id"] == id).cloned();

    let built_in: Vec<Value> = entries
        .iter()
        .filter(|entry| entry["origin"] == "builtIn")
        .map(|entry| {
            json!({
                "id": entry["id"],
                "plugin": entry["name"],
                "version": entry["version"],
                "actions": entry["actions"].as_array().map_or(0, Vec::len),
                "status": entry["status"],
            })
        })
        .collect();

    let installed: Vec<Value> = agent
        .plugins()
        .map(|manager| {
            manager.installed_json()["plugins"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|mut record| {
            let id = record["id"].as_str().unwrap_or_default().to_string();
            if let Some(entry) = by_id(&id) {
                record["actions"] = json!(entry["actions"].as_array().map_or(0, Vec::len));
                record["settings"] = entry["settings"].clone();
                record["description"] = entry["description"].clone();
                record["events"] = entry["events"].clone();
            }
            record
        })
        .collect();

    json!({ "builtIn": built_in, "installed": installed })
}

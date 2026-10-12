//! How the window behaves around the agent: whether it opens at sign-in,
//! whether it opens at all, and what closing or minimizing it does.
//!
//! These belong to the window, not the agent, so they live beside it — a small
//! JSON file in the app's config folder — rather than in the agent's settings,
//! which the command-line binary shares and has no use for. Starting at sign-in
//! is not stored here at all: the OS entry *is* the setting, so the toggle can
//! never claim one that was removed behind its back.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

const FILE: &str = "window.json";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowPrefs {
    /// Open to the tray rather than showing the window.
    pub start_minimized: bool,

    /// Closing the window hides it and keeps the agent serving. Off, closing
    /// the window quits — and takes every connected deck down with it.
    pub close_to_tray: bool,

    /// Minimizing the window hides it to the tray instead of the taskbar.
    pub minimize_to_tray: bool,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            start_minimized: false,
            close_to_tray: true,
            minimize_to_tray: false,
        }
    }
}

/// The preferences in force, shared with the close handler.
pub struct PrefsState(pub Mutex<WindowPrefs>);

impl PrefsState {
    pub fn get(&self) -> WindowPrefs {
        *self.0.lock().expect("the prefs lock is not poisoned")
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(FILE))
}

/// The saved preferences, or the defaults. An unreadable file is not worth
/// refusing to open over: the worst case is the window showing once.
pub fn load(app: &AppHandle) -> WindowPrefs {
    path(app)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, prefs: &WindowPrefs) -> Result<(), String> {
    let path = path(app).ok_or("this computer has no config folder for the app")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(prefs).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

fn view(app: &AppHandle, prefs: WindowPrefs) -> Value {
    json!({
        "startMinimized": prefs.start_minimized,
        "closeToTray": prefs.close_to_tray,
        "minimizeToTray": prefs.minimize_to_tray,
        // Read from the OS each time, so it reflects reality, not a memory of it.
        "autostart": app.autolaunch().is_enabled().unwrap_or(false),
    })
}

/// How the window starts, closes and minimizes.
#[tauri::command]
pub async fn window_prefs(app: AppHandle, prefs: State<'_, PrefsState>) -> Result<Value, String> {
    Ok(view(&app, prefs.get()))
}

/// Change any of them; whatever is left out stays as it was.
#[tauri::command]
pub async fn set_window_prefs(
    app: AppHandle,
    prefs: State<'_, PrefsState>,
    start_minimized: Option<bool>,
    close_to_tray: Option<bool>,
    minimize_to_tray: Option<bool>,
    autostart: Option<bool>,
) -> Result<Value, String> {
    if let Some(enabled) = autostart {
        let launcher = app.autolaunch();
        if enabled {
            launcher.enable()
        } else {
            launcher.disable()
        }
        .map_err(|error| error.to_string())?;
    }

    let mut next = prefs.get();
    if let Some(value) = start_minimized {
        next.start_minimized = value;
    }
    if let Some(value) = close_to_tray {
        next.close_to_tray = value;
    }
    if let Some(value) = minimize_to_tray {
        next.minimize_to_tray = value;
    }
    save(&app, &next)?;
    *prefs.0.lock().expect("the prefs lock is not poisoned") = next;

    Ok(view(&app, next))
}

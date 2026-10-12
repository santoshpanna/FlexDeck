//! Tell the window when there is something new to read.
//!
//! One event with no payload. The window re-reads everything on it, so there is
//! no incremental update that can drift out of step with the agent — the same
//! reasoning the agent's own change channel uses when it announces a revision
//! instead of broadcasting a patch.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast::error::RecvError;

use flexdeck_server::{Agent, WebAccess};

pub const STATE_EVENT: &str = "shell://state";

pub fn spawn(app: AppHandle, agent: Arc<Agent>, web_access: Arc<WebAccess>) {
    forward_config(app.clone(), Arc::clone(&agent));
    forward_presence(app.clone(), Arc::clone(&agent));
    forward_devices(app.clone(), Arc::clone(&agent));
    forward_plugins(app.clone(), Arc::clone(&agent));
    forward_approvals(app.clone(), Arc::clone(&agent));
    forward_tunnel(app, web_access);
}

/// A browser elsewhere asking for something that needs a yes here.
///
/// The window is brought forward as well as told: the request waits only
/// briefly, and a prompt in a hidden window is a prompt nobody answers.
fn forward_approvals(app: AppHandle, agent: Arc<Agent>) {
    let mut changes = agent.approvals().subscribe();

    tauri::async_runtime::spawn(async move {
        while let Ok(_) | Err(RecvError::Lagged(_)) = changes.recv().await {
            if !agent.approvals().pending().is_empty() {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            announce(&app);
        }
    });
}

/// A configuration edit, from the website or from anywhere else.
fn forward_config(app: AppHandle, agent: Arc<Agent>) {
    let mut changes = agent.subscribe();

    tauri::async_runtime::spawn(async move {
        // Lagging is harmless: the window re-reads the current state rather than
        // applying what it missed. A closed channel ends the loop.
        while let Ok(_) | Err(RecvError::Lagged(_)) = changes.recv().await {
            announce(&app);
        }
    });
}

/// A plugin installed, removed, restarted, crashed, or reconfigured.
fn forward_plugins(app: AppHandle, agent: Arc<Agent>) {
    let mut changes = agent.behavior().subscribe_catalogue();

    tauri::async_runtime::spawn(async move {
        while let Ok(_) | Err(RecvError::Lagged(_)) = changes.recv().await {
            announce(&app);
        }
    });
}

/// A tablet connecting or going away.
fn forward_presence(app: AppHandle, agent: Arc<Agent>) {
    let mut changes = agent.presence().changed();

    tauri::async_runtime::spawn(async move {
        while let Ok(_) | Err(RecvError::Lagged(_)) = changes.recv().await {
            announce(&app);
        }
    });
}

/// A tablet or browser paired or revoked. Pairing is what closes the code on
/// the Pair tab, and a new tablet need not connect its socket straight away, so
/// presence alone cannot be relied on to say so.
fn forward_devices(app: AppHandle, agent: Arc<Agent>) {
    let mut changes = agent.subscribe_devices();

    tauri::async_runtime::spawn(async move {
        while let Ok(_) | Err(RecvError::Lagged(_)) = changes.recv().await {
            announce(&app);
        }
    });
}

/// The tunnel coming up or dropping.
fn forward_tunnel(app: AppHandle, access: Arc<WebAccess>) {
    let mut events = access.events();

    tauri::async_runtime::spawn(async move {
        while let Ok(_) | Err(RecvError::Lagged(_)) = events.recv().await {
            announce(&app);
        }
    });
}

/// Say that something changed.
///
/// Public because the listener can also fail to come up after start-up, and the
/// window needs to hear about that too.
pub fn announce(app: &AppHandle) {
    // A window that has gone away, or a webview that cannot receive, is not an
    // error worth reporting: the state it missed is re-read on the next change.
    let _ = app.emit(STATE_EVENT, ());
}

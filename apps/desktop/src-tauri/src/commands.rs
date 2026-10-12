//! What the window can ask of the agent.
//!
//! Every command returns JSON rather than a typed struct, on purpose: the shape
//! the window consumes is the same shape the agent already serves over HTTP, so
//! there is one description of "a device" and one description of "the status",
//! not two that can disagree.

use serde_json::{json, Value};
use tauri::State;

use crate::state::ShellState;

/// Everything the window shows, in one read.
///
/// One command rather than one per card, so the cards cannot be rendered from
/// different moments in time and quietly contradict each other.
#[tauri::command]
pub async fn status(state: State<'_, ShellState>) -> Result<Value, String> {
    let agent = &state.agent;
    let settings = &state.settings;

    let warnings: Vec<String> = agent
        .startup_warnings(state.bind)
        .into_iter()
        .map(|warning| warning.message().to_string())
        .collect();

    // ONE description of a tunnel, shared with the agent's own HTTP route. The
    // window used to assemble this shape itself, which is how a field could be
    // added to the route and not here — and the comment claiming otherwise sat
    // right above it.
    let tunnel = flexdeck_server::view(Some(&state.web_access))
        .map_err(|error| error.message().to_string())?;

    let presence = agent.presence();
    let devices: Vec<Value> = agent
        .devices()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|device| {
            let online = presence.is_online(&device.id);

            json!({
                "id": device.id,
                "name": device.name,
                // `created_at`, matching the agent's own API rather than
                // inventing a second spelling for the same value.
                "created_at": device.created_at,
                // Whether a socket is open right now, as opposed to whether the
                // device has ever paired.
                "online": online,
                // A tablet, a browser on this computer, or a browser elsewhere.
                "kind": device.kind,
            })
        })
        .collect();

    // The agent's own offer, not a copy: once a tablet uses the code it is
    // gone, and the window must stop showing it.
    let pairing = agent
        .pairing_offer_now()
        .map(|offer| json!({ "pin": offer.pin, "expiresAt": offer.expires_at }));

    let server_error = state
        .server_error
        .lock()
        .expect("the server lock is not poisoned")
        .clone();

    Ok(json!({
        "fingerprint": agent.identity().fingerprint(),
        "host": settings.advertised_host(),
        "port": settings.port,
        "warnings": warnings,
        "tunnel": tunnel,
        "devices": devices,
        // Whether anything outside this machine can reach it, and why not if
        // not. The window is the only place this can be seen.
        "server": { "error": server_error },
        "plugins": crate::plugins::rows(agent),
        "pairing": pairing,
        // The code for connecting a browser on another device, while one is open.
        "browserPairing": agent
            .browser_pairing_offer_now()
            .map(|offer| json!({
                "code": flexdeck_pairing::display_code(&offer.pin),
                "expiresAt": offer.expires_at,
            })),
        // What a browser elsewhere is asking to do, waiting for a yes or no here.
        "approvals": agent.approvals().pending(),
    }))
}

/// Show a code for connecting a browser on another device.
#[tauri::command]
pub async fn begin_browser_pairing(state: State<'_, ShellState>) -> Result<Value, String> {
    let offer = state.agent.begin_browser_pairing_now();
    Ok(json!({
        "code": flexdeck_pairing::display_code(&offer.pin),
        "expiresAt": offer.expires_at,
    }))
}

/// Withdraw the browser code being shown, so it can no longer be used.
#[tauri::command]
pub async fn cancel_browser_pairing(state: State<'_, ShellState>) -> Result<(), String> {
    state.agent.cancel_browser_pairing();
    Ok(())
}

/// Allow or refuse something a browser elsewhere asked to do.
#[tauri::command]
pub async fn answer_approval(
    state: State<'_, ShellState>,
    id: u64,
    allow: bool,
) -> Result<(), String> {
    if state.agent.approvals().answer(id, allow) {
        Ok(())
    } else {
        // It timed out, or was answered already; the list will refresh.
        Err("That request is no longer waiting.".to_string())
    }
}

/// Open a pairing window.
#[tauri::command]
pub async fn begin_pairing(state: State<'_, ShellState>) -> Result<Value, String> {
    let offer = state.agent.begin_pairing_now();
    Ok(json!({ "pin": offer.pin, "expiresAt": offer.expires_at }))
}

/// Withdraw the tablet PIN being shown, so it can no longer be used.
#[tauri::command]
pub async fn cancel_pairing(state: State<'_, ShellState>) -> Result<(), String> {
    state.agent.cancel_pairing();
    Ok(())
}

/// Turn web access on or off, then report the new state.
#[tauri::command]
pub async fn set_web_access(state: State<'_, ShellState>, enabled: bool) -> Result<Value, String> {
    if enabled {
        // Refused by the agent when there is no address to reach, with a reason
        // worth showing: a tunnel with nowhere to dial is not a tunnel.
        state
            .web_access
            .enable()
            .map_err(|error| error.to_string())?;
    } else {
        state.web_access.disable();
    }

    status(state).await
}

/// Point this installation at a portal; a blank address restores the default.
///
/// The portal address is the one thing the window could not change, and a wrong
/// or missing one is indistinguishable from a portal that is switched off — so
/// the only way forward was a command-line flag. That is the dead end this
/// closes.
///
/// The address is VALIDATED and SAVED first, so one this build refuses never
/// reaches the tunnel, and the choice survives a restart. Nothing here dials: an
/// address that turns out to be unreachable is reported by the status, not
/// refused by the window, because the window cannot know what the machine can
/// reach.
#[tauri::command]
pub async fn set_portal(state: State<'_, ShellState>, url: String) -> Result<Value, String> {
    let mut settings = state.settings.clone();
    let url = url.trim();

    settings.cloud_url = Some(if url.is_empty() {
        flexdeck_server::settings::DEFAULT_PORTAL_URL.to_string()
    } else {
        url.to_string()
    });

    state
        .agent
        .save_settings(&settings)
        .map_err(|error| error.to_string())?;

    state
        .web_access
        .set_portal(settings.cloud_url)
        .map_err(|error| error.to_string())?;

    status(state).await
}

/// Revoke a paired tablet or browser.
#[tauri::command]
pub async fn revoke_device(state: State<'_, ShellState>, device_id: String) -> Result<(), String> {
    if !state
        .agent
        .revoke_device(&device_id)
        .map_err(|error| error.to_string())?
    {
        // The list is what the user is looking at, so the refusal names it.
        return Err("That device is not on the list.".to_string());
    }

    // The tablet's open socket is closed within one probe interval by the socket
    // loop itself, which re-checks its token — so its dot goes out on its own.
    Ok(())
}

/// Revoke every paired tablet and browser. The window is not a device itself,
/// so nothing is kept back.
#[tauri::command]
pub async fn revoke_all_devices(state: State<'_, ShellState>) -> Result<(), String> {
    state
        .agent
        .revoke_all_devices(None)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Open a web page in the computer's own browser.
///
/// Only http and https: this hands the string to the OS, which would just as
/// happily run a file or a custom scheme.
#[tauri::command]
pub async fn open_link(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only web addresses can be opened".to_string());
    }
    open::that_detached(&url).map_err(|error| error.to_string())
}

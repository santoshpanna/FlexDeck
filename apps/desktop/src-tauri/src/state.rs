use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use flexdeck_agent::HostParts;
use flexdeck_server::{Agent, Settings, WebAccess};

/// Everything the window's commands need, held for the life of the window.
///
/// The agent is shared rather than re-exposed over HTTP: the window is the
/// machine's own interface, so it can hold the handle directly, and that keeps
/// the self-signed certificate and the bearer token out of the webview entirely.
pub struct ShellState {
    pub agent: Arc<Agent>,
    pub settings: Settings,
    pub bind: SocketAddr,

    /// Web access. NOT optional, even when the host was assembled without a
    /// portal: the window is where the address is SET, so it must hold the
    /// handle whether or not one is configured yet.
    pub web_access: Arc<WebAccess>,

    /// The pairing code currently on show.
    ///
    /// Minted on demand and remembered here rather than minted per request: a
    /// window that polls `status` must see the *same* code, because minting one
    /// per poll would invalidate the code the user is halfway through typing.
    pub pairing: Mutex<Option<(String, i64)>>,

    /// Why the listener is not up, if it is not.
    ///
    /// The agent can be perfectly healthy — the window reads it in-process —
    /// while *nothing can reach this machine* because the port was taken by
    /// another agent. That is invisible from inside the window unless it is said
    /// out loud, so it is.
    pub server_error: Arc<Mutex<Option<String>>>,
}

impl ShellState {
    pub fn new(
        parts: HostParts,
        web_access: Arc<WebAccess>,
        server_error: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            agent: parts.agent,
            settings: parts.settings,
            bind: parts.bind,
            web_access,
            pairing: Mutex::new(None),
            server_error,
        }
    }
}

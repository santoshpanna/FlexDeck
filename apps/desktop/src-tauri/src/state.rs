use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use flexdeck_agent::HostParts;
use flexdeck_server::{Agent, Settings, WebAccess};
use tokio::sync::mpsc;

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

    /// Why the listener is not up, if it is not.
    ///
    /// The agent can be perfectly healthy — the window reads it in-process —
    /// while *nothing can reach this machine* because the port was taken by
    /// another agent. That is invisible from inside the window unless it is said
    /// out loud, so it is.
    pub server_error: Arc<Mutex<Option<String>>>,

    /// Asks the listener to try its port again now rather than on its timer.
    ///
    /// Pulled when the window comes forward: someone has most likely just closed
    /// whatever held the port and is looking at the window to see it recover.
    pub retry: mpsc::Sender<()>,
}

impl ShellState {
    pub fn new(
        parts: HostParts,
        web_access: Arc<WebAccess>,
        server_error: Arc<Mutex<Option<String>>>,
        retry: mpsc::Sender<()>,
    ) -> Self {
        Self {
            agent: parts.agent,
            settings: parts.settings,
            bind: parts.bind,
            web_access,
            server_error,
            retry,
        }
    }

    /// Try the port again now, if the listener is waiting to.
    ///
    /// Safe to call at any time and any number of times. The channel holds one
    /// signal, so a burst (a tray click and the focus it causes) is one retry;
    /// and the listener empties it before each wait, so a kick sent while it was
    /// up or mid-attempt never makes a *later* failure retry at once.
    pub fn kick(&self) {
        let _ = self.retry.try_send(());
    }
}

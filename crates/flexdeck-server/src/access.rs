//! Who may do what, and from where.
//!
//! A credential alone does not decide what a request may do; *where it came in*
//! does too. The same API is reachable three ways, and each is trusted
//! differently:
//!
//! | Channel | Who is there | Credentials it accepts |
//! |---|---|---|
//! | [`Channel::Lan`] | anything on the network | tablets, and the admin token |
//! | [`Channel::Loopback`] | a browser or coding agent on this very computer | browsers paired locally, agent tokens (only on `/mcp`), and the admin token |
//! | [`Channel::Relay`] | a browser anywhere, through the cloud | browsers paired with a code — nothing else |
//!
//! The channel is a typed request **extension**, set by the listener or the
//! tunnel forwarder that produced the request. A client cannot send one: it is
//! not a header, so there is nothing to forge. A request with no channel at all
//! is treated as [`Channel::Lan`], the one that grants a browser nothing, so
//! forgetting to set it can only ever refuse more, never allow more.
//!
//! A token presented on the wrong channel is simply *unknown*. A browser token
//! stolen from a laptop is useless on the LAN; a tablet token is useless through
//! the cloud; and the admin token never works through the cloud at all, so the
//! cloud — even a compromised one — holds nothing that can act on this machine.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use flexdeck_store::{Device, DeviceKind};
use serde::Serialize;
use tokio::sync::{broadcast, oneshot};

/// Where a request came in. See the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    /// The LAN listener. Also what a request with no channel counts as.
    #[default]
    Lan,
    /// The loopback-only listener the portal uses on this computer.
    Loopback,
    /// Forwarded from the cloud through the tunnel.
    Relay,
}

impl Channel {
    /// Whether a paired credential of `kind` is accepted on this channel.
    pub fn accepts(self, kind: DeviceKind) -> bool {
        matches!(
            (self, kind),
            (Self::Lan, DeviceKind::Tablet)
                | (Self::Loopback, DeviceKind::BrowserLocal)
                | (Self::Loopback, DeviceKind::Agent)
                | (Self::Relay, DeviceKind::BrowserRemote)
        )
    }

    /// Whether the agent's own admin token is accepted on this channel.
    ///
    /// Never through the relay: the cloud must hold nothing that can act here.
    pub fn accepts_admin(self) -> bool {
        self != Self::Relay
    }
}

/// What a presented credential is allowed to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authority {
    /// The agent's own credential: the command line, and tests.
    Admin,

    /// A paired tablet. Reads the deck and presses buttons, nothing more.
    Tablet(Device),

    /// A paired browser — the configurator. See [`Authority::needs_approval`]
    /// for the one way a remote one is held back.
    Browser(Device),

    /// A coding agent on this computer, speaking MCP. Edits the deck and
    /// automations through `/mcp` and nothing else.
    Agent(Device),
}

impl Authority {
    /// May rewrite the layout, automations and plugins.
    pub fn can_configure(&self) -> bool {
        matches!(self, Self::Admin | Self::Browser(_))
    }

    pub fn can_edit_deck(&self) -> bool {
        matches!(self, Self::Admin | Self::Browser(_) | Self::Agent(_))
    }

    pub fn needs_launch_approval(&self) -> bool {
        self.needs_approval() || matches!(self, Self::Agent(_))
    }

    /// Whether running something that executes code on this machine needs a
    /// person at the desktop to say yes first.
    ///
    /// Only for a browser reaching in from elsewhere. The admin token is the
    /// machine's own, and a local browser's user is already sitting at it.
    pub fn needs_approval(&self) -> bool {
        matches!(self, Self::Browser(device) if device.kind == DeviceKind::BrowserRemote)
    }

    /// A short name for the approval prompt: who is asking.
    pub fn requester(&self) -> &str {
        match self {
            Self::Admin => "this computer",
            Self::Tablet(device) | Self::Browser(device) | Self::Agent(device) => &device.name,
        }
    }
}

/// How long a remote request waits for someone at the desktop.
///
/// Shorter than the cloud's 30-second relay timeout, so the browser hears a
/// clear "not approved" rather than a gateway timeout.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(25);

/// Something a remote browser asked to do that needs a yes at the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: u64,
    /// What would happen, in words fit for a dialog.
    pub action: String,
    /// The specifics: which plugin, which permissions.
    pub detail: String,
    /// The paired browser asking.
    pub requester: String,
}

/// Why an approval did not come.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalError {
    #[error("this needs approval on the desktop, and this agent has no window to ask in")]
    NoApprover,

    #[error("this was not approved on the desktop")]
    Denied,

    #[error("nobody approved this on the desktop in time")]
    TimedOut,
}

struct Pending {
    request: ApprovalRequest,
    reply: oneshot::Sender<bool>,
}

#[derive(Default)]
struct Queue {
    /// Set by a shell that can show a prompt. The headless agent never sets it,
    /// so it refuses rather than leaving a request hanging nobody can answer.
    enabled: bool,
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
}

/// Requests waiting for a person at the desktop.
pub struct Approvals {
    queue: Mutex<Queue>,
    changes: broadcast::Sender<()>,
}

impl Default for Approvals {
    fn default() -> Self {
        let (changes, _) = broadcast::channel(16);
        Self {
            queue: Mutex::new(Queue::default()),
            changes,
        }
    }
}

impl Approvals {
    /// Declare that something can show these requests to a person.
    pub fn enable(&self) {
        self.queue().enabled = true;
    }

    /// Ask, and wait up to `timeout` for an answer.
    pub async fn ask(
        &self,
        action: impl Into<String>,
        detail: impl Into<String>,
        requester: impl Into<String>,
        timeout: Duration,
    ) -> Result<(), ApprovalError> {
        let (reply, answer) = oneshot::channel();
        let id = {
            let mut queue = self.queue();
            if !queue.enabled {
                return Err(ApprovalError::NoApprover);
            }
            queue.next_id += 1;
            let id = queue.next_id;
            queue.pending.insert(
                id,
                Pending {
                    request: ApprovalRequest {
                        id,
                        action: action.into(),
                        detail: detail.into(),
                        requester: requester.into(),
                    },
                    reply,
                },
            );
            id
        };
        let _ = self.changes.send(());

        let outcome = tokio::time::timeout(timeout, answer).await;

        // Whatever happened, the prompt is over. A timed-out request must not
        // linger on screen to be approved after the caller has gone.
        if self.queue().pending.remove(&id).is_some() {
            let _ = self.changes.send(());
        }

        match outcome {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) | Ok(Err(_)) => Err(ApprovalError::Denied),
            Err(_) => Err(ApprovalError::TimedOut),
        }
    }

    /// What is waiting right now, oldest first.
    pub fn pending(&self) -> Vec<ApprovalRequest> {
        self.queue()
            .pending
            .values()
            .map(|pending| pending.request.clone())
            .collect()
    }

    /// Answer a request. `false` if it was not waiting any more.
    pub fn answer(&self, id: u64, allow: bool) -> bool {
        let Some(pending) = self.queue().pending.remove(&id) else {
            return false;
        };
        let _ = pending.reply.send(allow);
        let _ = self.changes.send(());
        true
    }

    /// Told whenever the pending set changes.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changes.subscribe()
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
pub(crate) fn answer_next(
    agent: std::sync::Arc<crate::agent::Agent>,
    allow: bool,
) -> tokio::task::JoinHandle<ApprovalRequest> {
    tokio::spawn(async move {
        loop {
            if let Some(request) = agent.approvals().pending().into_iter().next() {
                agent.approvals().answer(request.id, allow);
                return request;
            }
            tokio::task::yield_now().await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn device(kind: DeviceKind) -> Device {
        Device {
            id: "d".into(),
            name: "Laptop".into(),
            created_at: 0,
            kind,
        }
    }

    #[test]
    fn each_credential_is_accepted_only_on_its_own_channel() {
        use DeviceKind::*;
        for (channel, kind, expected) in [
            (Channel::Lan, Tablet, true),
            (Channel::Lan, BrowserLocal, false),
            (Channel::Lan, BrowserRemote, false),
            (Channel::Loopback, Tablet, false),
            (Channel::Loopback, BrowserLocal, true),
            (Channel::Loopback, BrowserRemote, false),
            (Channel::Loopback, Agent, true),
            (Channel::Lan, Agent, false),
            (Channel::Relay, Agent, false),
            (Channel::Relay, Tablet, false),
            (Channel::Relay, BrowserLocal, false),
            (Channel::Relay, BrowserRemote, true),
        ] {
            assert_eq!(channel.accepts(kind), expected, "{channel:?} / {kind:?}");
        }
        assert!(Channel::Lan.accepts_admin());
        assert!(Channel::Loopback.accepts_admin());
        assert!(
            !Channel::Relay.accepts_admin(),
            "the cloud never acts as admin"
        );
    }

    #[test]
    fn only_a_remote_browser_needs_approval() {
        assert!(!Authority::Admin.needs_approval());
        assert!(!Authority::Browser(device(DeviceKind::BrowserLocal)).needs_approval());
        assert!(Authority::Browser(device(DeviceKind::BrowserRemote)).needs_approval());
        assert!(!Authority::Tablet(device(DeviceKind::Tablet)).can_configure());
        assert!(!Authority::Agent(device(DeviceKind::Agent)).needs_approval());
    }

    #[test]
    fn launches_ask_for_a_remote_browser_and_an_agent_only() {
        assert!(!Authority::Admin.needs_launch_approval());
        assert!(!Authority::Browser(device(DeviceKind::BrowserLocal)).needs_launch_approval());
        assert!(Authority::Browser(device(DeviceKind::BrowserRemote)).needs_launch_approval());
        let agent = Authority::Agent(device(DeviceKind::Agent));
        assert!(agent.needs_launch_approval());
        assert!(agent.can_edit_deck());
        assert!(!agent.can_configure());
    }

    #[tokio::test]
    async fn without_a_window_to_ask_in_the_answer_is_no() {
        let approvals = Approvals::default();
        let outcome = approvals
            .ask("Install", "obs", "Laptop", Duration::from_secs(5))
            .await;
        assert_eq!(outcome, Err(ApprovalError::NoApprover));
    }

    #[tokio::test]
    async fn an_approved_request_proceeds_and_a_denied_one_does_not() {
        let approvals = Arc::new(Approvals::default());
        approvals.enable();

        for (allow, expected) in [(true, Ok(())), (false, Err(ApprovalError::Denied))] {
            let asker = Arc::clone(&approvals);
            let asked = tokio::spawn(async move {
                asker
                    .ask("Install", "obs", "Laptop", Duration::from_secs(5))
                    .await
            });
            let id = loop {
                if let Some(request) = approvals.pending().first() {
                    break request.id;
                }
                tokio::task::yield_now().await;
            };
            assert!(approvals.answer(id, allow));
            assert_eq!(asked.await.expect("joins"), expected);
            assert!(approvals.pending().is_empty());
        }
    }

    #[tokio::test]
    async fn an_unanswered_request_times_out_and_leaves_the_queue() {
        let approvals = Approvals::default();
        approvals.enable();
        let outcome = approvals
            .ask("Install", "obs", "Laptop", Duration::from_millis(20))
            .await;
        assert_eq!(outcome, Err(ApprovalError::TimedOut));
        assert!(
            approvals.pending().is_empty(),
            "nothing left to approve late"
        );
    }
}

//! The flexdeck agent's server: HTTP API and WebSocket hub.
//!
//! The crate is arranged so that the parts worth testing are not tangled up with
//! the networking:
//!
//! * `sync` — revision catching-up and the patch wire format. No I/O, no
//!   sockets, and where the protocol round trip is actually proven.
//!
//! The HTTP and WebSocket layers sit on top of `sync` and are deliberately thin:
//! they translate requests into `sync` calls and do nothing clever themselves.
//! That split matters, because the interesting failure modes here are not
//! network failures. They are a patch that does not quite reconstruct the
//! document, or a client told it is current when it is not — and neither is
//! visible from a socket-level test.
//!
//! The same reasoning applies one level further down: `dispatch` decides what a
//! press *should* do, and `ws` only carries frames. So the press path is tested
//! without a socket, and the socket without a real keyboard.

pub mod access;
pub mod agent;
pub mod behavior;
pub mod executor;
pub mod faces;
pub mod http;
pub mod live;
pub mod images;
pub mod launches;
pub mod loopback;
pub mod mcp;
pub mod media;
pub mod net;
pub mod pages;
pub mod plugins;
pub mod presence;
pub mod recipes;
pub mod repos;
pub mod settings;
pub mod sources;
pub mod startup;
pub mod sync;
pub mod tls;
pub mod tunnel;
pub mod upgrade;
pub mod web_access;
pub mod verify;
pub mod watch;
pub mod widgets;
pub mod ws;

pub use agent::{Agent, AgentError, DEFAULT_CONFIG};
pub use behavior::{Behavior, BehaviorError, Interaction};
pub use executor::{executor_owner_thread, ExecutorError, ExecutorHandle, RemoteExecutor};
pub use net::{advertised_host, discover_lan_address, is_reachable_host};
pub use plugins::{Installed, PluginManager, PluginsError, Source};
pub use presence::{Presence, PresenceGuard};
pub use settings::{Settings, SettingsError};
pub use startup::StartupWarning;
pub use sync::{apply_patch, catch_up, decode, encode, patch_message, SyncError, SyncOutcome};
pub use web_access::{view, WebAccess, WebAccessError};

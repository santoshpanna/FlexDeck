//! The cloud portal's server side.
//!
//! The portal is one deployable: this crate is its logic, `apps/portal` is its
//! interface, and the relay that desktops dial into lives here too.
//!
//! # No accounts
//!
//! The cloud hosts the website, forwards traffic, and routes pairing codes. It
//! holds no accounts and no credential that can act on a desktop:
//!
//! * [`Hub`] — the relay. A desktop proves its identity by signature when it
//!   connects (see [`tunnel_socket`]), and a tunnel is then a channel here, so
//!   correlation, reconnects and timeouts are provable without networking.
//! * [`Offers`] — pairing codes a desktop has opened, by hash, so a browser that
//!   types one can be routed to the right desktop.
//! * [`http`] — the website's API: relay a paired browser's call, redeem a code.
//!
//! A browser earns a desktop's trust from the desktop itself, by typing a code
//! shown on its screen; the desktop issues and checks its credential.

pub mod cloud;
pub mod http;
pub mod hub;
pub mod limit;
pub mod offers;
pub mod portal;
pub mod store;
pub mod tunnel_socket;

pub use cloud::Cloud;
pub use hub::{Hub, RelayError, DEFAULT_TIMEOUT};
pub use offers::Offers;
pub use store::{DataDir, StoreError};
pub use tunnel_socket::{admit, Admission};

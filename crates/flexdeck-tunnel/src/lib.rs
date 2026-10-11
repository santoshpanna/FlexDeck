//! The desktop <-> cloud tunnel, minus the networking.
//!
//! Both ends of the tunnel are our own software, which changes what is worth
//! sharing and what is worth testing:
//!
//! * [`frame`] converts to and from the wire JSON. The rules of that JSON live
//!   in `flexdeck-protocol` and are pinned there; this adds one error type, so
//!   neither end has to invent its own idea of what a bad frame means.
//! * [`Correlator`] matches responses to the requests waiting for them. It is
//!   the part with real failure modes — a response for a request nobody is
//!   waiting for, an id reused too early, a tunnel that dies with work in
//!   flight — and none of them need a socket to provoke.
//!
//! Keeping that logic here rather than in either binary is the same split the
//! agent already uses: the interesting bugs are not network bugs, and a test
//! that needs a live socket to find them is slower, flakier, and no more
//! convincing.

pub mod correlate;
pub mod frame;

pub use correlate::{Correlator, Resolved, MAX_IN_FLIGHT};
pub use frame::{decode, encode};

/// The tunnel protocol version this build speaks.
///
/// Sent in `TunnelHello` and checked by the other end, which refuses an unknown
/// version rather than guessing at the frames. A version mismatch that is
/// *silently* tolerated is how a transport quietly becomes a security bug.
pub const PROTOCOL_VERSION: u32 = 2;

/// Everything that can go wrong on the tunnel.
#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    /// A frame could not be converted to or from JSON.
    #[error("a tunnel frame was not valid JSON: {0}")]
    Frame(#[from] serde_json::Error),

    /// A frame parsed, but named no branch.
    ///
    /// `{}` is valid JSON and a valid `TunnelFrame` in the type system — every
    /// field of a `oneof` is optional — but it is not a frame. Refusing it here
    /// means the receiving match never has to invent an answer for "something
    /// arrived that is nothing", which is exactly the case that would otherwise
    /// be mistaken for a request that simply never got a reply.
    #[error("a tunnel frame did not say what it was")]
    Empty,

    /// The tunnel is gone, so nothing more can be sent on it.
    #[error("the tunnel is closed")]
    Closed,

    /// The peer asked for more concurrent work than we allow.
    #[error("too many requests are already in flight (the limit is {limit})")]
    TooManyInFlight { limit: usize },
}

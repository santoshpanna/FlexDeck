//! The flexdeck automation engine: a small, bounded, visual language.
//!
//! # Who this is for
//!
//! The authoring goal is "a programming language for kids, by UI". That has two
//! consequences which shape every decision here:
//!
//! * **nothing may hang.** A child cannot debug an infinite loop, so every
//!   dimension of a run is bounded ([`Limits`]) and reaching a bound is a
//!   *reported outcome*, not a silent stop.
//! * **an unavailable value must be visible.** If `system.audio.volume` has
//!   never been read, a naive engine compares `0 >= 50`, takes the "otherwise"
//!   branch, and the child's deck does something inexplicable. So conditions are
//!   **three-valued**: [`ConditionOutcome::Unknown`] is a real answer, it takes
//!   the else path, and the trace says why.
//!
//! # Why the evaluator is a plain recursive function
//!
//! `Wait` and the wall-clock bound both need time, but nothing here reads the
//! clock or sleeps directly: [`Clock`] and [`Sleeper`] are injected. A test
//! supplies a [`FakeClock`] and a [`RecordingSleeper`], so a ten-second
//! automation finishes instantly and the wall-clock bound is provable at the
//! boundary with no sleeping.
//!
//! The cost is that a run occupies a thread while it waits. That is deliberate
//! and the engine's job to contain, not this crate's: a run belongs on a
//! blocking thread with a cap on how many may be in flight, never on an async
//! worker.

pub mod clock;
pub mod eval;
pub mod model;

pub use clock::{
    AdvancingSleeper, Cancel, CancelFlag, Clock, FakeClock, NeverCancel, RecordingSleeper, Sleeper,
    SystemClock, ThreadSleeper,
};
pub use eval::{
    run, ConditionOutcome, Environment, Inputs, Resolved, Run, RunOutcome, Trace, NO_INPUTS,
};
pub use model::{
    parse_document, validate_blocks, Automation, AutomationDocument, Block, Branch, CompareOp,
    Condition, DocumentError, LocalTime, Trigger, ValueRef, AUTOMATION_SCHEMA,
    MIN_SCHEDULE_SECONDS,
};

/// Every hard bound a run obeys.
///
/// Chosen to be far above anything a person would author by hand and far below
/// anything that would hurt the machine — the point is to turn "runaway" into
/// "stopped, and here is which limit".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How deeply blocks and conditions may nest.
    pub max_depth: usize,
    /// How many blocks one document may contain, enforced before it is stored.
    pub max_blocks: usize,
    /// How many steps one run may take.
    pub max_steps: usize,
    /// How many times one `Repeat` may loop.
    pub max_iterations: u32,
    /// The longest a single `Wait` may ask for.
    pub max_wait_ms: u64,
    /// The longest a whole run may take, checked between steps.
    pub max_run_ms: u64,
    /// The longest string a `Trace` entry may carry. Bounds at the source
    /// rather than at the tunnel so the relay never has to truncate a
    /// body it has already paid to parse.
    pub max_trace_string: usize,
    /// The total bytes the trace may consume, enforced inside the evaluator.
    pub max_trace_bytes: usize,
    /// How many widget-triggered automations one press handler may chain.
    /// Caps the chain at the seam between the press handler and the engine
    /// rather than inside a run, where every chain element sleeps the full
    /// `Wait` against a `ThreadSleeper` and serialises every other press.
    pub max_press_chain: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 16,
            max_blocks: 512,
            max_steps: 2_000,
            max_iterations: 1_000,
            max_wait_ms: 60_000,
            max_run_ms: 30_000,
            // 256 characters is plenty for a typed/argued string and well
            // below the size where the trace becomes a denial-of-service
            // vector. The trace-bytes cap is the second line of defence:
            // even many short strings cannot exceed ~32 KiB together.
            max_trace_string: 256,
            max_trace_bytes: 32 * 1024,
            // 32 widget-triggered automations is more than a hand-written
            // config should ever have, and large enough that a stress test
            // pasting one in cannot DoS the agent.
            max_press_chain: 32,
        }
    }
}

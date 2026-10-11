//! Time and cancellation, injected rather than reached for.
//!
//! Every one of these exists so a test can be exact. A `Wait` that really slept
//! would make the wall-clock bound untestable and the suite slow; a clock read
//! inside the evaluator would make "did it exceed 30 seconds" a matter of how
//! busy the machine was.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// A source of "now", in milliseconds.
///
/// Only *differences* are meaningful, so an implementation may start anywhere.
pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now_ms(&self) -> i64;
}

/// The real clock.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// A clock a test moves by hand.
#[derive(Debug, Default)]
pub struct FakeClock {
    now_ms: Mutex<i64>,
}

impl FakeClock {
    pub fn new(start_ms: i64) -> Self {
        Self {
            now_ms: Mutex::new(start_ms),
        }
    }

    /// Move time forward. Nothing moves it on its own, so a test is exact.
    pub fn advance(&self, ms: i64) {
        let mut now = self.now_ms.lock().unwrap_or_else(PoisonError::into_inner);
        *now = now.saturating_add(ms);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        *self.now_ms.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Waits. The only thing in this crate that would really block.
pub trait Sleeper: Send + std::fmt::Debug {
    fn sleep(&mut self, ms: u64);
}

/// Records what was asked for and returns immediately.
#[derive(Debug, Default)]
pub struct RecordingSleeper {
    slept: Vec<u64>,
}

impl RecordingSleeper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn slept(&self) -> &[u64] {
        &self.slept
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&mut self, ms: u64) {
        self.slept.push(ms);
    }
}

/// Records the wait **and** moves a [`FakeClock`] forward by it.
///
/// This is what makes the wall-clock bound testable: without it a fake clock
/// never advances, and `max_run_ms` could never be reached in a test.
#[derive(Debug)]
pub struct AdvancingSleeper {
    clock: Arc<FakeClock>,
    slept: Vec<u64>,
}

impl AdvancingSleeper {
    pub fn new(clock: Arc<FakeClock>) -> Self {
        Self {
            clock,
            slept: Vec::new(),
        }
    }

    pub fn slept(&self) -> &[u64] {
        &self.slept
    }
}

impl Sleeper for AdvancingSleeper {
    fn sleep(&mut self, ms: u64) {
        self.clock.advance(ms as i64);
        self.slept.push(ms);
    }
}

/// Really waits.
///
/// A run belongs on a blocking thread, so blocking here is correct — the engine
/// that spawns it owns the decision about how many may be in flight.
#[derive(Debug, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// Whether a run should stop.
///
/// Checked **between** steps, which is the honest bound: nothing here can
/// interrupt a plugin call that is already inside the operating system.
pub trait Cancel: Send + Sync + std::fmt::Debug {
    fn cancelled(&self) -> bool;
}

/// Never stops. The default for a run nobody is waiting to abort.
#[derive(Debug, Default)]
pub struct NeverCancel;

impl Cancel for NeverCancel {
    fn cancelled(&self) -> bool {
        false
    }
}

/// A flag a caller can raise — the agent raises it when an automation is
/// disabled or the document changes under a run in flight.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Cancel for CancelFlag {
    fn cancelled(&self) -> bool {
        self.is_cancelled()
    }
}

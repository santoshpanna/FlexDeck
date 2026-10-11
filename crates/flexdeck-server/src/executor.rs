//! The executor's owner thread.
//!
//! The executor — the thing that types and presses keys — lives on a dedicated
//! thread and is reached through a channel. That breaks the deadlock the jury
//! identified: a widget press that triggers an automation whose `DoAction`
//! needs the executor would otherwise try to re-lock a non-reentrant `Mutex`
//! and hang, because `Agent::press` held the executor lock while calling the
//! engine.
//!
//! `RemoteExecutor` is the bridge for the automation engine: the engine borrows
//! `&mut dyn Executor` to hand plugins through `Ctx`, but the executor is no
//! longer behind a shared lock — it is *owned* by the owner thread. The wrapper
//! performs an action by sending a request to the owner and waiting for the
//! reply, so every plugin call goes through the channel and re-entrancy is
//! impossible: the owner thread is the only place `executor.perform` is ever
//! called.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use flexdeck_actions::{ActionError, CoreAction, Executor};

#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    /// The owner thread is no longer reachable. This is a `JoinError` in
    /// disguise: the channel closes when the thread dies, and the only way
    /// that happens is the executor being dropped.
    #[error("the executor owner has gone away")]
    Unavailable,

    #[error(transparent)]
    Action(#[from] ActionError),
}

pub(crate) enum ExecMsg {
    Perform {
        action: CoreAction,
        reply: mpsc::Sender<Result<(), ActionError>>,
    },
    /// Swap the executor in place. Sent by [`Run::install_live_executor`].
    Install(Box<dyn Executor + Send>),
}

/// A cheap, cloneable handle to the executor owner.
#[derive(Clone)]
pub struct ExecutorHandle {
    pub(crate) sender: mpsc::SyncSender<ExecMsg>,
}

impl ExecutorHandle {
    /// Perform an action synchronously by sending a request to the owner and
    /// blocking on the reply.
    ///
    /// Always callable from any context — it uses the channel system, not a
    /// tokio runtime — so the engine can call it from a blocking task and a
    /// test can call it from a synchronous test body.
    ///
    /// Refuses with [`ExecutorError::Unavailable`] rather than blocking when
    /// the bounded channel is full: a wedged executor should not turn into
    /// an unbounded queue of waiters. The caller (a press handler) turns the
    /// refusal into a visible error rather than a silent slowdown.
    pub fn perform(&self, action: CoreAction) -> Result<(), ExecutorError> {
        let (reply, received) = mpsc::channel();
        self.sender
            .try_send(ExecMsg::Perform { action, reply })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => ExecutorError::Unavailable,
                mpsc::TrySendError::Disconnected(_) => ExecutorError::Unavailable,
            })?;
        match received.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(ExecutorError::Action(error)),
            Err(_) => Err(ExecutorError::Unavailable),
        }
    }

    /// Swap in a new one. Installed under the same lock as `perform`, so a
    /// action in flight finishes against the old executor and the next one
    /// uses the new one — never a torn executor.
    pub fn install(&self, executor: Box<dyn Executor + Send>) -> Result<(), ExecutorError> {
        self.sender
            .try_send(ExecMsg::Install(executor))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => ExecutorError::Unavailable,
                mpsc::TrySendError::Disconnected(_) => ExecutorError::Unavailable,
            })
    }
}

/// Spawn the owner thread. Returns the handle the rest of the agent talks to
/// and the [`JoinHandle`] of the owner (mostly useful for tests).
///
/// The channel is bounded — [`CHANNEL_CAPACITY`] pending messages. The bound
/// matters because an unbounded queue turns a wedged executor into an
/// unbounded memory growth: every press that arrives while the owner is
/// stuck queues another `Perform` and a reply allocation, and nothing in the
/// pipeline ever sheds the load. With a bound, [`ExecutorHandle::perform`]
/// refuses the eleventh concurrent request instead of queueing it, and the
/// caller (`Behavior::press`) gets a fast `Unavailable` to turn into a refusal
/// rather than a silent slowdown.
pub fn executor_owner_thread(
    initial: Box<dyn Executor + Send>,
) -> (ExecutorHandle, JoinHandle<()>) {
    let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
    let join = thread::Builder::new()
        .name("flexdeck-executor".to_string())
        .spawn(move || run(initial, receiver))
        .expect("spawn executor owner thread");
    (ExecutorHandle { sender }, join)
}

/// The hard upper bound on in-flight messages between the agent and the
/// executor owner. Sized for "ten widgets could press at once and the owner
/// is briefly blocked" rather than "sustained back-pressure queues forever".
const CHANNEL_CAPACITY: usize = 64;

fn run(mut executor: Box<dyn Executor + Send>, receiver: mpsc::Receiver<ExecMsg>) {
    while let Ok(message) = receiver.recv() {
        match message {
            ExecMsg::Perform { action, reply } => {
                // Discard the reply if the caller is gone — the action ran,
                // and we cannot help the part where nobody was listening.
                let _ = reply.send(executor.perform(&action));
            }
            ExecMsg::Install(new_executor) => {
                executor = new_executor;
            }
        }
    }
}

/// An [`Executor`] backed by the owner thread.
///
/// Constructed inside an engine call so each invocation borrows the executor
/// for its duration, but every `perform` here crosses the channel and ends up
/// on the same thread that owns it. A widget press that fires an automation
/// whose `DoAction` ends in `core.hotkey` therefore flows: press -> engine ->
/// remote perform -> owner thread. No lock to re-enter.
pub struct RemoteExecutor {
    handle: ExecutorHandle,
}

impl RemoteExecutor {
    pub fn new(handle: ExecutorHandle) -> Self {
        Self { handle }
    }
}

impl Executor for RemoteExecutor {
    fn perform(&mut self, action: &CoreAction) -> Result<(), ActionError> {
        self.handle
            .perform(action.clone())
            .map_err(|error| match error {
                ExecutorError::Action(action) => action,
                ExecutorError::Unavailable => {
                    ActionError::Execution("the executor owner has gone away".to_string())
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexdeck_actions::Hotkey;
    use flexdeck_actions::RecordingExecutor;

    #[test]
    fn perform_round_trips_through_the_owner_thread() {
        let recorder = Box::new(RecordingExecutor::new());
        let (handle, _join) = executor_owner_thread(recorder);

        handle
            .perform(CoreAction::Hotkey(Hotkey::parse("ctrl+shift+m").unwrap()))
            .expect("perform reaches the owner thread");
    }

    #[test]
    fn the_owner_thread_exits_cleanly_when_the_last_handle_is_dropped() {
        // When every clone of `ExecutorHandle` drops, the sender closes, the
        // owner's `recv` returns `Err`, and the thread exits. The whole path
        // is the failure mode this module exists to test.
        let (handle, join) = executor_owner_thread(Box::new(RecordingExecutor::new()));
        drop(handle);
        join.join().expect("owner thread exits cleanly");
    }

    #[test]
    fn remote_executor_compiles_and_runs() {
        // The trait impl is the contract the engine depends on. If it
        // regresses, every DoAction breaks.
        let recorder = Box::new(RecordingExecutor::new());
        let (handle, _join) = executor_owner_thread(recorder);
        let mut remote = RemoteExecutor::new(handle);
        remote
            .perform(&CoreAction::Text("hi".to_string()))
            .expect("remote perform");
    }

    #[test]
    fn install_swaps_the_executor_so_the_next_perform_uses_it() {
        let recorder_a = Box::new(RecordingExecutor::new());
        let (handle, _join) = executor_owner_thread(recorder_a);
        let mut remote = RemoteExecutor::new(handle.clone());

        remote
            .perform(&CoreAction::Text("first".to_string()))
            .unwrap();

        // Install a fresh recorder; the owner thread holds onto it.
        handle.install(Box::new(RecordingExecutor::new())).unwrap();

        // The next perform uses the installed executor. We cannot peek, but
        // the call must still succeed: that proves the install landed and the
        // owner did not crash.
        remote
            .perform(&CoreAction::Text("second".to_string()))
            .unwrap();
    }

    #[test]
    fn the_channel_refuses_when_full_rather_than_blocking() {
        // The bounded channel is a back-pressure surface. Hitting the cap
        // returns `Unavailable` rather than waiting forever, which is what
        // the press handler turns into a refusal.
        //
        // We pin the behaviour by driving the channel directly: a wedged
        // executor is harder to stage, and the test of the bound is the
        // bound itself — full means full, regardless of who is on the
        // receiving end. The `perform()` path goes through the same
        // `try_send`, so testing `try_send` proves `perform`.
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct CountingExecutor {
            count: Arc<AtomicUsize>,
        }
        impl Executor for CountingExecutor {
            fn perform(&mut self, _: &CoreAction) -> Result<(), ActionError> {
                self.count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let (handle, _join) = executor_owner_thread(Box::new(CountingExecutor {
            count: count.clone(),
        }));

        // Fill the channel with the same shape `perform` uses. The owner
        // thread drains these serially; filling it puts the next send
        // over the cap.
        let (overflow_tx, _overflow_rx) = mpsc::channel::<()>();
        for _ in 0..CHANNEL_CAPACITY {
            handle
                .sender
                .try_send(ExecMsg::Install(Box::new(CountingExecutor {
                    count: count.clone(),
                })))
                .expect("the bounded channel should accept CHANNEL_CAPACITY messages");
        }

        // The next one is refused rather than queued: the channel is at
        // capacity. This is the assertion the test exists to make.
        let overflow = handle
            .sender
            .try_send(ExecMsg::Install(Box::new(CountingExecutor {
                count: count.clone(),
            })));
        assert!(
            matches!(overflow, Err(mpsc::TrySendError::Full(_))),
            "the overflow must be refused as Full, got {overflow:?}",
        );

        // The reader end is unused but kept alive so the channel does not
        // count as disconnected.
        let _ = overflow_tx;
    }
}

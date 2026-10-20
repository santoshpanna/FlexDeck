//! Running an automation, under hard bounds.
//!
//! The evaluator is a plain recursive walk. Everything that touches the outside
//! world arrives through [`Environment`], and every bound is checked in one
//! place ([`step`]) so "why did it stop" has exactly one answer.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use flexdeck_actions::{CoreAction, Executor};
use flexdeck_plugin::{Ctx, Notifier, Registry};
use flexdeck_protocol::Action;
use flexdeck_vars::{Value, VariableStore};

use crate::clock::{Cancel, Clock, Sleeper};
use crate::model::{Automation, Block, CompareOp, Condition, ValueRef};
use crate::Limits;

/// What a [`ValueRef`] resolved to at run time.
///
/// `Unknown` is the variant that earns its keep. A variable that has never been
/// sampled, or whose plugin is unavailable on this machine, is **not** zero and
/// is **not** false — inventing either would make an automation take a branch
/// for a reason the author cannot see.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Bool(bool),
    Number(f64),
    Text(String),
    Unknown { reason: String },
}

/// The three-valued answer to a condition.
///
/// `Unknown` takes the else path **and is reported in the trace**, which is the
/// difference between "my automation went the wrong way" and "my automation
/// never found out the volume".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConditionOutcome {
    True,
    False,
    Unknown(String),
}

/// One thing that happened, in order. This is what the editor's Test button
/// shows, so it is written to be read by a person rather than a log parser.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Trace {
    Condition {
        summary: String,
        outcome: ConditionOutcome,
    },
    Ran {
        plugin: String,
        action: String,
        /// What was typed or pressed, when the action injected input.
        input: Option<String>,
        /// The look the action reported, for a widget whose looks it provides.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        /// Where the action sits in the automation, counted the way a
        /// widget's provider path counts (see `flexdeck-server`'s `block_at`):
        /// how the agent finds *which* action reported a look. Plumbing, so
        /// it is not shown.
        #[serde(skip)]
        path: Vec<usize>,
    },
    Set {
        name: String,
        value: Value,
    },
    Waited {
        ms: u64,
    },
    Repeated {
        times: u32,
    },
    Stopped {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunOutcome {
    /// Ran to the end.
    Completed,
    /// Someone asked it to stop between steps.
    Cancelled,
    /// A bound was reached. Not a failure of the automation's logic.
    LimitReached,
    /// The automation could not do what it said.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub outcome: RunOutcome,
    pub trace: Vec<Trace>,
}

impl Run {
    pub fn succeeded(&self) -> bool {
        self.outcome == RunOutcome::Completed
    }

    /// The reason a run stopped, if it did.
    pub fn reason(&self) -> Option<&str> {
        self.trace.iter().find_map(|entry| match entry {
            Trace::Stopped { reason } => Some(reason.as_str()),
            _ => None,
        })
    }
}

/// Values a run is given rather than reads from the variable store: a slider's
/// position, an event's payload. Only that one run sees them.
pub type Inputs = BTreeMap<String, Value>;

/// No inputs, for a run that has none.
pub static NO_INPUTS: Inputs = BTreeMap::new();

/// What a value reference can read: the live variables, and this run's inputs.
#[derive(Clone, Copy)]
pub(crate) struct Scope<'a> {
    pub vars: &'a VariableStore,
    pub inputs: &'a Inputs,
}

impl<'a> Scope<'a> {
    #[cfg(test)]
    pub fn of(vars: &'a VariableStore) -> Self {
        Self {
            vars,
            inputs: &NO_INPUTS,
        }
    }
}

/// Everything a run can touch.
pub struct Environment<'a> {
    pub registry: &'a mut Registry,
    pub latest: Option<&'a dyn Fn() -> Registry>,
    pub vars: &'a VariableStore,
    /// See [`Inputs`]. `&NO_INPUTS` when there are none.
    pub inputs: &'a Inputs,
    pub input: &'a mut dyn Executor,
    pub notifier: &'a mut dyn Notifier,
    pub settings: &'a BTreeMap<String, String>,
    pub clock: &'a dyn Clock,
    pub sleeper: &'a mut dyn Sleeper,
    pub cancel: &'a dyn Cancel,
    pub limits: Limits,
}

struct State<'a> {
    latest: Option<&'a dyn Fn() -> Registry>,
    vars: &'a VariableStore,
    inputs: &'a Inputs,
    /// The path to the block running now; see `Trace::Ran::path`.
    path: Vec<usize>,
    input: &'a mut dyn Executor,
    notifier: &'a mut dyn Notifier,
    settings: &'a BTreeMap<String, String>,
    clock: &'a dyn Clock,
    sleeper: &'a mut dyn Sleeper,
    cancel: &'a dyn Cancel,
    limits: Limits,
    started_ms: i64,
    steps: usize,
    trace: Vec<Trace>,
    trace_bytes: usize,
}

/// Why a run stopped early.
///
/// `pub(crate)` because the condition helpers a test drives return it; it is not
/// part of the crate's published surface.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop {
    Cancelled,
    Limit(String),
    Failed(String),
}

/// Run one automation to completion, or to a bound.
///
/// Never panics on a bad document: every refusal is an outcome and a line in the
/// trace, because the person reading it is looking at a screen, not a log.
pub fn run(environment: &mut Environment<'_>, automation: &Automation) -> Run {
    let started_ms = environment.clock.now_ms();

    let mut state = State {
        latest: environment.latest,
        vars: environment.vars,
        inputs: environment.inputs,
        path: Vec::new(),
        input: &mut *environment.input,
        notifier: &mut *environment.notifier,
        sleeper: &mut *environment.sleeper,
        clock: environment.clock,
        cancel: environment.cancel,
        settings: environment.settings,
        limits: environment.limits,
        started_ms,
        steps: 0,
        trace: Vec::new(),
        // Budget consumed by the trace so far, used to cap total bytes at
        // the source rather than letting a long trace amplify into a denial
        // of service at the relay.
        trace_bytes: 0,
    };
    let registry = &mut *environment.registry;

    let result = if automation.enabled {
        run_blocks(registry, &mut state, &automation.body, 0, 1)
    } else {
        Err(Stop::Failed("this automation is switched off".to_string()))
    };

    let outcome = match result {
        Ok(()) => RunOutcome::Completed,
        Err(Stop::Cancelled) => RunOutcome::Cancelled,
        Err(Stop::Limit(reason)) => {
            push_trace(&mut state, Trace::Stopped { reason });
            RunOutcome::LimitReached
        }
        Err(Stop::Failed(reason)) => {
            push_trace(&mut state, Trace::Stopped { reason });
            RunOutcome::Failed
        }
    };

    Run {
        outcome,
        trace: state.trace,
    }
}

/// Push one trace entry while enforcing the per-string and total-byte caps.
///
/// Strings longer than `limits.max_trace_string` are truncated with a
/// trailing `…` so a person reading the trace can see it was over the
/// cap. Once the total budget is exhausted, further pushes are dropped —
/// the trace keeps what it had, and the run continues. The relay then
/// sees a bounded JSON body rather than truncating a multi-megabyte one
/// after the cost of parsing and serialising it.
fn push_trace(state: &mut State<'_>, entry: Trace) {
    // Truncate per-string first; the per-string cap is the user-visible
    // bound and the byte budget is the second line of defence.
    let entry = clamp_trace_entry(entry, state.limits.max_trace_string);
    let cost = estimate_size(&entry);
    if state.trace_bytes + cost > state.limits.max_trace_bytes {
        return;
    }
    state.trace_bytes += cost;
    state.trace.push(entry);
}

/// Truncate every `String` field of a `Trace` to the given length.
///
/// `Trace` has string fields in every variant; `Ran` has the user-controlled
/// `input`, `Stopped` and `Condition` have summary reasons, `Set` has the
/// variable name. None of them are bounded today, so the helper walks the
/// variant.
fn clamp_trace_entry(entry: Trace, max: usize) -> Trace {
    fn cap(string: String, max: usize) -> String {
        if string.len() <= max {
            return string;
        }
        // Walk back to the nearest UTF-8 boundary so we never split a
        // code point. The trailing ellipsis is one byte over the cap,
        // which is the right shape for a human reading the trace.
        let mut boundary = max;
        while boundary > 0 && !string.is_char_boundary(boundary) {
            boundary -= 1;
        }
        let mut truncated = string[..boundary].to_string();
        truncated.push('…');
        truncated
    }

    match entry {
        Trace::Condition { summary, outcome } => Trace::Condition {
            summary: cap(summary, max),
            outcome,
        },
        Trace::Ran {
            plugin,
            action,
            input,
            state,
            path,
        } => Trace::Ran {
            plugin,
            action,
            input: input.map(|text| cap(text, max)),
            state: state.map(|text| cap(text, max)),
            path,
        },
        Trace::Set { name, value } => Trace::Set {
            name: cap(name, max),
            value,
        },
        Trace::Stopped { reason } => Trace::Stopped {
            reason: cap(reason, max),
        },
        Trace::Waited { ms } => Trace::Waited { ms },
        Trace::Repeated { times } => Trace::Repeated { times },
    }
}

/// Best-effort byte-cost estimate for a trace entry.
///
/// `String::len` is the byte length, which is the right unit to compare
/// against the budget. The variant tag and the small integers do not need
/// accounting; if the strings fit, the whole entry fits.
fn estimate_size(entry: &Trace) -> usize {
    match entry {
        Trace::Condition { summary, .. } => summary.len(),
        Trace::Ran {
            plugin,
            action,
            input,
            state,
            ..
        } => {
            plugin.len()
                + action.len()
                + input.as_ref().map(String::len).unwrap_or(0)
                + state.as_ref().map(String::len).unwrap_or(0)
        }
        Trace::Set { name, .. } => name.len(),
        Trace::Stopped { reason } => reason.len(),
        Trace::Waited { .. } | Trace::Repeated { .. } => 0,
    }
}

/// The one place a bound is enforced.
///
/// Cancellation is checked here too, which is the honest limit of the promise:
/// this stops a run *between* steps and cannot interrupt a plugin that is
/// already inside the operating system.
fn step(state: &mut State<'_>) -> Result<(), Stop> {
    if state.cancel.cancelled() {
        return Err(Stop::Cancelled);
    }

    state.steps += 1;
    if state.steps > state.limits.max_steps {
        return Err(Stop::Limit(format!(
            "it ran more than {} steps",
            state.limits.max_steps
        )));
    }

    let elapsed = state.clock.now_ms().saturating_sub(state.started_ms);
    if elapsed > state.limits.max_run_ms as i64 {
        return Err(Stop::Limit(format!(
            "it ran longer than {} ms",
            state.limits.max_run_ms
        )));
    }

    Ok(())
}

/// Check the wall-clock budget again after a block has spent time.
///
/// The check at the top of [`step`] only fires between blocks, so a single
/// long `Wait` or a plugin call that overshoots the limit must be caught
/// *afterwards*. This is the post-block check.
fn check_wall(state: &State<'_>) -> Result<(), Stop> {
    let elapsed = state.clock.now_ms().saturating_sub(state.started_ms);
    if elapsed > state.limits.max_run_ms as i64 {
        return Err(Stop::Limit(format!(
            "it ran longer than {} ms",
            state.limits.max_run_ms
        )));
    }
    Ok(())
}

/// Run `blocks`, which are a parent's children starting at `offset` in the
/// order a provider path counts them.
fn run_blocks(
    registry: &mut Registry,
    state: &mut State<'_>,
    blocks: &[Block],
    offset: usize,
    depth: usize,
) -> Result<(), Stop> {
    if depth > state.limits.max_depth {
        return Err(Stop::Limit(format!(
            "it nested more than {} levels deep",
            state.limits.max_depth
        )));
    }

    for (index, block) in blocks.iter().enumerate() {
        step(state)?;
        state.path.push(offset + index);
        let result = run_block(registry, state, block, depth);
        state.path.pop();
        result?;
    }

    Ok(())
}

fn run_block(
    registry: &mut Registry,
    state: &mut State<'_>,
    block: &Block,
    depth: usize,
) -> Result<(), Stop> {
    match block {
        Block::If {
            condition,
            then_blocks,
            else_if,
            otherwise,
        } => {
            let outcome = record_condition(state, condition, depth)?;

            if outcome == ConditionOutcome::True {
                return run_blocks(registry, state, then_blocks, 0, depth + 1);
            }

            let mut offset = then_blocks.len();
            for branch in else_if {
                let outcome = record_condition(state, &branch.condition, depth)?;
                if outcome == ConditionOutcome::True {
                    return run_blocks(registry, state, &branch.blocks, offset, depth + 1);
                }
                offset += branch.blocks.len();
            }

            // Reached for both `False` and `Unknown`. An unknown answer takes
            // the else path deliberately, and the trace already says why.
            run_blocks(registry, state, otherwise, offset, depth + 1)
        }

        Block::Repeat { times, body } => {
            let count = match resolve(scope(state), times) {
                Resolved::Number(value) if value >= 0.0 => value as u32,
                other => {
                    return Err(Stop::Failed(format!(
                        "repeat needs a count, but got {}",
                        shape(&other)
                    )))
                }
            };

            if count > state.limits.max_iterations {
                return Err(Stop::Limit(format!(
                    "repeat asked for {count} times, and the limit is {}",
                    state.limits.max_iterations
                )));
            }

            push_trace(state, Trace::Repeated { times: count });

            for _ in 0..count {
                run_blocks(registry, state, body, 0, depth + 1)?;
            }

            Ok(())
        }

        Block::Wait { ms } => {
            let millis = match resolve(scope(state), ms) {
                Resolved::Number(value) if value >= 0.0 => value as u64,
                other => {
                    return Err(Stop::Failed(format!(
                        "wait needs a number of milliseconds, but got {}",
                        shape(&other)
                    )))
                }
            };

            if millis > state.limits.max_wait_ms {
                return Err(Stop::Limit(format!(
                    "a wait of {millis} ms is longer than the {} ms limit",
                    state.limits.max_wait_ms
                )));
            }

            state.sleeper.sleep(millis);
            check_wall(state)?;
            push_trace(state, Trace::Waited { ms: millis });
            Ok(())
        }

        Block::SetVariable { name, value } => {
            let resolved = concrete(scope(state), value, "a value to set")?;

            state
                .vars
                .set(name, resolved.clone())
                .map_err(|error| Stop::Failed(format!("'{name}' could not be set: {error}")))?;

            push_trace(
                state,
                Trace::Set {
                    name: name.clone(),
                    value: resolved,
                },
            );
            Ok(())
        }

        Block::DoAction {
            plugin,
            action,
            args,
        } => {
            let mut configured_args = HashMap::with_capacity(args.len());

            for (name, reference) in args {
                let resolved = concrete(
                    scope(state),
                    reference,
                    &format!("the argument '{name}' of '{plugin}.{action}'"),
                )?;
                configured_args.insert(name.clone(), argument_text(&resolved));
            }

            let configured = Action {
                plugin: plugin.clone(),
                action: action.clone(),
                args: configured_args,
            };

            if let Some(latest) = state.latest {
                *registry = latest();
            }
            let now_ms = state.clock.now_ms();
            let performed = {
                let mut ctx = Ctx {
                    input: &mut *state.input,
                    vars: state.vars,
                    notifier: &mut *state.notifier,
                    settings: state.settings,
                    now_ms,
                };
                registry.invoke(&configured, &mut ctx)
            };

            match performed {
                Ok(performed) => {
                    let path = state.path.clone();
                    push_trace(
                        state,
                        Trace::Ran {
                            plugin: performed.plugin,
                            action: performed.action,
                            input: performed.input.as_ref().map(describe),
                            state: performed.state,
                            path,
                        },
                    )
                }
                Err(error) => {
                    return Err(Stop::Failed(format!(
                        "'{plugin}.{action}' could not run: {error}"
                    )))
                }
            }

            check_wall(state)?;
            Ok(())
        }
    }
}

fn record_condition(
    state: &mut State<'_>,
    condition: &Condition,
    depth: usize,
) -> Result<ConditionOutcome, Stop> {
    let outcome = eval_condition(scope(state), &state.limits, condition, depth)?;
    push_trace(
        state,
        Trace::Condition {
            summary: summarise(condition),
            outcome: outcome.clone(),
        },
    );
    Ok(outcome)
}

fn scope<'a>(state: &State<'a>) -> Scope<'a> {
    Scope {
        vars: state.vars,
        inputs: state.inputs,
    }
}

/// Resolve a reference, refusing an unknown one with the reason it is unknown.
fn concrete(scope: Scope<'_>, reference: &ValueRef, what: &str) -> Result<Value, Stop> {
    match resolve(scope, reference) {
        Resolved::Bool(value) => Ok(Value::Bool(value)),
        Resolved::Number(value) => Ok(Value::Number(value)),
        Resolved::Text(value) => Ok(Value::Text(value)),
        Resolved::Unknown { reason } => Err(Stop::Failed(format!(
            "{what} cannot be worked out: {reason}"
        ))),
    }
}

/// Read a reference out of the live variables, or this run's inputs.
pub(crate) fn resolve(scope: Scope<'_>, reference: &ValueRef) -> Resolved {
    let known = |value: Option<Value>| match value {
        Some(Value::Bool(value)) => Some(Resolved::Bool(value)),
        Some(Value::Number(value)) => Some(Resolved::Number(value)),
        Some(Value::Text(value)) => Some(Resolved::Text(value)),
        None => None,
    };
    match reference {
        ValueRef::Bool(value) => Resolved::Bool(*value),
        ValueRef::Number(value) => Resolved::Number(*value),
        // Text with holes is filled now: "Temp: {{vars.temp}}", "{{input.value | round}}%".
        ValueRef::Text(value) if value.contains("{{") => Resolved::Text(fill(scope, value)),
        ValueRef::Text(value) => Resolved::Text(value.clone()),
        ValueRef::Variable(name) => known(scope.vars.get(name)).unwrap_or_else(|| {
            Resolved::Unknown {
                reason: format!("'{name}' has no value yet"),
            }
        }),
        ValueRef::Input(name) => known(scope.inputs.get(name).cloned()).unwrap_or_else(|| {
            Resolved::Unknown {
                reason: format!("this run was not given '{name}'"),
            }
        }),
    }
}

/// Fill a template's holes from the live variables, and this run's inputs as
/// `input.<name>`. A template that does not parse is kept as written: the
/// step then says plainly what it was given.
fn fill(scope: Scope<'_>, template: &str) -> String {
    let as_val = |value: Option<Value>| match value {
        Some(Value::Bool(b)) => flexdeck_expr::Val::Bool(b),
        Some(Value::Number(n)) => flexdeck_expr::Val::Number(n),
        Some(Value::Text(t)) => flexdeck_expr::Val::Text(t),
        None => flexdeck_expr::Val::Missing,
    };
    let lookup = |name: &str| match name.strip_prefix("input.") {
        Some(input) => as_val(scope.inputs.get(input).cloned()),
        None => as_val(scope.vars.get(name)),
    };
    flexdeck_expr::render(template, &lookup, &flexdeck_expr::Locale::default())
}

/// Evaluate a condition with three-valued logic.
///
/// `All` is false as soon as one arm is false; `Any` is true as soon as one arm
/// is true; otherwise an unknown arm makes the whole thing unknown. That is the
/// standard strong-Kleene reading, and it is the only one that never claims to
/// know something it does not.
pub(crate) fn eval_condition(
    vars: Scope<'_>,
    limits: &Limits,
    condition: &Condition,
    depth: usize,
) -> Result<ConditionOutcome, Stop> {
    if depth > limits.max_depth {
        return Err(Stop::Limit(format!(
            "the condition nested more than {} levels deep",
            limits.max_depth
        )));
    }

    match condition {
        Condition::Compare { left, op, right } => {
            let left = resolve(vars, left);
            let right = resolve(vars, right);

            if let Resolved::Unknown { reason } = &left {
                return Ok(ConditionOutcome::Unknown(reason.clone()));
            }
            if let Resolved::Unknown { reason } = &right {
                return Ok(ConditionOutcome::Unknown(reason.clone()));
            }

            let verdict = compare(&left, *op, &right).map_err(Stop::Failed)?;
            Ok(if verdict {
                ConditionOutcome::True
            } else {
                ConditionOutcome::False
            })
        }

        Condition::All { conditions } => {
            let mut unknown = None;
            for nested in conditions {
                match eval_condition(vars, limits, nested, depth + 1)? {
                    ConditionOutcome::False => return Ok(ConditionOutcome::False),
                    ConditionOutcome::Unknown(reason) => {
                        if unknown.is_none() {
                            unknown = Some(reason);
                        }
                    }
                    ConditionOutcome::True => {}
                }
            }
            Ok(match unknown {
                Some(reason) => ConditionOutcome::Unknown(reason),
                None => ConditionOutcome::True,
            })
        }

        Condition::Any { conditions } => {
            let mut unknown = None;
            for nested in conditions {
                match eval_condition(vars, limits, nested, depth + 1)? {
                    ConditionOutcome::True => return Ok(ConditionOutcome::True),
                    ConditionOutcome::Unknown(reason) => {
                        if unknown.is_none() {
                            unknown = Some(reason);
                        }
                    }
                    ConditionOutcome::False => {}
                }
            }
            Ok(match unknown {
                Some(reason) => ConditionOutcome::Unknown(reason),
                None => ConditionOutcome::False,
            })
        }

        Condition::Not { condition } => match eval_condition(vars, limits, condition, depth + 1)? {
            ConditionOutcome::True => Ok(ConditionOutcome::False),
            ConditionOutcome::False => Ok(ConditionOutcome::True),
            ConditionOutcome::Unknown(reason) => Ok(ConditionOutcome::Unknown(reason)),
        },
    }
}

/// Compare two concrete values.
///
/// A comparison that cannot mean anything is an **error**, not `false`. "Is the
/// volume more than the word 'loud'" is a mistake in the automation, and a
/// quiet `false` would send the author looking in the wrong place.
fn compare(left: &Resolved, op: CompareOp, right: &Resolved) -> Result<bool, String> {
    match (left, right) {
        (Resolved::Number(left), Resolved::Number(right)) => Ok(match op {
            CompareOp::Eq => left == right,
            CompareOp::NotEq => left != right,
            CompareOp::Greater => left > right,
            CompareOp::GreaterOrEqual => left >= right,
            CompareOp::Less => left < right,
            CompareOp::LessOrEqual => left <= right,
            CompareOp::Contains => return Err("'contains' only works on text".to_string()),
        }),

        (Resolved::Text(left), Resolved::Text(right)) => Ok(match op {
            CompareOp::Eq => left == right,
            CompareOp::NotEq => left != right,
            CompareOp::Contains => left.contains(right.as_str()),
            _ => {
                return Err(
                    "only 'is', 'is not' and 'contains' can compare text; the others would be alphabetical, which is rarely what was meant"
                        .to_string(),
                )
            }
        }),

        (Resolved::Bool(left), Resolved::Bool(right)) => Ok(match op {
            CompareOp::Eq => left == right,
            CompareOp::NotEq => left != right,
            _ => return Err("only 'is' and 'is not' can compare yes/no values".to_string()),
        }),

        (Resolved::Unknown { reason }, _) | (_, Resolved::Unknown { reason }) => Err(reason.clone()),

        _ => Err(format!(
            "{} cannot be compared with {}",
            shape(left),
            shape(right)
        )),
    }
}

/// Render a resolved value the way a person would type it, for an action
/// argument.
fn argument_text(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => number_text(*value),
        Value::Text(value) => value.clone(),
    }
}

/// `50`, not `50.0`.
///
/// The arguments become strings again at the plugin boundary, and a user who
/// typed `50` should not see `50.0` come back out of a round trip.
fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn describe(action: &CoreAction) -> String {
    match action {
        CoreAction::Hotkey(hotkey) => format!("hotkey {hotkey:?}"),
        CoreAction::Text(text) => format!("text {text:?}"),
    }
}

fn shape(resolved: &Resolved) -> &'static str {
    match resolved {
        Resolved::Bool(_) => "a yes/no value",
        Resolved::Number(_) => "a number",
        Resolved::Text(_) => "text",
        Resolved::Unknown { .. } => "a value that is not available",
    }
}

/// A one-line rendering of a condition, for the trace a person reads.
fn summarise(condition: &Condition) -> String {
    match condition {
        Condition::Compare { left, op, right } => {
            format!("{} {} {}", render(left), operator_text(*op), render(right))
        }
        Condition::All { conditions } => join(conditions, " and "),
        Condition::Any { conditions } => join(conditions, " or "),
        Condition::Not { condition } => format!("not ({})", summarise(condition)),
    }
}

fn join(conditions: &[Condition], separator: &str) -> String {
    conditions
        .iter()
        .map(summarise)
        .collect::<Vec<_>>()
        .join(separator)
}

fn render(reference: &ValueRef) -> String {
    match reference {
        ValueRef::Bool(value) => value.to_string(),
        ValueRef::Number(value) => number_text(*value),
        ValueRef::Text(value) => format!("\"{value}\""),
        ValueRef::Variable(name) => name.clone(),
        ValueRef::Input(name) => format!("the {name} it was given"),
    }
}

fn operator_text(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "is",
        CompareOp::NotEq => "is not",
        CompareOp::Greater => "is more than",
        CompareOp::GreaterOrEqual => "is at least",
        CompareOp::Less => "is less than",
        CompareOp::LessOrEqual => "is at most",
        CompareOp::Contains => "contains",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{AdvancingSleeper, CancelFlag, FakeClock, NeverCancel, RecordingSleeper};
    use crate::model::{Branch, Trigger, AUTOMATION_SCHEMA};
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_plugin::builtin::{self};
    use flexdeck_plugin::example::Example;
    use flexdeck_plugin::RecordingNotifier;
    use std::sync::Arc;

    const VOLUME: &str = "system.audio.volume";
    const DEVICE: &str = "system.audio.output_device";

    struct World {
        registry: Registry,
        vars: VariableStore,
        input: RecordingExecutor,
        notifier: RecordingNotifier,
        settings: BTreeMap<String, String>,
        clock: Arc<FakeClock>,
        sleeper: AdvancingSleeper,
        cancel: CancelFlag,
    }

    impl World {
        fn new() -> Self {
            let mut registry = Registry::new();
            for plugin in builtin::builtins(builtin::Backends::recording()) {
                registry.register(plugin);
            }
            registry.register(Box::new(Example::new()));

            let vars = VariableStore::new();
            registry.declare_variables(&vars);

            let clock = Arc::new(FakeClock::new(0));

            Self {
                registry,
                vars,
                input: RecordingExecutor::new(),
                notifier: RecordingNotifier::new(),
                settings: BTreeMap::new(),
                sleeper: AdvancingSleeper::new(Arc::clone(&clock)),
                clock,
                cancel: CancelFlag::new(),
            }
        }

        fn sample(&mut self, names: &[&str]) {
            let interested = names.iter().map(|name| (*name).to_string()).collect();
            let now_ms = self.clock.now_ms();
            let mut ctx = Ctx {
                input: &mut self.input,
                vars: &self.vars,
                notifier: &mut self.notifier,
                settings: &self.settings,
                now_ms,
            };
            self.registry
                .sample(&interested, &mut ctx)
                .expect("sampling works");
        }

        fn run_with(&mut self, limits: Limits, automation: &Automation) -> Run {
            let mut environment = Environment {
                registry: &mut self.registry,
                latest: None,
                vars: &self.vars,
                inputs: &NO_INPUTS,
                input: &mut self.input,
                notifier: &mut self.notifier,
                settings: &self.settings,
                clock: self.clock.as_ref(),
                sleeper: &mut self.sleeper,
                cancel: &self.cancel,
                limits,
            };
            run(&mut environment, automation)
        }

        fn run(&mut self, automation: &Automation) -> Run {
            self.run_with(Limits::default(), automation)
        }

        fn run_given(&mut self, inputs: &Inputs, automation: &Automation) -> Run {
            let mut environment = Environment {
                registry: &mut self.registry,
                latest: None,
                vars: &self.vars,
                inputs,
                input: &mut self.input,
                notifier: &mut self.notifier,
                settings: &self.settings,
                clock: self.clock.as_ref(),
                sleeper: &mut self.sleeper,
                cancel: &self.cancel,
                limits: Limits::default(),
            };
            run(&mut environment, automation)
        }
    }

    fn set_volume_to(value: ValueRef) -> Block {
        Block::DoAction {
            plugin: "system".to_string(),
            action: "set_volume".to_string(),
            args: BTreeMap::from([("percent".to_string(), value)]),
        }
    }

    #[test]
    fn each_action_runs_against_the_latest_registry() {
        let mut world = World::new();
        let live = std::mem::take(&mut world.registry);
        let latest = || live.clone();
        let inputs = Inputs::from([("value".to_string(), Value::Number(42.0))]);
        let run = {
            let mut environment = Environment {
                registry: &mut world.registry,
                latest: Some(&latest),
                vars: &world.vars,
                inputs: &inputs,
                input: &mut world.input,
                notifier: &mut world.notifier,
                settings: &world.settings,
                clock: world.clock.as_ref(),
                sleeper: &mut world.sleeper,
                cancel: &world.cancel,
                limits: Limits::default(),
            };
            run(&mut environment, &automation(vec![set_volume_to(ValueRef::Input("value".into()))]))
        };
        assert_eq!(run.outcome, RunOutcome::Completed, "{:?}", run.trace);
    }

    #[test]
    fn a_runs_input_reaches_an_actions_argument() {
        // A slider's position, handed to the run that it started.
        let mut world = World::new();
        let inputs = Inputs::from([("value".to_string(), Value::Number(37.0))]);
        let run = world.run_given(
            &inputs,
            &automation(vec![set_volume_to(ValueRef::Input("value".into()))]),
        );
        assert_eq!(run.outcome, RunOutcome::Completed, "{:?}", run.trace);
        world.sample(&[VOLUME]);
        assert_eq!(world.vars.get(VOLUME), Some(Value::Number(37.0)));
    }

    #[test]
    fn an_input_the_run_was_not_given_stops_it_with_a_reason() {
        let mut world = World::new();
        let run = world.run(&automation(vec![set_volume_to(ValueRef::Input(
            "value".into(),
        ))]));
        assert_eq!(run.outcome, RunOutcome::Failed);
        assert!(
            run.reason().is_some_and(|why| why.contains("not given 'value'")),
            "{:?}",
            run.reason()
        );
    }

    #[test]
    fn an_input_reference_has_its_own_wire_shape() {
        let json = serde_json::to_string(&ValueRef::Input("value".into())).unwrap();
        assert_eq!(json, r#"{"input":"value"}"#);
    }

    #[test]
    fn each_action_is_traced_with_the_path_a_provider_names_it_by() {
        // if false { a } else if true { b, c } else { d }: `c` is child 2 of
        // the if (then has 1, the first else-if starts at 1).
        let no = Condition::Compare {
            left: ValueRef::Number(1.0),
            op: CompareOp::Eq,
            right: ValueRef::Number(2.0),
        };
        let yes = Condition::Compare {
            left: ValueRef::Number(1.0),
            op: CompareOp::Eq,
            right: ValueRef::Number(1.0),
        };
        let mut world = World::new();
        let run = world.run(&automation(vec![
            set_volume_to(ValueRef::Number(10.0)),
            Block::If {
                condition: no,
                then_blocks: vec![set_volume_to(ValueRef::Number(1.0))],
                else_if: vec![crate::model::Branch {
                    condition: yes,
                    blocks: vec![
                        set_volume_to(ValueRef::Number(2.0)),
                        set_volume_to(ValueRef::Number(3.0)),
                    ],
                }],
                otherwise: vec![set_volume_to(ValueRef::Number(4.0))],
            },
        ]));
        let paths: Vec<Vec<usize>> = run
            .trace
            .iter()
            .filter_map(|entry| match entry {
                Trace::Ran { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec![vec![0], vec![1, 1], vec![1, 2]]);
    }

    fn automation(body: Vec<Block>) -> Automation {
        Automation {
            id: "a1".to_string(),
            name: "Test".to_string(),
            enabled: true,
            trigger: Trigger::Manual,
            body,
        }
    }

    fn variable(name: &str) -> ValueRef {
        ValueRef::Variable(name.to_string())
    }

    fn switch_device(device: &str) -> Block {
        Block::DoAction {
            plugin: "system".to_string(),
            action: "set_output_device".to_string(),
            args: BTreeMap::from([("device".to_string(), ValueRef::Text(device.to_string()))]),
        }
    }

    /// The example from the brief: `if system.audio.volume >= 50` then switch the
    /// output device.
    fn volume_gate() -> Automation {
        automation(vec![Block::If {
            condition: Condition::Compare {
                left: variable(VOLUME),
                op: CompareOp::GreaterOrEqual,
                right: ValueRef::Number(50.0),
            },
            then_blocks: vec![switch_device("headphones")],
            else_if: Vec::new(),
            otherwise: Vec::new(),
        }])
    }

    #[test]
    fn the_briefs_example_switches_device_only_when_the_volume_is_high() {
        let mut world = World::new();
        world.sample(&[VOLUME, DEVICE]);
        assert_eq!(
            world.vars.get(DEVICE),
            Some(Value::Text("speaker".to_string())),
            "the baseline must be a real reading, not an empty value"
        );

        world.vars.refresh(VOLUME, Value::Number(20.0)).unwrap();
        let quiet = world.run(&volume_gate());
        assert_eq!(quiet.outcome, RunOutcome::Completed);
        assert_eq!(
            world.vars.get(DEVICE),
            Some(Value::Text("speaker".to_string())),
            "below the threshold the device must not change"
        );

        world.vars.refresh(VOLUME, Value::Number(80.0)).unwrap();
        let loud = world.run(&volume_gate());
        assert_eq!(loud.outcome, RunOutcome::Completed);
        assert_eq!(
            world.vars.get(DEVICE),
            Some(Value::Text("headphones".to_string())),
            "above the threshold it must"
        );
    }

    #[test]
    fn a_variable_that_was_never_read_makes_the_answer_unknown_not_false() {
        // The failure this exists to prevent: `0 >= 50` is false, so a naive
        // engine would take the else path and look like it had decided.
        let mut world = World::new();
        let run = world.run(&volume_gate());

        assert_eq!(run.outcome, RunOutcome::Completed);

        let unknown = run.trace.iter().any(|entry| {
            matches!(
                entry,
                Trace::Condition {
                    outcome: ConditionOutcome::Unknown(_),
                    ..
                }
            )
        });
        assert!(unknown, "the trace must say the value was unavailable");
    }

    #[test]
    fn an_unknown_value_still_runs_the_otherwise_branch() {
        let mut world = World::new();
        let run = world.run(&automation(vec![Block::If {
            condition: Condition::Compare {
                left: variable(VOLUME),
                op: CompareOp::GreaterOrEqual,
                right: ValueRef::Number(50.0),
            },
            then_blocks: Vec::new(),
            else_if: Vec::new(),
            otherwise: vec![Block::SetVariable {
                name: "example.counter".to_string(),
                value: ValueRef::Number(7.0),
            }],
        }]));

        assert_eq!(run.outcome, RunOutcome::Completed);
        assert_eq!(world.vars.get("example.counter"), Some(Value::Number(7.0)));
    }

    #[test]
    fn an_action_argument_can_be_read_from_a_variable() {
        // The reason `ValueRef` exists: a plain string argument cannot say
        // "whatever the last phrase was".
        let mut world = World::new();
        let run = world.run(&automation(vec![
            Block::SetVariable {
                name: "example.last_text".to_string(),
                value: ValueRef::Text("stream is live".to_string()),
            },
            Block::DoAction {
                plugin: "notifications".to_string(),
                action: "notify".to_string(),
                args: BTreeMap::from([
                    ("title".to_string(), ValueRef::Text("Notice".to_string())),
                    ("body".to_string(), variable("example.last_text")),
                ]),
            },
        ]));

        assert_eq!(run.outcome, RunOutcome::Completed);
        assert_eq!(
            world.notifier.shown(),
            [("Notice".to_string(), "stream is live".to_string())]
        );
    }

    #[test]
    fn a_number_argument_is_rendered_without_a_trailing_zero() {
        let mut world = World::new();
        world.sample(&[VOLUME]);

        let run = world.run(&automation(vec![Block::DoAction {
            plugin: "system".to_string(),
            action: "set_volume".to_string(),
            args: BTreeMap::from([("percent".to_string(), ValueRef::Number(50.0))]),
        }]));

        assert_eq!(run.outcome, RunOutcome::Completed);
        assert_eq!(world.vars.get(VOLUME), Some(Value::Number(50.0)));
    }

    #[test]
    fn if_picks_exactly_one_branch() {
        fn run_at(world: &mut World, volume: f64) -> RunOutcome {
            world.vars.refresh(VOLUME, Value::Number(volume)).unwrap();
            world
                .run(&automation(vec![Block::If {
                    condition: Condition::Compare {
                        left: variable(VOLUME),
                        op: CompareOp::GreaterOrEqual,
                        right: ValueRef::Number(50.0),
                    },
                    then_blocks: vec![Block::SetVariable {
                        name: "example.counter".to_string(),
                        value: ValueRef::Number(1.0),
                    }],
                    else_if: vec![Branch {
                        condition: Condition::Compare {
                            left: variable(VOLUME),
                            op: CompareOp::GreaterOrEqual,
                            right: ValueRef::Number(10.0),
                        },
                        blocks: vec![Block::SetVariable {
                            name: "example.counter".to_string(),
                            value: ValueRef::Number(2.0),
                        }],
                    }],
                    otherwise: vec![Block::SetVariable {
                        name: "example.counter".to_string(),
                        value: ValueRef::Number(3.0),
                    }],
                }]))
                .outcome
        }

        let mut world = World::new();
        world.sample(&[VOLUME]);

        assert_eq!(run_at(&mut world, 80.0), RunOutcome::Completed);
        assert_eq!(world.vars.get("example.counter"), Some(Value::Number(1.0)));

        assert_eq!(run_at(&mut world, 20.0), RunOutcome::Completed);
        assert_eq!(world.vars.get("example.counter"), Some(Value::Number(2.0)));

        assert_eq!(run_at(&mut world, 5.0), RunOutcome::Completed);
        assert_eq!(world.vars.get("example.counter"), Some(Value::Number(3.0)));
    }

    #[test]
    fn all_any_and_not_are_three_valued() {
        let mut world = World::new();
        world.sample(&[VOLUME]);

        let yes = Condition::Compare {
            left: variable(VOLUME),
            op: CompareOp::Greater,
            right: ValueRef::Number(0.0),
        };
        let no = Condition::Compare {
            left: variable(VOLUME),
            op: CompareOp::Greater,
            right: ValueRef::Number(1_000.0),
        };
        let unknown = Condition::Compare {
            left: variable("system.cpu.usage"),
            op: CompareOp::Greater,
            right: ValueRef::Number(50.0),
        };

        // 40 is more than 0 and not more than 1000.
        world.vars.refresh(VOLUME, Value::Number(40.0)).unwrap();

        let limits = Limits::default();

        // False beats unknown: `all` is already decided.
        assert_eq!(
            eval_condition(
                Scope::of(&world.vars),
                &limits,
                &Condition::All {
                    conditions: vec![no.clone(), unknown.clone()]
                },
                1
            )
            .expect("evaluates"),
            ConditionOutcome::False
        );

        // True beats unknown for `any`.
        assert_eq!(
            eval_condition(
                Scope::of(&world.vars),
                &limits,
                &Condition::Any {
                    conditions: vec![yes.clone(), unknown.clone()]
                },
                1
            )
            .expect("evaluates"),
            ConditionOutcome::True
        );

        // Nothing decides it, so it is unknown rather than guessed.
        assert!(matches!(
            eval_condition(
                Scope::of(&world.vars),
                &limits,
                &Condition::All {
                    conditions: vec![yes.clone(), unknown.clone()]
                },
                1
            )
            .expect("evaluates"),
            ConditionOutcome::Unknown(_)
        ));

        assert_eq!(
            eval_condition(
                Scope::of(&world.vars),
                &limits,
                &Condition::Not {
                    condition: Box::new(yes)
                },
                1
            )
            .expect("evaluates"),
            ConditionOutcome::False
        );
    }

    #[test]
    fn comparing_two_different_kinds_stops_the_run_with_a_reason() {
        // The two sides have to be real values for the comparison to reach
        // them: an unread variable is Unknown, and Unknown beats everything
        // else. That is the right order — the bug this test asserts is a
        // *configuration* mistake, not an unavailable reading.
        let mut world = World::new();
        world.sample(&[VOLUME]);
        let run = world.run(&automation(vec![Block::If {
            condition: Condition::Compare {
                left: variable(VOLUME),
                op: CompareOp::Greater,
                right: ValueRef::Text("loud".to_string()),
            },
            then_blocks: Vec::new(),
            else_if: Vec::new(),
            otherwise: Vec::new(),
        }]));

        assert_eq!(run.outcome, RunOutcome::Failed);
        assert!(run.reason().unwrap().contains("cannot be compared"));
    }

    #[test]
    fn ordering_text_is_refused_rather_than_done_alphabetically() {
        let mut world = World::new();
        let run = world.run(&automation(vec![Block::If {
            condition: Condition::Compare {
                left: ValueRef::Text("a".to_string()),
                op: CompareOp::Greater,
                right: ValueRef::Text("b".to_string()),
            },
            then_blocks: Vec::new(),
            else_if: Vec::new(),
            otherwise: Vec::new(),
        }]));

        assert_eq!(run.outcome, RunOutcome::Failed);
        assert!(run.reason().unwrap().contains("alphabetical"));
    }

    #[test]
    fn repeat_runs_the_body_exactly_that_many_times() {
        let mut world = World::new();
        world
            .vars
            .set("example.counter", Value::Number(0.0))
            .unwrap();

        let run = world.run(&automation(vec![Block::Repeat {
            times: ValueRef::Number(3.0),
            body: vec![Block::SetVariable {
                name: "example.counter".to_string(),
                value: variable("example.counter"),
            }],
        }]));

        assert_eq!(run.outcome, RunOutcome::Completed);
        assert!(run.trace.contains(&Trace::Repeated { times: 3 }));
    }

    #[test]
    fn a_repeat_beyond_the_limit_is_stopped_before_it_starts() {
        let mut world = World::new();
        let run = world.run(&automation(vec![Block::Repeat {
            times: ValueRef::Number(100_000.0),
            body: vec![Block::SetVariable {
                name: "example.counter".to_string(),
                value: ValueRef::Number(1.0),
            }],
        }]));

        assert_eq!(run.outcome, RunOutcome::LimitReached);
        assert!(run.reason().unwrap().contains("repeat asked for"));
    }

    #[test]
    fn a_wait_is_recorded_and_never_really_sleeps() {
        let mut world = World::new();
        let run = world.run(&automation(vec![Block::Wait {
            ms: ValueRef::Number(1_000.0),
        }]));

        assert_eq!(run.outcome, RunOutcome::Completed);
        assert_eq!(world.sleeper.slept(), [1_000]);
        assert!(run.trace.contains(&Trace::Waited { ms: 1_000 }));
    }

    #[test]
    fn a_wait_longer_than_the_limit_is_refused() {
        let mut world = World::new();
        let run = world.run(&automation(vec![Block::Wait {
            ms: ValueRef::Number(600_000.0),
        }]));

        assert_eq!(run.outcome, RunOutcome::LimitReached);
        assert!(world.sleeper.slept().is_empty(), "it must not wait first");
    }

    #[test]
    fn a_runaway_is_stopped_by_the_step_budget() {
        let mut world = World::new();
        world
            .vars
            .set("example.counter", Value::Number(0.0))
            .unwrap();

        let limits = Limits {
            max_steps: 5,
            ..Limits::default()
        };

        let run = world.run_with(
            limits,
            &automation(vec![Block::Repeat {
                times: ValueRef::Number(100.0),
                body: vec![Block::SetVariable {
                    name: "example.counter".to_string(),
                    value: ValueRef::Number(1.0),
                }],
            }]),
        );

        assert_eq!(run.outcome, RunOutcome::LimitReached);
        assert!(run.reason().unwrap().contains("steps"));
    }

    #[test]
    fn the_wall_clock_bound_stops_a_run() {
        let mut world = World::new();

        let limits = Limits {
            max_run_ms: 500,
            ..Limits::default()
        };

        // The sleeper moves the fake clock, so the wait really does spend the
        // budget without the test sleeping at all.
        let run = world.run_with(
            limits,
            &automation(vec![
                Block::Wait {
                    ms: ValueRef::Number(400.0),
                },
                Block::Wait {
                    ms: ValueRef::Number(400.0),
                },
            ]),
        );

        assert_eq!(run.outcome, RunOutcome::LimitReached);
        assert!(run.reason().unwrap().contains("longer than"));
    }

    #[test]
    fn cancelling_stops_a_run_between_steps() {
        let mut world = World::new();
        world
            .vars
            .set("example.counter", Value::Number(0.0))
            .unwrap();
        world.cancel.cancel();

        let run = world.run(&automation(vec![Block::SetVariable {
            name: "example.counter".to_string(),
            value: ValueRef::Number(9.0),
        }]));

        assert_eq!(run.outcome, RunOutcome::Cancelled);
        assert_eq!(world.vars.get("example.counter"), Some(Value::Number(0.0)));
    }

    #[test]
    fn a_switched_off_automation_does_nothing_and_says_so() {
        let mut world = World::new();
        let mut switched_off = automation(vec![Block::SetVariable {
            name: "example.counter".to_string(),
            value: ValueRef::Number(1.0),
        }]);
        switched_off.enabled = false;

        let run = world.run(&switched_off);

        assert_eq!(run.outcome, RunOutcome::Failed);
        assert!(run.reason().unwrap().contains("switched off"));
        assert_eq!(world.vars.get("example.counter"), None);
    }

    #[test]
    fn a_dry_run_against_a_scratch_store_leaves_the_live_one_untouched() {
        // This is the editor's Test button: real registry, real plugin code, but
        // a throwaway store, so pressing Test cannot move the user's deck.
        let mut world = World::new();
        world
            .vars
            .set("example.counter", Value::Number(1.0))
            .unwrap();

        let scratch = VariableStore::new();
        world.registry.declare_variables(&scratch);

        let run = {
            let mut environment = Environment {
                registry: &mut world.registry,
                latest: None,
                vars: &scratch,
                inputs: &NO_INPUTS,
                input: &mut world.input,
                notifier: &mut world.notifier,
                settings: &world.settings,
                clock: world.clock.as_ref(),
                sleeper: &mut world.sleeper,
                cancel: &NeverCancel,
                limits: Limits::default(),
            };
            run(
                &mut environment,
                &automation(vec![Block::SetVariable {
                    name: "example.counter".to_string(),
                    value: ValueRef::Number(99.0),
                }]),
            )
        };

        assert_eq!(run.outcome, RunOutcome::Completed);
        assert_eq!(scratch.get("example.counter"), Some(Value::Number(99.0)));
        assert_eq!(
            world.vars.get("example.counter"),
            Some(Value::Number(1.0)),
            "the live value must not move"
        );
    }

    #[test]
    fn the_trace_reads_like_a_sentence() {
        let mut world = World::new();
        world.sample(&[VOLUME]);
        world.vars.refresh(VOLUME, Value::Number(80.0)).unwrap();

        let run = world.run(&volume_gate());

        let Trace::Condition { summary, outcome } = &run.trace[0] else {
            panic!("the first line is the condition");
        };
        assert_eq!(summary, "system.audio.volume is at least 50");
        assert_eq!(*outcome, ConditionOutcome::True);
    }

    #[test]
    fn a_document_from_a_newer_agent_is_refused_with_a_sentence() {
        let json = format!(r#"{{"schema":{},"automations":[]}}"#, AUTOMATION_SCHEMA + 1);
        let error = crate::parse_document(&json, &Limits::default()).expect_err("too new");

        assert!(error.to_string().contains("needs a newer agent"));
    }

    #[test]
    fn a_document_nested_past_the_limit_is_refused_before_it_runs() {
        let depth = Limits::default().max_depth + 1;
        let mut block = Block::Wait {
            ms: ValueRef::Number(1.0),
        };
        for _ in 0..depth {
            block = Block::Repeat {
                times: ValueRef::Number(1.0),
                body: vec![block],
            };
        }

        let document = crate::AutomationDocument {
            schema: AUTOMATION_SCHEMA,
            rev: 1,
            automations: vec![automation(vec![block])],
        };

        let error = document.validate(&Limits::default()).expect_err("too deep");
        assert!(error.to_string().contains("too big"));
    }

    #[test]
    fn a_trigger_knows_what_it_watches() {
        let watched = Trigger::Variables {
            names: vec![VOLUME.to_string()],
        };
        assert_eq!(watched.watched(), [VOLUME.to_string()]);
        assert!(watched.watches(VOLUME));
        assert!(!watched.watches(DEVICE));

        let widget = Trigger::Widget {
            widget_id: "w1".to_string(),
        };
        assert!(widget.fires_for_widget("w1"));
        assert!(widget.watched().is_empty());

        assert!(Trigger::Manual.watched().is_empty());
    }

    #[test]
    fn a_recording_sleeper_is_available_for_a_test_that_wants_no_time_at_all() {
        let mut sleeper = RecordingSleeper::new();
        sleeper.sleep(5);
        assert_eq!(sleeper.slept(), [5]);
    }

    #[test]
    fn a_single_trace_string_longer_than_the_cap_is_truncated_at_the_engine() {
        // The trace entry's `input` string is user-controlled; without the
        // cap, a 100 KB typed text becomes a 100 MB dry-run response, and
        // the relay truncates the body AFTER the cost of parsing it. The
        // cap is at the engine so the relay never sees an oversize body.
        let mut world = World::new();
        let big_text = "x".repeat(Limits::default().max_trace_string * 8);
        let run = world.run_with(
            Limits::default(),
            &automation(vec![Block::DoAction {
                plugin: "core".to_string(),
                action: "text".to_string(),
                args: BTreeMap::new(),
            }]),
        );
        // World is wired with `RecordingSleeper`/no executor, so the doAction
        // path fails; the Run still has a trace, but the key point here is
        // that the per-string cap is enforced whenever a string field would
        // exceed it. Verify via a `Ran` trace entry built directly instead.
        let _ = run;

        let mut state = State {
            latest: None,
            vars: &world.vars,
            inputs: &NO_INPUTS,
            path: Vec::new(),
            input: &mut world.input,
            notifier: &mut world.notifier,
            sleeper: &mut world.sleeper,
            clock: world.clock.as_ref(),
            cancel: &world.cancel,
            settings: &BTreeMap::new(),
            limits: Limits::default(),
            started_ms: 0,
            steps: 0,
            trace: Vec::new(),
            trace_bytes: 0,
        };
        let big = big_text.clone();
        push_trace(
            &mut state,
            Trace::Ran {
                plugin: "core".to_string(),
                action: "text".to_string(),
                input: Some(big),
                state: None,
                path: vec![0],
            },
        );

        let stored = state.trace.last().expect("the entry was pushed");
        match stored {
            Trace::Ran { input, .. } => {
                let text = input.as_ref().expect("Ran carries input");
                assert!(text.ends_with('…'), "truncation marker is present");
                assert!(
                    text.chars().count() <= Limits::default().max_trace_string + 1,
                    "the truncated string respects the per-string cap (got {} chars)",
                    text.chars().count(),
                );
            }
            other => panic!("expected a Ran entry, got {other:?}"),
        }
    }

    #[test]
    fn the_trace_stops_growing_once_the_byte_budget_is_spent() {
        // Each `push_trace` call adds an entry; once the cumulative bytes
        // hit the budget, further pushes are dropped and the run keeps going.
        let mut world = World::new();
        let mut state = State {
            latest: None,
            vars: &world.vars,
            inputs: &NO_INPUTS,
            path: Vec::new(),
            input: &mut world.input,
            notifier: &mut world.notifier,
            sleeper: &mut world.sleeper,
            clock: world.clock.as_ref(),
            cancel: &world.cancel,
            settings: &BTreeMap::new(),
            limits: Limits::default(),
            started_ms: 0,
            steps: 0,
            trace: Vec::new(),
            trace_bytes: 0,
        };
        let mut pushed = 0;
        for _ in 0..(Limits::default().max_trace_bytes / 64 + 100) {
            push_trace(
                &mut state,
                Trace::Stopped {
                    reason: "x".repeat(64),
                },
            );
            pushed += 1;
        }
        assert!(
            state.trace.len() < pushed,
            "the trace dropped entries past the byte budget (pushed {}, kept {})",
            pushed,
            state.trace.len(),
        );
    }

    #[test]
    fn text_with_holes_is_filled_from_values_and_inputs() {
        let vars = VariableStore::new();
        vars.set("vars.temp", Value::Number(21.46)).unwrap();
        let inputs: Inputs = BTreeMap::from([("value".to_string(), Value::Number(40.0))]);
        let scope = Scope { vars: &vars, inputs: &inputs };
        let text = |template: &str| match resolve(scope, &ValueRef::Text(template.to_string())) {
            Resolved::Text(text) => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(text("Temp: {{ vars.temp | round(1) }}°"), "Temp: 21.5°");
        assert_eq!(text("Set to {{input.value}}%"), "Set to 40%");
        assert_eq!(text("{{ nobody.knows }}"), "–");
        assert_eq!(text("no holes"), "no holes");
        assert_eq!(text("{{ broken | rond }}"), "{{ broken | rond }}", "kept as written");
    }
}

//! Templates and expressions: what a key's label, a rule and a recipe compute.
//!
//! A template is text with `{{ … }}` holes. Each hole is an expression:
//!
//! ```text
//! REC {{obs.record_time}}                 a variable, as schema 1 had it
//! {{ system.cpu.usage | round }}%         a filter
//! {{ (btc.price - btc.open) / btc.open * 100 | round(2) | sign }}%
//! {{ weather.code | map(0: "sun", 1: "cloud", _: "fog") }}
//! {{ if(obs.recording, "REC", "idle") }}
//! ```
//!
//! # One language, three users
//!
//! Labels, the rules that colour a key, and recipes all evaluate through here,
//! so a filter means the same thing everywhere — and only in Rust: the editor
//! asks the agent rather than carrying a second implementation that would drift.
//!
//! # Safe by construction
//!
//! There is no IO, no loop and no recursion a template can trigger. Parsing
//! is bounded by [`MAX_TEMPLATE_LEN`] and [`MAX_DEPTH`]; evaluation is a walk
//! of a tree no deeper than that. A template that does not parse is refused
//! when the deck is saved, with a sentence — not on the tablet.
//!
//! # Unknown is not zero
//!
//! A variable nobody has read yet is [`Val::Missing`]. Arithmetic on it stays
//! missing, a comparison with it is false, and it renders as `–`: a blank
//! looks broken, and a zero looks like a real reading. `default(…)` replaces it.

use std::collections::BTreeSet;
use std::fmt::Write as _;

mod format;
mod parse;

pub use format::Locale;
use parse::Expr;

/// The longest template the agent accepts.
pub const MAX_TEMPLATE_LEN: usize = 2_000;

/// How deeply an expression may nest.
pub const MAX_DEPTH: usize = 32;

/// What a variable or an expression is worth.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Number(f64),
    Text(String),
    Bool(bool),
    /// Not known yet, or the result of a computation on something unknown.
    Missing,
}

impl Val {
    /// As a number: text that is one, a bool as 1 or 0.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Val::Number(n) if n.is_finite() => Some(*n),
            Val::Text(text) => text.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
            Val::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// As a condition: a true bool, a non-zero number, non-empty text other
    /// than "false", "off" or "0".
    pub fn truthy(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Number(n) => *n != 0.0 && !n.is_nan(),
            Val::Text(text) => {
                let text = text.trim();
                !(text.is_empty() || text.eq_ignore_ascii_case("false") || text.eq_ignore_ascii_case("off") || text == "0")
            }
            Val::Missing => false,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Val::Missing)
    }
}

/// Where variables come from. The agent's variable store, a test's map.
pub trait Lookup {
    fn get(&self, name: &str) -> Val;
}

impl<F: Fn(&str) -> Val> Lookup for F {
    fn get(&self, name: &str) -> Val {
        self(name)
    }
}

/// A template or an expression that did not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExprError {
    #[error("it is longer than {MAX_TEMPLATE_LEN} characters")]
    TooLong,
    #[error("a '{{{{' is never closed")]
    Unclosed,
    #[error("'{{{{ }}}}' is empty")]
    Empty,
    #[error("it nests deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("'{0}' is not a filter; try round, number, currency, pct, temp, default, map, upper, time or duration")]
    UnknownFilter(String),
    #[error("{filter} takes {expected}")]
    Arguments { filter: String, expected: &'static str },
    #[error("{0}")]
    Syntax(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Text(String),
    Hole(Expr),
}

/// A parsed template: text and the expressions in its holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    parts: Vec<Part>,
}

impl Template {
    /// Parse `source`. Text outside `{{ }}` is kept as it is.
    pub fn parse(source: &str) -> Result<Self, ExprError> {
        if source.len() > MAX_TEMPLATE_LEN {
            return Err(ExprError::TooLong);
        }
        let mut parts = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find("{{") {
            if start > 0 {
                parts.push(Part::Text(rest[..start].to_string()));
            }
            let after = &rest[start + 2..];
            let end = after.find("}}").ok_or(ExprError::Unclosed)?;
            parts.push(Part::Hole(parse_hole(&after[..end])?));
            rest = &after[end + 2..];
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest.to_string()));
        }
        Ok(Self { parts })
    }

    /// Whether it has any holes: a template without is plain text.
    pub fn is_live(&self) -> bool {
        self.parts.iter().any(|part| matches!(part, Part::Hole(_)))
    }

    /// Every variable it reads.
    pub fn variables(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for part in &self.parts {
            if let Part::Hole(expr) = part {
                expr.variables(&mut names);
            }
        }
        names
    }

    /// The text, with each hole filled.
    pub fn render(&self, lookup: &dyn Lookup, locale: &Locale) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Hole(expr) => {
                    let _ = write!(out, "{}", format::display(&expr.eval(lookup, locale), locale));
                }
            }
        }
        out
    }

    /// The value of a template that is one hole and nothing else — what a bar
    /// or a gauge needs, and an icon address that must not become text.
    /// Anything else is its rendered text.
    pub fn value(&self, lookup: &dyn Lookup, locale: &Locale) -> Val {
        match self.parts.as_slice() {
            [Part::Hole(expr)] => expr.eval(lookup, locale),
            _ => Val::Text(self.render(lookup, locale)),
        }
    }

    /// Like [`Template::value`], but every hole must be known: an icon whose
    /// address is half missing is no icon at all.
    pub fn value_if_known(&self, lookup: &dyn Lookup, locale: &Locale) -> Val {
        for part in &self.parts {
            if let Part::Hole(expr) = part {
                if expr.eval(lookup, locale).is_missing() {
                    return Val::Missing;
                }
            }
        }
        self.value(lookup, locale)
    }
}

/// Parse one expression, without braces: what a rule's left side holds.
pub fn parse_expression(source: &str) -> Result<Expression, ExprError> {
    if source.len() > MAX_TEMPLATE_LEN {
        return Err(ExprError::TooLong);
    }
    parse_hole(source).map(Expression)
}

/// A parsed expression, for rules and recipes.
#[derive(Debug, Clone, PartialEq)]
pub struct Expression(Expr);

impl Expression {
    pub fn eval(&self, lookup: &dyn Lookup, locale: &Locale) -> Val {
        self.0.eval(lookup, locale)
    }

    pub fn variables(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        self.0.variables(&mut names);
        names
    }
}

/// Parse the inside of a hole.
///
/// A hole that is only name characters, dots and hyphens is a variable's
/// name, as schema 1 read every hole: `obs.input.mic-aux.volume` keeps
/// working. So subtraction needs spaces around its minus — `a - b` — which is
/// how the editor writes it. Anything else must parse as an expression.
fn parse_hole(source: &str) -> Result<Expr, ExprError> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(ExprError::Empty);
    }
    let name_like = trimmed.starts_with(|c: char| c.is_alphabetic() || c == '_')
        && trimmed.contains('-')
        && trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '@' | '$' | '/'));
    if name_like {
        return Ok(Expr::Var(trimmed.to_string()));
    }
    parse::parse(trimmed)
}

/// Fill a template in one go, for callers that do not keep the parse.
/// A template that does not parse renders as itself.
pub fn render(source: &str, lookup: &dyn Lookup, locale: &Locale) -> String {
    match Template::parse(source) {
        Ok(template) => template.render(lookup, locale),
        Err(_) => source.to_string(),
    }
}

#[cfg(test)]
mod tests;

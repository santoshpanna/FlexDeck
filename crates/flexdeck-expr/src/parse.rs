//! The expression grammar, and evaluating it.
//!
//! ```text
//! pipe    := or ('|' filter)*
//! or      := and (('||' | 'or') and)*
//! and     := cmp (('&&' | 'and') cmp)*
//! cmp     := add (('==' | '!=' | '<' | '<=' | '>' | '>=') add)?
//! add     := mul (('+' | '-') mul)*
//! mul     := unary (('*' | '/' | '%') unary)*
//! unary   := ('-' | '!' | 'not') unary | primary
//! primary := number | string | true | false | path | call | '(' pipe ')'
//! call    := name '(' (arg (',' arg)*)? ')'
//! filter  := name ('(' (arg (',' arg)*)? ')')?
//! arg     := pipe (':' pipe)?
//! ```
//!
//! A filter is a function whose first argument is what is piped into it, so
//! `x | round(1)` and `round(x, 1)` are the same call.

use std::collections::BTreeSet;

use crate::format::{self, Locale};
use crate::{ExprError, Lookup, Val, MAX_DEPTH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arg {
    pub key: Option<Expr>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    /// A function or a filter: `args[0]` is the piped value for a filter.
    Call(String, Vec<Arg>),
}

/// Every function, how many arguments it takes (with the piped one), and a
/// phrase for when that is wrong.
const FUNCTIONS: &[(&str, usize, usize, &str)] = &[
    ("round", 1, 2, "at most one number of decimals: round(1)"),
    ("number", 1, 2, "at most one number of decimals: number(2)"),
    ("pct", 1, 2, "at most one number of decimals: pct(1)"),
    ("currency", 1, 3, "a currency code and decimals: currency(\"EUR\", 2)"),
    ("abs", 1, 1, "nothing"),
    ("floor", 1, 1, "nothing"),
    ("ceil", 1, 1, "nothing"),
    ("sign", 1, 1, "nothing"),
    ("clamp", 3, 3, "a lowest and a highest value: clamp(0, 100)"),
    ("min", 2, 2, "one other value: min(100)"),
    ("max", 2, 2, "one other value: max(0)"),
    ("upper", 1, 1, "nothing"),
    ("lower", 1, 1, "nothing"),
    ("trim", 1, 1, "nothing"),
    ("truncate", 2, 2, "a length: truncate(12)"),
    ("default", 2, 2, "the value to show instead: default(\"?\")"),
    ("map", 2, usize::MAX, "pairs: map(0: \"sun\", 1: \"cloud\", _: \"fog\")"),
    ("time", 1, 2, "at most a format: time(\"%H:%M\")"),
    ("duration", 1, 1, "nothing"),
    ("temp", 1, 2, "at most one number of decimals: temp(1)"),
    ("bytes", 1, 1, "nothing"),
    ("if", 3, 3, "a condition, then two values: if(x > 1, \"many\", \"one\")"),
];

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Text(String),
    Name(String),
    Punct(&'static str),
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

const PUNCT: &[&str] = &[
    "==", "!=", "<=", ">=", "&&", "||", "(", ")", ",", ":", "|", "+", "-", "*", "/", "%", "<", ">", "!",
];

fn lex(source: &str) -> Result<Vec<Token>, ExprError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let number = text
                .parse::<f64>()
                .map_err(|_| ExprError::Syntax(format!("'{text}' is not a number")))?;
            tokens.push(Token::Number(number));
        } else if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            let mut text = String::new();
            loop {
                match chars.get(i) {
                    None => return Err(ExprError::Syntax("a quote is never closed".into())),
                    Some('\\') if chars.get(i + 1).is_some() => {
                        text.push(chars[i + 1]);
                        i += 2;
                    }
                    Some(&ch) if ch == quote => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        text.push(ch);
                        i += 1;
                    }
                }
            }
            tokens.push(Token::Text(text));
        } else if c.is_alphabetic() || c == '_' || c == '@' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '@' | '$' | '.'))
            {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if name.ends_with('.') {
                return Err(ExprError::Syntax(format!("'{name}' ends with a dot")));
            }
            tokens.push(Token::Name(name));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let punct = PUNCT
                .iter()
                .find(|p| rest.starts_with(**p))
                .ok_or_else(|| ExprError::Syntax(format!("'{c}' is not understood here")))?;
            i += punct.chars().count();
            tokens.push(Token::Punct(punct));
        }
    }
    Ok(tokens)
}

/// Parse one expression.
pub(crate) fn parse(source: &str) -> Result<Expr, ExprError> {
    Parser { tokens: lex(source)?, at: 0, depth: 0 }.parse()
}

impl Parser {
    fn parse(mut self) -> Result<Expr, ExprError> {
        let expr = self.pipe()?;
        match self.tokens.get(self.at) {
            None => Ok(expr),
            Some(token) => Err(ExprError::Syntax(format!("{} was not expected here", describe(token)))),
        }
    }

    fn peek_punct(&self, punct: &str) -> bool {
        matches!(self.tokens.get(self.at), Some(Token::Punct(p)) if *p == punct)
    }

    fn peek_word(&self, word: &str) -> bool {
        matches!(self.tokens.get(self.at), Some(Token::Name(n)) if n == word)
    }

    fn eat_punct(&mut self, punct: &str) -> bool {
        if self.peek_punct(punct) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, punct: &str) -> Result<(), ExprError> {
        if self.eat_punct(punct) {
            Ok(())
        } else {
            Err(ExprError::Syntax(match self.tokens.get(self.at) {
                Some(token) => format!("expected '{punct}' but found {}", describe(token)),
                None => format!("expected '{punct}' at the end"),
            }))
        }
    }

    fn deeper(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(ExprError::TooDeep)
        } else {
            Ok(())
        }
    }

    fn pipe(&mut self) -> Result<Expr, ExprError> {
        self.deeper()?;
        let mut expr = self.or()?;
        while self.eat_punct("|") {
            let name = match self.tokens.get(self.at) {
                Some(Token::Name(name)) => name.clone(),
                Some(token) => {
                    return Err(ExprError::Syntax(format!(
                        "a filter's name should follow '|', not {}",
                        describe(token)
                    )))
                }
                None => return Err(ExprError::Syntax("a filter's name should follow '|'".into())),
            };
            self.at += 1;
            let mut args = vec![Arg { key: None, value: expr }];
            if self.eat_punct("(") {
                args.extend(self.args()?);
            }
            expr = call(name, args)?;
        }
        self.depth -= 1;
        Ok(expr)
    }

    fn args(&mut self) -> Result<Vec<Arg>, ExprError> {
        let mut args = Vec::new();
        if self.eat_punct(")") {
            return Ok(args);
        }
        loop {
            let first = self.pipe()?;
            let arg = if self.eat_punct(":") {
                Arg { key: Some(first), value: self.pipe()? }
            } else {
                Arg { key: None, value: first }
            };
            args.push(arg);
            if self.eat_punct(")") {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    fn or(&mut self) -> Result<Expr, ExprError> {
        let mut left = self.and()?;
        while self.eat_punct("||") || self.eat_word("or") {
            let right = self.and()?;
            left = Expr::Binary(Op::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, ExprError> {
        let mut left = self.cmp()?;
        while self.eat_punct("&&") || self.eat_word("and") {
            let right = self.cmp()?;
            left = Expr::Binary(Op::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if self.peek_word(word) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn cmp(&mut self) -> Result<Expr, ExprError> {
        let left = self.add()?;
        let op = [
            ("==", Op::Eq),
            ("!=", Op::Ne),
            ("<=", Op::Le),
            (">=", Op::Ge),
            ("<", Op::Lt),
            (">", Op::Gt),
        ]
        .into_iter()
        .find(|(punct, _)| self.peek_punct(punct));
        match op {
            Some((_, op)) => {
                self.at += 1;
                let right = self.add()?;
                Ok(Expr::Binary(op, Box::new(left), Box::new(right)))
            }
            None => Ok(left),
        }
    }

    fn add(&mut self) -> Result<Expr, ExprError> {
        let mut left = self.mul()?;
        loop {
            let op = if self.eat_punct("+") {
                Op::Add
            } else if self.eat_punct("-") {
                Op::Sub
            } else {
                return Ok(left);
            };
            let right = self.mul()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn mul(&mut self) -> Result<Expr, ExprError> {
        let mut left = self.unary()?;
        loop {
            let op = if self.eat_punct("*") {
                Op::Mul
            } else if self.eat_punct("/") {
                Op::Div
            } else if self.eat_punct("%") {
                Op::Rem
            } else {
                return Ok(left);
            };
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Expr, ExprError> {
        self.deeper()?;
        let expr = if self.eat_punct("-") {
            Expr::Neg(Box::new(self.unary()?))
        } else if self.eat_punct("!") || self.eat_word("not") {
            Expr::Not(Box::new(self.unary()?))
        } else {
            self.primary()?
        };
        self.depth -= 1;
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, ExprError> {
        let token = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or_else(|| ExprError::Syntax("a value is missing at the end".into()))?;
        self.at += 1;
        match token {
            Token::Number(n) => Ok(Expr::Number(n)),
            Token::Text(text) => Ok(Expr::Text(text)),
            Token::Name(name) if name == "true" => Ok(Expr::Bool(true)),
            Token::Name(name) if name == "false" => Ok(Expr::Bool(false)),
            Token::Name(name) => {
                if self.eat_punct("(") {
                    let args = self.args()?;
                    call(name, args)
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Token::Punct("(") => {
                let inner = self.pipe()?;
                self.expect(")")?;
                Ok(inner)
            }
            Token::Punct(p) => Err(ExprError::Syntax(format!("'{p}' was not expected here"))),
        }
    }
}

fn describe(token: &Token) -> String {
    match token {
        Token::Number(n) => format!("the number {n}"),
        Token::Text(text) => format!("the text \"{text}\""),
        Token::Name(name) => format!("'{name}'"),
        Token::Punct(p) => format!("'{p}'"),
    }
}

/// A call, refused here if the function does not exist or is given the wrong
/// number of arguments — so a typo is caught on save.
fn call(name: String, args: Vec<Arg>) -> Result<Expr, ExprError> {
    let Some(&(_, least, most, expected)) = FUNCTIONS.iter().find(|(known, ..)| *known == name) else {
        return Err(ExprError::UnknownFilter(name));
    };
    let keyed = args.iter().skip(1).any(|arg| arg.key.is_some());
    if args.len() < least || args.len() > most || (keyed && name != "map") || (name == "map" && args.iter().skip(1).any(|arg| arg.key.is_none())) {
        return Err(ExprError::Arguments { filter: name, expected });
    }
    Ok(Expr::Call(name, args))
}

impl Expr {
    pub(crate) fn variables(&self, names: &mut BTreeSet<String>) {
        match self {
            Expr::Var(name) => {
                names.insert(name.clone());
            }
            Expr::Neg(inner) | Expr::Not(inner) => inner.variables(names),
            Expr::Binary(_, left, right) => {
                left.variables(names);
                right.variables(names);
            }
            Expr::Call(_, args) => {
                for arg in args {
                    if let Some(key) = &arg.key {
                        key.variables(names);
                    }
                    arg.value.variables(names);
                }
            }
            Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) => {}
        }
    }

    pub(crate) fn eval(&self, lookup: &dyn Lookup, locale: &Locale) -> Val {
        match self {
            Expr::Number(n) => Val::Number(*n),
            Expr::Text(text) => Val::Text(text.clone()),
            Expr::Bool(b) => Val::Bool(*b),
            Expr::Var(name) => lookup.get(name),
            Expr::Neg(inner) => match inner.eval(lookup, locale).as_number() {
                Some(n) => Val::Number(-n),
                None => Val::Missing,
            },
            Expr::Not(inner) => Val::Bool(!inner.eval(lookup, locale).truthy()),
            Expr::Binary(op, left, right) => binary(*op, left, right, lookup, locale),
            Expr::Call(name, args) => {
                if name == "if" {
                    let branch = if args[0].value.eval(lookup, locale).truthy() { 1 } else { 2 };
                    return args[branch].value.eval(lookup, locale);
                }
                let input = args[0].value.eval(lookup, locale);
                if name == "map" {
                    return map(input, &args[1..], lookup, locale);
                }
                let rest: Vec<Val> = args[1..].iter().map(|arg| arg.value.eval(lookup, locale)).collect();
                format::apply(name, input, &rest, locale)
            }
        }
    }
}

fn binary(op: Op, left: &Expr, right: &Expr, lookup: &dyn Lookup, locale: &Locale) -> Val {
    match op {
        Op::And => {
            return Val::Bool(left.eval(lookup, locale).truthy() && right.eval(lookup, locale).truthy())
        }
        Op::Or => {
            return Val::Bool(left.eval(lookup, locale).truthy() || right.eval(lookup, locale).truthy())
        }
        _ => {}
    }
    let a = left.eval(lookup, locale);
    let b = right.eval(lookup, locale);
    if matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) {
        return Val::Bool(compare(op, &a, &b));
    }
    if a.is_missing() || b.is_missing() {
        return Val::Missing;
    }
    // `+` joins text when either side is text that is not a number.
    if op == Op::Add {
        let is_words = |v: &Val| matches!(v, Val::Text(_)) && v.as_number().is_none();
        if is_words(&a) || is_words(&b) {
            return Val::Text(format!("{}{}", format::display(&a, locale), format::display(&b, locale)));
        }
    }
    let (Some(x), Some(y)) = (a.as_number(), b.as_number()) else {
        return Val::Missing;
    };
    let result = match op {
        Op::Add => x + y,
        Op::Sub => x - y,
        Op::Mul => x * y,
        Op::Div if y == 0.0 => return Val::Missing,
        Op::Div => x / y,
        Op::Rem if y == 0.0 => return Val::Missing,
        Op::Rem => x % y,
        _ => unreachable!("comparisons and logic return above"),
    };
    if result.is_finite() {
        Val::Number(result)
    } else {
        Val::Missing
    }
}

/// A comparison with something unknown is false, whichever way round — so an
/// unread value never lights a key. Numbers compare as numbers (text that is
/// one included), anything else as exact text.
pub(crate) fn compare(op: Op, a: &Val, b: &Val) -> bool {
    if a.is_missing() || b.is_missing() {
        return false;
    }
    let ordering = match (a.as_number(), b.as_number()) {
        (Some(x), Some(y)) if !(matches!(a, Val::Bool(_)) ^ matches!(b, Val::Bool(_))) => x.partial_cmp(&y),
        _ => {
            let (x, y) = (format::plain(a), format::plain(b));
            Some(x.cmp(&y))
        }
    };
    let Some(ordering) = ordering else { return false };
    use std::cmp::Ordering::*;
    match op {
        Op::Eq => ordering == Equal,
        Op::Ne => ordering != Equal,
        Op::Lt => ordering == Less,
        Op::Le => ordering != Greater,
        Op::Gt => ordering == Greater,
        Op::Ge => ordering != Less,
        _ => false,
    }
}

/// `map(k: v, …, _: fallback)`: the value of the first key equal to the
/// input. No match and no `_`: the input itself.
fn map(input: Val, pairs: &[Arg], lookup: &dyn Lookup, locale: &Locale) -> Val {
    let mut fallback = None;
    for pair in pairs {
        let Some(key) = &pair.key else { continue };
        if matches!(key, Expr::Var(name) if name == "_") {
            fallback = Some(&pair.value);
            continue;
        }
        if compare(Op::Eq, &input, &key.eval(lookup, locale)) {
            return pair.value.eval(lookup, locale);
        }
    }
    match fallback {
        Some(value) => value.eval(lookup, locale),
        None => input,
    }
}

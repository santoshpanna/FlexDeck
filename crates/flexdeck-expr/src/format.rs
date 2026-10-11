//! How values turn into text, and the filters.

use chrono::{DateTime, FixedOffset, Local, TimeZone, Utc};

use crate::Val;

/// How numbers, money, times and temperatures are written. Set once per desktop.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Locale {
    /// `.` in "1.5", `,` in "1,5".
    pub decimal: char,
    /// `,` in "1,000"; `'\0'` for none.
    pub group: char,
    /// The currency `currency` uses when it is given none.
    pub currency: String,
    /// `time` without a format: 12-hour "9:41 PM" or 24-hour "21:41".
    pub hour12: bool,
    /// `temp` shows °F rather than °C.
    pub fahrenheit: bool,
}

impl Default for Locale {
    fn default() -> Self {
        Self {
            decimal: '.',
            group: ',',
            currency: "USD".into(),
            hour12: false,
            fahrenheit: false,
        }
    }
}

/// A value as a label shows it: unknown as `–`, bools as on/off, whole
/// numbers without decimals, others to one place.
pub(crate) fn display(value: &Val, locale: &Locale) -> String {
    match value {
        Val::Missing => "–".into(),
        Val::Bool(true) => "on".into(),
        Val::Bool(false) => "off".into(),
        Val::Text(text) => text.clone(),
        Val::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        Val::Number(n) => decimals(*n, 1, false, locale),
    }
}

/// A value for comparing as text: like `display`, but bools as true/false.
pub(crate) fn plain(value: &Val) -> String {
    match value {
        Val::Bool(b) => b.to_string(),
        other => display(other, &Locale::default()),
    }
}

/// `n` to `places` decimals, grouped in thousands when `grouped`.
fn decimals(n: f64, places: usize, grouped: bool, locale: &Locale) -> String {
    let places = places.min(10);
    // Half away from zero, as people round: 0.5 -> 1, not to the even 0.
    let scale = 10f64.powi(places as i32);
    let rounded = (n.abs() * scale).round() / scale;
    let text = format!("{:.*}", places, rounded);
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole.to_string(), Some(fraction.to_string())),
        None => (text, None),
    };
    let whole = if grouped && locale.group != '\0' {
        let digits: Vec<char> = whole.chars().collect();
        let mut out = String::new();
        for (index, digit) in digits.iter().enumerate() {
            if index > 0 && (digits.len() - index) % 3 == 0 {
                out.push(locale.group);
            }
            out.push(*digit);
        }
        out
    } else {
        whole
    };
    // "-0" is not a reading anyone wants to see.
    let negative = n < 0.0 && text_is_nonzero(&whole, fraction.as_deref());
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&whole);
    if let Some(fraction) = fraction {
        out.push(locale.decimal);
        out.push_str(&fraction);
    }
    out
}

fn text_is_nonzero(whole: &str, fraction: Option<&str>) -> bool {
    whole.chars().chain(fraction.unwrap_or("").chars()).any(|c| c.is_ascii_digit() && c != '0')
}

fn places(args: &[Val], index: usize, default: usize) -> usize {
    args.get(index)
        .and_then(Val::as_number)
        .map(|n| n.clamp(0.0, 10.0) as usize)
        .unwrap_or(default)
}

fn symbol(code: &str) -> Option<&'static str> {
    Some(match code.to_ascii_uppercase().as_str() {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "INR" => "₹",
        "CNY" => "¥",
        "KRW" => "₩",
        "BTC" => "₿",
        "AUD" => "A$",
        "CAD" => "C$",
        _ => return None,
    })
}

/// A filter applied to `input`, with its other arguments.
pub(crate) fn apply(name: &str, input: Val, args: &[Val], locale: &Locale) -> Val {
    // `default` is the one filter that looks at an unknown value.
    if name == "default" {
        return if input.is_missing() { args[0].clone() } else { input };
    }
    if input.is_missing() {
        return Val::Missing;
    }
    let number = input.as_number();
    let text = || display(&input, locale);
    let numeric = |f: &dyn Fn(f64) -> f64| match number {
        Some(n) => {
            let out = f(n);
            if out.is_finite() { Val::Number(out) } else { Val::Missing }
        }
        None => Val::Missing,
    };
    match name {
        "round" => match number {
            Some(n) => Val::Text(decimals(n, places(args, 0, 0), false, locale)),
            None => Val::Missing,
        },
        "number" => match number {
            Some(n) => Val::Text(decimals(n, places(args, 0, 0), true, locale)),
            None => Val::Missing,
        },
        "pct" => match number {
            Some(n) => Val::Text(format!("{}%", decimals(n, places(args, 0, 0), false, locale))),
            None => Val::Missing,
        },
        "currency" => match number {
            Some(n) => {
                let code = match args.first() {
                    Some(Val::Text(code)) if !code.trim().is_empty() => code.trim().to_ascii_uppercase(),
                    _ => locale.currency.clone(),
                };
                let amount = decimals(n.abs(), places(args, 1, 2), true, locale);
                let sign = if n < 0.0 && text_is_nonzero(&amount, None) { "-" } else { "" };
                Val::Text(match symbol(&code) {
                    Some(symbol) => format!("{sign}{symbol}{amount}"),
                    None => format!("{sign}{amount} {code}"),
                })
            }
            None => Val::Missing,
        },
        "abs" => numeric(&f64::abs),
        "floor" => numeric(&f64::floor),
        "ceil" => numeric(&f64::ceil),
        "sign" => match number {
            Some(n) if n > 0.0 => Val::Text(format!("+{}", text())),
            Some(n) if n < 0.0 => Val::Text(text().replacen('-', "−", 1)),
            Some(_) => Val::Text(text()),
            None => Val::Missing,
        },
        "clamp" => match (number, args[0].as_number(), args[1].as_number()) {
            (Some(n), Some(lo), Some(hi)) if lo <= hi => Val::Number(n.clamp(lo, hi)),
            _ => Val::Missing,
        },
        "min" => match (number, args[0].as_number()) {
            (Some(a), Some(b)) => Val::Number(a.min(b)),
            _ => Val::Missing,
        },
        "max" => match (number, args[0].as_number()) {
            (Some(a), Some(b)) => Val::Number(a.max(b)),
            _ => Val::Missing,
        },
        "upper" => Val::Text(text().to_uppercase()),
        "lower" => Val::Text(text().to_lowercase()),
        "trim" => Val::Text(text().trim().to_string()),
        "truncate" => {
            let limit = places(args, 0, 0).max(1);
            let full = text();
            if full.chars().count() <= limit {
                Val::Text(full)
            } else {
                let cut: String = full.chars().take(limit.saturating_sub(1)).collect();
                Val::Text(format!("{}…", cut.trim_end()))
            }
        }
        "time" => time(&input, args.first(), locale),
        // Celsius in, the desktop's unit out: "21°" or "70°".
        "temp" => match number {
            Some(c) => {
                let shown = if locale.fahrenheit { c * 9.0 / 5.0 + 32.0 } else { c };
                Val::Text(format!("{}°", decimals(shown, places(args, 0, 0), false, locale)))
            }
            None => Val::Missing,
        },
        "duration" => match number {
            Some(n) if n >= 0.0 => {
                let total = n.round() as u64;
                let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
                Val::Text(if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") })
            }
            _ => Val::Missing,
        },
        "bytes" => match number {
            Some(n) if n >= 0.0 => {
                const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
                let mut value = n;
                let mut unit = 0;
                while value >= 1024.0 && unit < UNITS.len() - 1 {
                    value /= 1024.0;
                    unit += 1;
                }
                let places = if unit == 0 || value >= 100.0 { 0 } else { 1 };
                Val::Text(format!("{} {}", decimals(value, places, false, locale), UNITS[unit]))
            }
            _ => Val::Missing,
        },
        _ => Val::Missing,
    }
}

/// A moment as text. Seconds since 1970 are shown in the desktop's zone; an
/// ISO 8601 time keeps its own offset.
fn time(input: &Val, format: Option<&Val>, locale: &Locale) -> Val {
    let at: Option<DateTime<FixedOffset>> = match input {
        Val::Text(text) => DateTime::parse_from_rfc3339(text.trim()).ok(),
        other => other.as_number().and_then(|seconds| {
            let utc = Utc.timestamp_millis_opt((seconds * 1000.0) as i64).single()?;
            Some(utc.with_timezone(&Local).fixed_offset())
        }),
    };
    let Some(at) = at else { return Val::Missing };
    let format = match format {
        Some(Val::Text(format)) if !format.is_empty() => format.clone(),
        _ if locale.hour12 => "%-I:%M %p".into(),
        _ => "%H:%M".into(),
    };
    // A bad format must not panic the agent: chrono reports it through `fmt`.
    let mut out = String::new();
    use std::fmt::Write as _;
    if write!(out, "{}", at.format(&format)).is_err() {
        return Val::Missing;
    }
    Val::Text(out)
}

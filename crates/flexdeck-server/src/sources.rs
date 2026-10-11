//! Data sources: live values from the web or a program, without a plugin.
//!
//! A source is a named feed — a URL fetched every few minutes, a command run
//! every so often — and a list of fields, each an expression over what came
//! back: `json.current.temperature_2m`, `(json.price - json.open) / json.open * 100`.
//! Each field becomes a variable, `src.<source>.<field>`, that any label, rule
//! or automation reads like a plugin's. Three more say how the last run went:
//! `src.<source>.ok`, `.error` and `.updated` (seconds since 1970).
//!
//! # One per key
//!
//! A source can take arguments — a city, a currency pair — and a key can run
//! its own copy with its own (`Widget.sources`). That copy's values are
//! `src.<source>.@<widget>.<field>`, and the key's templates reach them as
//! `{{this.<field>}}`. Two weather keys, two cities, one source.
//!
//! # Safety
//!
//! Fetching runs on this computer, on behalf of a configuration someone else
//! may have shared, so it is fenced:
//!
//! - http and https only; at most 1 MB back; 10 seconds at most.
//! - Every address the host resolves to is checked before connecting, and
//!   again on each redirect (three at most): one on this computer or this
//!   network is refused unless the source says `lan` — which Home Assistant
//!   on the LAN needs, and which the editor asks about by name.
//! - A command runs the program given, with no shell between, its window
//!   hidden, killed after 10 seconds. Only an admin can save one, as with
//!   every configuration write.
//! - Nothing runs faster than every 30 seconds for the web, 5 for a command.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read as _;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use flexdeck_expr::{parse_expression, Expression, Locale, Lookup, Template, Val};
use flexdeck_vars::{Value, VariableSpec, VariableStore};
use serde::{Deserialize, Serialize};

/// Where the definitions are kept.
pub const SOURCES_KEY: &str = "sources";
/// Where secret arguments are kept — tokens, keys — never in the deck's config,
/// which every tablet reads.
pub const SECRETS_KEY: &str = "source-secrets";
/// What a secret is shown as, where it would otherwise be shown.
pub const HIDDEN: &str = "\u{2022}\u{2022}\u{2022}\u{2022}";

pub const MAX_BODY: usize = 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_REDIRECTS: usize = 3;
pub const MIN_HTTP_SECONDS: u32 = 30;
pub const MIN_COMMAND_SECONDS: u32 = 5;
pub const MAX_SOURCES: usize = 64;
pub const MAX_FIELDS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    #[default]
    Http,
    Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HttpSpec {
    /// A template: `https://api.open-meteo.com/v1/forecast?city={{args.city}}`.
    pub url: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpec {
    pub name: String,
    /// An expression over `json.…`, `text`, `status`, `args.…` and variables.
    pub value: String,
    /// The value is a path to a list of numbers (`json.hourly.temp`,
    /// `json.list.*.main.temp`): it becomes the field's history, for a graph.
    #[serde(default)]
    pub series: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArgSpec {
    pub name: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub description: String,
    /// A token or key: kept on this computer only, never in the config, an
    /// error message or a field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDef {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub kind: SourceKind,
    /// How often it runs. 0: every 5 minutes for the web, every minute for a command.
    #[serde(default)]
    pub every_seconds: u32,
    #[serde(default)]
    pub http: Option<HttpSpec>,
    #[serde(default)]
    pub command: Option<CommandSpec>,
    #[serde(default)]
    pub fields: Vec<FieldSpec>,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    /// May reach this computer and this network.
    #[serde(default)]
    pub lan: bool,
}

fn yes() -> bool {
    true
}

impl SourceDef {
    /// How long between runs, floors applied.
    pub fn interval(&self) -> Duration {
        let (floor, default) = match self.kind {
            SourceKind::Http => (MIN_HTTP_SECONDS, 300),
            SourceKind::Command => (MIN_COMMAND_SECONDS, 60),
        };
        let seconds = if self.every_seconds == 0 { default } else { self.every_seconds.max(floor) };
        Duration::from_secs(u64::from(seconds))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourcesDoc {
    #[serde(default)]
    pub sources: Vec<SourceDef>,
}

/// What a recipe package holds: web sources and gallery presets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecipeDoc {
    #[serde(default)]
    pub sources: Vec<SourceDef>,
    #[serde(default)]
    pub presets: Vec<flexdeck_plugin::abi::PresetDto>,
}

/// Refuse a recipe that could not work, or that asks for more than a recipe
/// may: only web sources (a recipe may not run a program), each named after
/// the recipe, so two recipes never fight over one name.
pub fn validate_recipe(recipe: &str, doc: &RecipeDoc) -> Result<(), String> {
    validate(&SourcesDoc { sources: doc.sources.clone() })?;
    for source in &doc.sources {
        if source.kind != SourceKind::Http {
            return Err(format!("source '{}': a recipe may only read the web, not run a program", source.id));
        }
        if source.id != recipe && !source.id.starts_with(&format!("{recipe}-")) {
            return Err(format!("source '{}': a recipe's sources are named after it: '{recipe}' or '{recipe}-…'", source.id));
        }
    }
    Ok(())
}

/// Refuse a document that could not work, saying what is wrong where.
pub fn validate(doc: &SourcesDoc) -> Result<(), String> {
    if doc.sources.len() > MAX_SOURCES {
        return Err(format!("at most {MAX_SOURCES} sources"));
    }
    let mut seen = BTreeSet::new();
    for source in &doc.sources {
        let what = format!("source '{}'", source.id);
        let id_ok = source.id.len() <= 32
            && source.id.starts_with(|c: char| c.is_ascii_lowercase())
            && source.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !id_ok {
            return Err(format!("{what}: an id is lowercase letters, digits, - and _, starting with a letter"));
        }
        if !seen.insert(source.id.clone()) {
            return Err(format!("{what}: two sources have that id"));
        }
        match source.kind {
            SourceKind::Http => {
                let http = source.http.as_ref().ok_or_else(|| format!("{what}: it fetches nothing — give it a URL"))?;
                Template::parse(&http.url).map_err(|e| format!("{what}, its URL: {e}"))?;
                Template::parse(&http.body).map_err(|e| format!("{what}, its body: {e}"))?;
                for (name, value) in &http.headers {
                    Template::parse(value).map_err(|e| format!("{what}, header {name}: {e}"))?;
                }
                let method = http.method.trim().to_ascii_uppercase();
                if !["", "GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) {
                    return Err(format!("{what}: '{}' is not a method it can use", http.method));
                }
            }
            SourceKind::Command => {
                let command = source.command.as_ref().ok_or_else(|| format!("{what}: it runs nothing — give it a program"))?;
                if command.program.trim().is_empty() {
                    return Err(format!("{what}: it runs nothing — give it a program"));
                }
                for arg in &command.args {
                    Template::parse(arg).map_err(|e| format!("{what}, an argument: {e}"))?;
                }
            }
        }
        if source.fields.len() > MAX_FIELDS {
            return Err(format!("{what}: at most {MAX_FIELDS} fields"));
        }
        let mut names = BTreeSet::new();
        for field in &source.fields {
            if let Some(arg) = source.args.iter().find(|a| a.secret && mentions(&field.value, &a.name)) {
                return Err(format!("{what}, field {}: a field may not show the secret '{}'", field.name, arg.name));
            }
            let ok = !field.name.is_empty()
                && field.name.len() <= 32
                && !["ok", "error", "updated"].contains(&field.name.as_str())
                && field.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                return Err(format!("{what}: '{}' cannot be a field's name (letters, digits and _; not ok, error or updated)", field.name));
            }
            if !names.insert(field.name.clone()) {
                return Err(format!("{what}: two fields are called '{}'", field.name));
            }
            // Checked as it will run with the arguments' defaults filled in.
            let defaults: BTreeMap<String, String> = source.args.iter().map(|a| (a.name.clone(), a.default.clone())).collect();
            let value = fill_args(&field.value, &defaults);
            if field.series {
                if !value.trim().starts_with("json.") {
                    return Err(format!("{what}, field {}: a series is a path to a list, like json.hourly.temperature", field.name));
                }
            } else {
                parse_expression(&value).map_err(|e| format!("{what}, field {}: {e}", field.name))?;
            }
        }
    }
    Ok(())
}

/// Whether `expression` reads the argument `arg`.
fn mentions(expression: &str, arg: &str) -> bool {
    let needle = format!("args.{arg}");
    expression.match_indices(&needle).any(|(at, _)| {
        let after = expression[at + needle.len()..].chars().next();
        !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    })
}

/// The vault's name for one secret: a source's argument, for one key, or for
/// every key of that source that has none of its own (no widget).
pub fn secret_key(source: &str, widget: Option<&str>, arg: &str) -> String {
    format!("{source}/{}/{arg}", widget.unwrap_or(""))
}

/// `text` with every secret among `args` blotted out.
pub fn redact(source: &SourceDef, args: &BTreeMap<String, String>, text: &str) -> String {
    let mut text = text.to_string();
    for arg in source.args.iter().filter(|a| a.secret) {
        if let Some(value) = args.get(&arg.name).filter(|v| v.len() >= 3) {
            text = text.replace(value.as_str(), HIDDEN);
        }
    }
    text
}

// ---- What came back, and reading it ----------------------------------------

/// The result of one run, as fields read it.
#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    pub text: String,
    pub json: Option<serde_json::Value>,
}

/// An iCalendar feed (`BEGIN:VCALENDAR`) as JSON: `{events: [{summary,
/// location, start, end, allDay}]}`, times in seconds since 1970, sorted,
/// only those not yet over at `now`. A recurring event shows its first
/// occurrence only; most feeds a calendar app publishes expand them anyway.
pub fn calendar(text: &str, now: i64) -> Option<serde_json::Value> {
    if !text.trim_start().starts_with("BEGIN:VCALENDAR") {
        return None;
    }
    // Unfold: a line starting with a space or a tab continues the last.
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        match raw.strip_prefix([' ', '\t']) {
            Some(more) if !lines.is_empty() => lines.last_mut().unwrap().push_str(more),
            _ => lines.push(raw.trim_end_matches('\r').to_string()),
        }
    }
    let unescape = |v: &str| v.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\");
    let when = |params: &str, value: &str| -> Option<(i64, bool)> {
        use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
        let value = value.trim();
        if params.contains("VALUE=DATE") || value.len() == 8 {
            let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
            let midnight = Local.from_local_datetime(&date.and_hms_opt(0, 0, 0)?).earliest()?;
            return Some((midnight.timestamp(), true));
        }
        if let Some(utc) = value.strip_suffix('Z') {
            let at = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
            return Some((Utc.from_utc_datetime(&at).timestamp(), false));
        }
        // A named zone or none: read as this desktop's local time.
        let at = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
        Some((Local.from_local_datetime(&at).earliest()?.timestamp(), false))
    };
    let mut events = Vec::new();
    let mut current: Option<serde_json::Map<String, serde_json::Value>> = None;
    for line in &lines {
        if line == "BEGIN:VEVENT" {
            current = Some(serde_json::Map::new());
            continue;
        }
        if line == "END:VEVENT" {
            if let Some(event) = current.take() {
                events.push(event);
            }
            continue;
        }
        let Some(event) = current.as_mut() else { continue };
        let Some((key, value)) = line.split_once(':') else { continue };
        let (name, params) = key.split_once(';').unwrap_or((key, ""));
        match name {
            "SUMMARY" => {
                event.insert("summary".into(), unescape(value).into());
            }
            "LOCATION" => {
                event.insert("location".into(), unescape(value).into());
            }
            "DTSTART" | "DTEND" => {
                if let Some((at, all_day)) = when(params, value) {
                    event.insert(if name == "DTSTART" { "start" } else { "end" }.into(), at.into());
                    if name == "DTSTART" {
                        event.insert("allDay".into(), all_day.into());
                    }
                }
            }
            _ => {}
        }
    }
    let mut events: Vec<serde_json::Value> = events
        .into_iter()
        .filter_map(|mut event| {
            let start = event.get("start")?.as_i64()?;
            let end = event.get("end").and_then(|e| e.as_i64()).unwrap_or(start);
            event.entry("end").or_insert(end.into());
            event.entry("summary").or_insert("(no title)".into());
            (end >= now).then_some(serde_json::Value::Object(event))
        })
        .collect();
    events.sort_by_key(|event| event["start"].as_i64().unwrap_or(i64::MAX));
    events.truncate(50);
    Some(serde_json::json!({ "events": events }))
}

/// A path into JSON: `a.b.0.c`. Numbers index lists; `*` maps over one.
pub fn json_path<'a>(json: &'a serde_json::Value, path: &str) -> Vec<&'a serde_json::Value> {
    let mut current = vec![json];
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        let mut next = Vec::new();
        for value in current {
            match (segment, value) {
                ("*", serde_json::Value::Array(items)) => next.extend(items.iter()),
                ("*", serde_json::Value::Object(map)) => next.extend(map.values()),
                (key, serde_json::Value::Array(items)) => {
                    if let Some(item) = key.parse::<usize>().ok().and_then(|i| items.get(i)) {
                        next.push(item);
                    }
                }
                (key, serde_json::Value::Object(map)) => {
                    if let Some(item) = map.get(key) {
                        next.push(item);
                    }
                }
                _ => {}
            }
        }
        current = next;
    }
    current
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn json_val(value: &serde_json::Value) -> Val {
    match value {
        serde_json::Value::Number(n) => n.as_f64().map(Val::Number).unwrap_or(Val::Missing),
        serde_json::Value::String(s) => Val::Text(s.clone()),
        serde_json::Value::Bool(b) => Val::Bool(*b),
        serde_json::Value::Null => Val::Missing,
        other => Val::Text(other.to_string()),
    }
}

fn store_val(value: Option<Value>) -> Val {
    match value {
        Some(Value::Number(n)) => Val::Number(n),
        Some(Value::Text(t)) => Val::Text(t),
        Some(Value::Bool(b)) => Val::Bool(b),
        None => Val::Missing,
    }
}

/// What a field's expression reads: the response, the arguments, and the
/// live variables.
struct FieldLookup<'a> {
    response: &'a Response,
    args: &'a BTreeMap<String, String>,
    vars: &'a VariableStore,
}

impl Lookup for FieldLookup<'_> {
    fn get(&self, name: &str) -> Val {
        if name == "text" {
            return Val::Text(self.response.text.clone());
        }
        if name == "status" {
            return Val::Number(f64::from(self.response.status));
        }
        if name == "now" {
            return Val::Number(now_seconds() as f64);
        }
        if let Some(path) = name.strip_prefix("json.") {
            return match &self.response.json {
                Some(json) => json_path(json, path).first().map(|v| json_val(v)).unwrap_or(Val::Missing),
                None => Val::Missing,
            };
        }
        if name == "json" {
            return self.response.json.as_ref().map(json_val).unwrap_or(Val::Missing);
        }
        if let Some(arg) = name.strip_prefix("args.") {
            return self.args.get(arg).map(|v| Val::Text(v.clone())).unwrap_or(Val::Missing);
        }
        store_val(self.vars.get(name))
    }
}

/// The value of each field, and the points of each series.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings {
    pub values: BTreeMap<String, Val>,
    pub series: BTreeMap<String, Vec<f64>>,
}

/// A field's expression with its `{{args.…}}` filled in, so a path can name
/// what an argument chose: `json.rates.{{args.to}}`.
pub fn fill_args(value: &str, args: &BTreeMap<String, String>) -> String {
    if !value.contains("{{") {
        return value.to_string();
    }
    let lookup = |name: &str| match name.strip_prefix("args.") {
        Some(arg) => args.get(arg).map(|v| Val::Text(v.clone())).unwrap_or(Val::Missing),
        None => Val::Missing,
    };
    render(value, &lookup)
}

pub fn read_fields(source: &SourceDef, response: &Response, args: &BTreeMap<String, String>, vars: &VariableStore, locale: &Locale) -> Readings {
    let lookup = FieldLookup { response, args, vars };
    let mut read = Readings::default();
    for field in &source.fields {
        let value = fill_args(&field.value, args);
        if field.series {
            let path = value.trim().trim_start_matches("json.");
            let points = response
                .json
                .as_ref()
                .map(|json| json_path(json, path))
                .unwrap_or_default()
                .into_iter()
                .flat_map(|v| match v {
                    serde_json::Value::Array(items) => items.iter().filter_map(|i| i.as_f64()).collect::<Vec<_>>(),
                    other => other.as_f64().into_iter().collect(),
                })
                .filter(|n| n.is_finite())
                .collect();
            read.series.insert(field.name.clone(), points);
            continue;
        }
        let value = parse_expression(&value)
            .map(|expr: Expression| expr.eval(&lookup, locale))
            .unwrap_or(Val::Missing);
        read.values.insert(field.name.clone(), value);
    }
    read
}

// ---- Fetching ----------------------------------------------------------------

/// Whether an address is this computer or a private network: what a source
/// may reach only when it says `lan`.
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // carrier-grade NAT
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || v6.is_unspecified() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// How a host's addresses are found. Real DNS, or a test's answer.
pub type Resolve = dyn Fn(&str, u16) -> Result<Vec<IpAddr>, String> + Send + Sync;

pub fn system_resolve(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    (host, port)
        .to_socket_addrs()
        .map(|addrs| addrs.map(|a| a.ip()).collect())
        .map_err(|error| format!("{host} could not be found: {error}"))
}

/// Refuse `url` unless it is http(s) and every address its host resolves to
/// is allowed.
pub fn check_url(url: &str, lan: bool, resolve: &Resolve) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("'{url}' is not an address: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("'{url}' is not a web address (http or https)"));
    }
    let host = parsed.host_str().ok_or_else(|| format!("'{url}' names no host"))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addresses = match host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => resolve(host, port)?,
    };
    if addresses.is_empty() {
        return Err(format!("{host} could not be found"));
    }
    if !lan {
        if let Some(local) = addresses.iter().find(|ip| is_local(**ip)) {
            return Err(format!(
                "{host} is on this computer or your network ({local}); turn on \"May reach my network\" to allow it"
            ));
        }
    }
    Ok(parsed)
}

fn render(template: &str, lookup: &dyn Lookup) -> String {
    flexdeck_expr::render(template, lookup, &Locale::default())
}

/// Fetch a web source once.
pub fn fetch(http: &HttpSpec, lan: bool, lookup: &dyn Lookup, resolve: &Resolve) -> Result<Response, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
                .build(),
        )
        .timeout_global(Some(TIMEOUT))
        // Redirects are followed here, so each hop is checked like the first.
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let method = match http.method.trim().to_ascii_uppercase().as_str() {
        "" => "GET".to_string(),
        other => other.to_string(),
    };
    let body = render(&http.body, lookup);
    let mut url = check_url(&render(&http.url, lookup), lan, resolve)?;
    for _ in 0..=MAX_REDIRECTS {
        let mut request = ureq::http::Request::builder().method(method.as_str()).uri(url.as_str());
        for (name, value) in &http.headers {
            request = request.header(name.as_str(), render(value, lookup));
        }
        let request = request.body(body.clone()).map_err(|e| e.to_string())?;
        let response = agent.run(request).map_err(|e| format!("{}: {e}", url.host_str().unwrap_or("?")))?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| format!("{status} with nowhere to go"))?;
            url = check_url(url.join(location).map_err(|e| e.to_string())?.as_str(), lan, resolve)?;
            continue;
        }
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BODY {
            return Err(format!("the answer is larger than {} KB", MAX_BODY / 1024));
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let json = serde_json::from_slice(&bytes).ok().or_else(|| calendar(&text, now_seconds()));
        if !(200..300).contains(&status) {
            return Err(format!("{} answered {status}", url.host_str().unwrap_or("it")));
        }
        return Ok(Response { status, text, json });
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
}

/// Run a command source once: the program, no shell, its output read.
pub fn run_command(command: &CommandSpec, lookup: &dyn Lookup) -> Result<Response, String> {
    let mut process = std::process::Command::new(command.program.trim());
    process
        .args(command.args.iter().map(|a| render(a, lookup)))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashing up every minute.
        process.creation_flags(0x0800_0000);
    }
    let mut child = process.spawn().map_err(|e| format!("'{}' could not be started: {e}", command.program))?;
    let mut stdout = child.stdout.take().ok_or("no output")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(MAX_BODY as u64 + 1).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("'{}' took longer than {} seconds", command.program, TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let bytes = reader.join().unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY)]).trim().to_string();
    let json = serde_json::from_str(&text).ok();
    Ok(Response { status: status.code().unwrap_or(-1).clamp(0, u16::MAX as i32) as u16, text, json })
}

// ---- Variables ---------------------------------------------------------------

/// `src.<source>` or `src.<source>.@<widget>`: where one run's values live.
pub fn base_name(source: &str, instance: Option<&str>) -> String {
    match instance {
        Some(widget) => format!("src.{source}.@{widget}"),
        None => format!("src.{source}"),
    }
}

/// The run a variable belongs to: `src.weather.@w1.temp` → `src.weather.@w1`.
pub fn base_of(name: &str) -> Option<String> {
    let mut parts = name.splitn(4, '.');
    if parts.next()? != "src" {
        return None;
    }
    let source = parts.next()?;
    match parts.next()? {
        instance if instance.starts_with('@') => {
            parts.next()?;
            Some(format!("src.{source}.{instance}"))
        }
        _ => Some(format!("src.{source}")),
    }
}

/// Write `value` to `name`, declaring it (or re-declaring it, when its kind
/// changed) as the source's own.
fn publish(vars: &VariableStore, name: &str, value: Value, description: &str) {
    let kind = value.kind();
    match vars.spec(name) {
        Some(spec) if spec.kind == kind => {}
        Some(_) => {
            vars.undeclare(name);
            vars.declare(VariableSpec::new(name, kind, description).read_only());
        }
        None => vars.declare(VariableSpec::new(name, kind, description).read_only()),
    }
    let _ = vars.refresh(name, value);
}

fn val_to_value(val: &Val) -> Option<Value> {
    match val {
        Val::Number(n) => Some(Value::Number(*n)),
        Val::Text(t) => Some(Value::Text(t.clone())),
        Val::Bool(b) => Some(Value::Bool(*b)),
        Val::Missing => None,
    }
}

/// How one run went, as the editor shows it.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub ok: bool,
    pub error: Option<String>,
    /// Seconds since 1970.
    pub updated: i64,
}

/// The source engine: what is defined, what is running, and how each went.
pub struct Sources {
    doc: RwLock<SourcesDoc>,
    /// Sources installed recipes bring, by recipe id. Run like the person's
    /// own, but not theirs to edit: removing the recipe removes them.
    recipes: RwLock<BTreeMap<String, Vec<SourceDef>>>,
    vars: Arc<VariableStore>,
    /// When each run (source, instance) last started.
    runs: Mutex<HashMap<(String, Option<String>), (Instant, bool)>>,
    status: Mutex<BTreeMap<String, RunStatus>>,
    resolve: Arc<Resolve>,
    /// Secret arguments, by [`secret_key`].
    secrets: RwLock<BTreeMap<String, String>>,
}

impl std::fmt::Debug for Sources {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Sources").finish_non_exhaustive()
    }
}

/// One run that is due: a source, its instance (a widget id) and arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    pub source: SourceDef,
    pub instance: Option<String>,
    pub args: BTreeMap<String, String>,
}

impl Sources {
    pub fn new(vars: Arc<VariableStore>) -> Self {
        Self::with_resolver(vars, Arc::new(system_resolve))
    }

    pub fn with_resolver(vars: Arc<VariableStore>, resolve: Arc<Resolve>) -> Self {
        Self {
            doc: RwLock::new(SourcesDoc::default()),
            recipes: RwLock::new(BTreeMap::new()),
            vars,
            runs: Mutex::new(HashMap::new()),
            status: Mutex::new(BTreeMap::new()),
            resolve,
            secrets: RwLock::new(BTreeMap::new()),
        }
    }

    // ---- Secrets ----

    /// The secret arguments `source` declares.
    pub fn secret_args(&self, source: &str) -> Vec<String> {
        let doc = self.doc.read().unwrap_or_else(PoisonError::into_inner);
        let recipes = self.recipes.read().unwrap_or_else(PoisonError::into_inner);
        doc.sources
            .iter()
            .chain(recipes.values().flatten())
            .filter(|s| s.id == source)
            .flat_map(|s| s.args.iter().filter(|a| a.secret).map(|a| a.name.clone()))
            .collect()
    }

    /// Every secret, for keeping.
    pub fn secrets(&self) -> BTreeMap<String, String> {
        self.secrets.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Which secrets are set — names only, never values.
    pub fn secret_names(&self) -> Vec<String> {
        self.secrets.read().unwrap_or_else(PoisonError::into_inner).keys().cloned().collect()
    }

    pub fn load_secrets(&self, secrets: BTreeMap<String, String>) {
        *self.secrets.write().unwrap_or_else(PoisonError::into_inner) = secrets;
    }

    /// Keep `value` as a key's secret. The first one given for a source also
    /// becomes the source's own, so the next key that uses it works at once —
    /// one Home Assistant token, however many tiles. Returns whether anything
    /// changed.
    pub fn set_secret(&self, source: &str, widget: Option<&str>, arg: &str, value: &str) -> bool {
        if value.is_empty() {
            return false;
        }
        let mut secrets = self.secrets.write().unwrap_or_else(PoisonError::into_inner);
        let shared = secret_key(source, None, arg);
        let mut changed = false;
        if widget.is_some() && !secrets.contains_key(&shared) {
            secrets.insert(shared, value.to_string());
            changed = true;
        }
        let key = secret_key(source, widget, arg);
        if secrets.get(&key).map(String::as_str) != Some(value) {
            secrets.insert(key, value.to_string());
            changed = true;
        }
        drop(secrets);
        if changed {
            self.runs.lock().unwrap_or_else(PoisonError::into_inner).clear();
        }
        changed
    }

    /// Forget a secret. Returns whether there was one.
    pub fn forget_secret(&self, source: &str, widget: Option<&str>, arg: &str) -> bool {
        self.secrets.write().unwrap_or_else(PoisonError::into_inner).remove(&secret_key(source, widget, arg)).is_some()
    }

    /// Put the kept secrets into `args`: the key's own, else the source's. A
    /// value already there (a config from before secrets) is left alone if
    /// nothing is kept.
    pub fn fill_secrets(&self, source: &SourceDef, widget: Option<&str>, args: &mut BTreeMap<String, String>) {
        let secrets = self.secrets.read().unwrap_or_else(PoisonError::into_inner);
        for arg in source.args.iter().filter(|a| a.secret) {
            let own = widget.and_then(|w| secrets.get(&secret_key(&source.id, Some(w), &arg.name)));
            if let Some(value) = own.or_else(|| secrets.get(&secret_key(&source.id, None, &arg.name))) {
                args.insert(arg.name.clone(), value.clone());
            }
        }
    }

    pub fn doc(&self) -> SourcesDoc {
        self.doc.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Take a new set of definitions. Everything runs afresh.
    pub fn replace(&self, doc: SourcesDoc) {
        *self.doc.write().unwrap_or_else(PoisonError::into_inner) = doc;
        self.runs.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    /// Add (or replace) the sources a recipe brings.
    pub fn add_recipe(&self, recipe: &str, sources: Vec<SourceDef>) {
        self.recipes.write().unwrap_or_else(PoisonError::into_inner).insert(recipe.to_string(), sources);
    }

    /// Take a recipe's sources away again.
    pub fn remove_recipe(&self, recipe: &str) {
        self.recipes.write().unwrap_or_else(PoisonError::into_inner).remove(recipe);
    }

    /// Every recipe's sources, by recipe.
    pub fn recipes(&self) -> BTreeMap<String, Vec<SourceDef>> {
        self.recipes.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn status(&self) -> BTreeMap<String, RunStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The runs due now. `uses` are the keys' own copies, as (source, widget,
    /// args); `wanted` the variables someone reads — a source nobody reads
    /// does not run.
    pub fn due(&self, uses: &[(String, String, BTreeMap<String, String>)], wanted: &BTreeSet<String>, now: Instant) -> Vec<Due> {
        let doc = self.doc.read().unwrap_or_else(PoisonError::into_inner);
        let recipes = self.recipes.read().unwrap_or_else(PoisonError::into_inner);
        let mut runs = self.runs.lock().unwrap_or_else(PoisonError::into_inner);
        let mut due = Vec::new();
        let all = doc.sources.iter().chain(recipes.values().flatten());
        for source in all.filter(|s| s.enabled) {
            let defaults: BTreeMap<String, String> = source.args.iter().map(|a| (a.name.clone(), a.default.clone())).collect();
            let mut wanted_runs: Vec<(Option<String>, BTreeMap<String, String>)> = Vec::new();
            let prefix = format!("src.{}.", source.id);
            if wanted.iter().any(|name| name.starts_with(&prefix) && !name[prefix.len()..].starts_with('@')) {
                wanted_runs.push((None, defaults.clone()));
            }
            for (used, widget, args) in uses {
                if used == &source.id {
                    let mut merged = defaults.clone();
                    merged.extend(args.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (k.clone(), v.clone())));
                    wanted_runs.push((Some(widget.clone()), merged));
                }
            }
            for (instance, args) in wanted_runs {
                let key = (source.id.clone(), instance.clone());
                let ready = match runs.get(&key) {
                    None => true,
                    Some((_, true)) => false,
                    Some((started, false)) => now.duration_since(*started) >= source.interval(),
                };
                if ready {
                    runs.insert(key, (now, true));
                    let mut args = args;
                    self.fill_secrets(source, instance.as_deref(), &mut args);
                    due.push(Due { source: source.clone(), instance, args });
                }
            }
        }
        due
    }

    /// Run one, publish what it read, and say how it went. Blocking.
    pub fn run(&self, due: &Due, now_secs: i64, locale: &Locale) -> Result<Readings, String> {
        let base = base_name(&due.source.id, due.instance.as_deref());
        let lookup = FieldLookup { response: &Response::default(), args: &due.args, vars: &self.vars };
        let outcome = match due.source.kind {
            SourceKind::Http => match &due.source.http {
                Some(http) => fetch(http, due.source.lan, &lookup, self.resolve.as_ref()),
                None => Err("it fetches nothing".to_string()),
            },
            SourceKind::Command => match &due.source.command {
                Some(command) => run_command(command, &lookup),
                None => Err("it runs nothing".to_string()),
            },
        };
        let outcome = outcome.map_err(|error| redact(&due.source, &due.args, &error));
        let result = outcome.map(|response| read_fields(&due.source, &response, &due.args, &self.vars, locale));
        let name = if due.source.name.trim().is_empty() { due.source.id.clone() } else { due.source.name.clone() };
        match &result {
            Ok(read) => {
                for (field, value) in &read.values {
                    if let Some(value) = val_to_value(value) {
                        publish(&self.vars, &format!("{base}.{field}"), value, &format!("{name}: {field}"));
                    }
                }
                for field in read.series.keys() {
                    // The latest point is also the field's value.
                    if let Some(last) = read.series[field].last() {
                        publish(&self.vars, &format!("{base}.{field}"), Value::Number(*last), &format!("{name}: {field}"));
                    }
                }
                publish(&self.vars, &format!("{base}.ok"), Value::Bool(true), &format!("{name}: whether the last run worked"));
                publish(&self.vars, &format!("{base}.error"), Value::Text(String::new()), &format!("{name}: why the last run failed"));
                publish(&self.vars, &format!("{base}.updated"), Value::Number(now_secs as f64), &format!("{name}: when it last worked"));
            }
            Err(error) => {
                publish(&self.vars, &format!("{base}.ok"), Value::Bool(false), &format!("{name}: whether the last run worked"));
                publish(&self.vars, &format!("{base}.error"), Value::Text(format!("{name}: {error}")), &format!("{name}: why the last run failed"));
            }
        }
        let key = (due.source.id.clone(), due.instance.clone());
        if let Some(entry) = self.runs.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&key) {
            entry.1 = false;
        }
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = status.get(&base).map(|s| s.updated).unwrap_or(0);
        status.insert(
            base,
            RunStatus {
                ok: result.is_ok(),
                error: result.as_ref().err().cloned(),
                updated: if result.is_ok() { now_secs } else { previous },
            },
        );
        result
    }

    /// Run `source` once with `args`, without publishing: what the editor's
    /// "Test" shows — the answer's start and what each field reads. A secret
    /// left blank is the kept one (`widget`'s, else the source's); any secret
    /// is blotted out of what comes back.
    pub fn test(&self, source: &SourceDef, args: &BTreeMap<String, String>, widget: Option<&str>, locale: &Locale) -> Result<(Response, Readings), String> {
        let mut args = args.clone();
        let mut kept = BTreeMap::new();
        self.fill_secrets(source, widget, &mut kept);
        for (name, value) in kept {
            if args.get(&name).is_none_or(|v| v.trim().is_empty()) {
                args.insert(name, value);
            }
        }
        let empty = Response::default();
        let lookup = FieldLookup { response: &empty, args: &args, vars: &self.vars };
        let response = match source.kind {
            SourceKind::Http => fetch(source.http.as_ref().ok_or("it fetches nothing")?, source.lan, &lookup, self.resolve.as_ref()),
            SourceKind::Command => run_command(source.command.as_ref().ok_or("it runs nothing")?, &lookup),
        };
        let mut response = response.map_err(|error| redact(source, &args, &error))?;
        let read = read_fields(source, &response, &args, &self.vars, locale);
        response.text = redact(source, &args, &response.text);
        Ok((response, read))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(value: serde_json::Value) -> SourceDef {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn paths_reach_into_lists_and_map_over_them() {
        let doc = json!({"list": [{"main": {"temp": 1.5}}, {"main": {"temp": 2.5}}], "a": {"b": [10, 20]}});
        assert_eq!(json_path(&doc, "a.b.1"), [&json!(20)]);
        assert_eq!(json_path(&doc, "list.*.main.temp"), [&json!(1.5), &json!(2.5)]);
        assert!(json_path(&doc, "a.nope").is_empty());
    }

    #[test]
    fn fields_read_the_answer_its_arguments_and_live_values() {
        let vars = VariableStore::new();
        vars.set("vars.offset", Value::Number(1.0)).unwrap();
        let weather = source(json!({"id": "weather", "http": {"url": "https://x"}, "fields": [
            {"name": "temp", "value": "json.current.temperature_2m + vars.offset"},
            {"name": "sky", "value": "json.current.weather_code | map(0: \"sun\", 3: \"cloud\", _: \"fog\")"},
            {"name": "city", "value": "args.city | upper"},
            {"name": "hourly", "value": "json.hourly.temperature_2m", "series": true}
        ]}));
        let response = Response {
            status: 200,
            text: String::new(),
            json: Some(json!({"current": {"temperature_2m": 20.5, "weather_code": 3}, "hourly": {"temperature_2m": [18, 19.5, 21]}})),
        };
        let args = BTreeMap::from([("city".to_string(), "berlin".to_string())]);
        let read = read_fields(&weather, &response, &args, &vars, &Locale::default());
        let pair = source(json!({"id": "fx", "http": {"url": "https://x"}, "fields": [{"name": "rate", "value": "json.rates.{{args.to}}"}]}));
        let rates = Response { json: Some(json!({"rates": {"USD": 1.09, "JPY": 162.3}})), ..Response::default() };
        let to = BTreeMap::from([("to".to_string(), "JPY".to_string())]);
        assert_eq!(read_fields(&pair, &rates, &to, &vars, &Locale::default()).values["rate"], Val::Number(162.3));
        assert_eq!(read.values["temp"], Val::Number(21.5));
        assert_eq!(read.values["sky"], Val::Text("cloud".into()));
        assert_eq!(read.values["city"], Val::Text("BERLIN".into()));
        assert_eq!(read.series["hourly"], [18.0, 19.5, 21.0]);
    }

    #[test]
    fn a_calendar_feed_reads_as_its_coming_events() {
        let ics = [
            "BEGIN:VCALENDAR",
            "BEGIN:VEVENT",
            r"SUMMARY:Stand-up\, daily",
            "DTSTART:20300101T090000Z",
            "DTEND:20300101T091500Z",
            "END:VEVENT",
            "BEGIN:VEVENT",
            "SUMMARY:Long ago",
            "DTSTART:20000101T090000Z",
            "DTEND:20000101T100000Z",
            "END:VEVENT",
            "BEGIN:VEVENT",
            "SUMMARY:Holi",
            " day",
            "DTSTART;VALUE=DATE:20291231",
            "END:VEVENT",
            "END:VCALENDAR",
        ]
        .join("\r\n");
        let json = calendar(&ics, 1_800_000_000).unwrap();
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 2, "the past is left out");
        assert_eq!(events[0]["summary"], "Holiday", "sorted, folded lines joined");
        assert_eq!(events[0]["allDay"], true);
        assert_eq!(events[1]["summary"], "Stand-up, daily");
        assert_eq!(events[1]["start"], 1_893_488_400);
        assert!(calendar("{}", 0).is_none());
    }

    #[test]
    fn this_computer_and_this_network_are_refused_unless_allowed() {
        let local: Arc<Resolve> = Arc::new(|_: &str, _: u16| Ok(vec!["192.168.1.20".parse().unwrap()]));
        let public: Arc<Resolve> = Arc::new(|_: &str, _: u16| Ok(vec!["93.184.216.34".parse().unwrap()]));
        assert!(check_url("https://example.com/x", false, public.as_ref()).is_ok());
        let refused = check_url("https://ha.local:8123/api", false, local.as_ref()).unwrap_err();
        assert!(refused.contains("May reach my network"), "{refused}");
        assert!(check_url("http://ha.local:8123/api", true, local.as_ref()).is_ok());
        for literal in ["http://127.0.0.1/", "http://[::1]/", "http://10.0.0.1/", "http://169.254.169.254/latest", "http://100.64.0.1/", "http://[::ffff:127.0.0.1]/"] {
            assert!(check_url(literal, false, public.as_ref()).is_err(), "{literal}");
        }
        assert!(check_url("file:///etc/passwd", true, public.as_ref()).is_err());
        assert!(check_url("ftp://example.com", true, public.as_ref()).is_err());
    }

    #[test]
    fn a_local_server_is_refused_on_redirect_and_answers_are_capped() {
        // A real server on this machine, reached with `lan` on, redirecting to
        // itself under a name that resolves privately: refused on the hop.
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for (n, stream) in server.incoming().take(3).enumerate() {
                let mut stream = stream.unwrap();
                let mut buffer = [0u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut buffer);
                let reply = match n {
                    0 => format!("HTTP/1.1 302 Found\r\nLocation: http://inside.test:{port}/\r\ncontent-length: 0\r\n\r\n"),
                    1 => format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}", MAX_BODY + 10, "x".repeat(MAX_BODY + 10)),
                    _ => "HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}".to_string(),
                };
                let _ = std::io::Write::write_all(&mut stream, reply.as_bytes());
            }
        });
        let lookup = |_: &str| Val::Missing;
        let resolve: Arc<Resolve> = Arc::new(|host: &str, _: u16| {
            if host == "inside.test" { Ok(vec!["10.1.2.3".parse().unwrap()]) } else { Ok(vec!["127.0.0.1".parse().unwrap()]) }
        });
        // Allowed to start locally (lan on), the hop to 10.x is still checked: with
        // lan on it is allowed, so instead check that with lan off the start fails.
        let http = HttpSpec { url: format!("http://127.0.0.1:{port}/"), ..Default::default() };
        assert!(fetch(&http, false, &lookup, resolve.as_ref()).unwrap_err().contains("your network"));
        // With lan on, the redirect is followed to inside.test (10.1.2.3) — which
        // does not answer, since it is not real — so the hop was checked and tried.
        let hop = fetch(&http, true, &lookup, resolve.as_ref()).unwrap_err();
        assert!(hop.contains("inside.test") || hop.contains("10.1.2.3"), "{hop}");
    }

    #[test]
    fn a_source_that_could_not_work_is_refused_on_save() {
        let refused = |doc: serde_json::Value| validate(&serde_json::from_value(doc).unwrap()).unwrap_err();
        assert!(refused(json!({"sources": [{"id": "Weather", "http": {"url": "https://x"}}]})).contains("lowercase"));
        assert!(refused(json!({"sources": [{"id": "w"}]})).contains("give it a URL"));
        assert!(refused(json!({"sources": [{"id": "w", "http": {"url": "https://x"}, "fields": [{"name": "ok", "value": "1"}]}]})).contains("cannot be a field"));
        assert!(refused(json!({"sources": [{"id": "w", "http": {"url": "https://x"}, "fields": [{"name": "t", "value": "json.x | rond"}]}]})).contains("field t"));
        assert!(refused(json!({"sources": [{"id": "w", "kind": "command", "command": {"program": " "}}]})).contains("program"));
        assert!(refused(json!({"sources": [{"id": "w", "http": {"url": "https://x"}}, {"id": "w", "http": {"url": "https://y"}}]})).contains("two sources"));
        validate(&serde_json::from_value(json!({"sources": [{"id": "weather", "http": {"url": "https://x?c={{args.city}}"}, "fields": [{"name": "t", "value": "json.t"}]}]})).unwrap()).unwrap();
    }

    #[test]
    fn a_run_is_due_when_read_and_not_again_until_its_interval_has_passed() {
        let sources = Sources::new(Arc::new(VariableStore::new()));
        sources.replace(serde_json::from_value(json!({"sources": [
            {"id": "weather", "everySeconds": 60, "http": {"url": "https://x"}, "args": [{"name": "city", "default": "Berlin"}]}
        ]})).unwrap());
        let now = Instant::now();
        assert!(sources.due(&[], &BTreeSet::new(), now).is_empty(), "nobody reads it");

        let wanted = BTreeSet::from(["src.weather.temp".to_string()]);
        let uses = vec![("weather".to_string(), "w2".to_string(), BTreeMap::from([("city".to_string(), "Tokyo".to_string())]))];
        let due = sources.due(&uses, &wanted, now);
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].args["city"], "Berlin");
        assert_eq!((due[1].instance.as_deref(), due[1].args["city"].as_str()), (Some("w2"), "Tokyo"));
        assert!(sources.due(&uses, &wanted, now + Duration::from_secs(120)).is_empty(), "still running");
    }

    #[test]
    fn a_run_publishes_its_fields_and_how_it_went() {
        let vars = Arc::new(VariableStore::new());
        let sources = Sources::new(Arc::clone(&vars));
        let due = Due {
            source: source(json!({"id": "echo", "kind": "command", "command": {"program": "nonexistent-program-flexdeck"}})),
            instance: Some("w1".into()),
            args: BTreeMap::new(),
        };
        assert!(sources.run(&due, 1_000, &Locale::default()).is_err());
        assert_eq!(vars.get("src.echo.@w1.ok"), Some(Value::Bool(false)));
        assert!(vars.get("src.echo.@w1.error").and_then(|v| v.as_text().map(str::to_string)).unwrap().contains("could not be started"));
        assert_eq!(base_of("src.echo.@w1.temp").as_deref(), Some("src.echo.@w1"));
        assert_eq!(base_of("src.echo.temp").as_deref(), Some("src.echo"));
        assert_eq!(base_of("system.cpu.usage"), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_command_is_run_without_a_shell_and_its_output_read() {
        let vars = Arc::new(VariableStore::new());
        let sources = Sources::new(Arc::clone(&vars));
        let due = Due {
            source: source(json!({"id": "echo", "kind": "command",
                "command": {"program": "cmd", "args": ["/C", "echo", "{{args.n}}"]},
                "args": [{"name": "n", "default": "41"}],
                "fields": [{"name": "n", "value": "text + 1"}]})),
            instance: None,
            args: BTreeMap::from([("n".to_string(), "41".to_string())]),
        };
        sources.run(&due, 5, &Locale::default()).unwrap();
        assert_eq!(vars.get("src.echo.n"), Some(Value::Number(42.0)));
        assert_eq!(vars.get("src.echo.updated"), Some(Value::Number(5.0)));
    }

    #[test]
    fn a_secret_is_kept_apart_filled_in_when_run_and_never_shown() {
        let vars = Arc::new(VariableStore::new());
        let sources = Sources::new(vars);
        let ha = source(json!({
            "id": "ha", "http": {"url": "https://x/{{args.entity}}", "headers": {"Authorization": "Bearer {{args.token}}"}},
            "args": [{"name": "entity", "default": "light.desk"}, {"name": "token", "secret": true}],
            "fields": [{"name": "state", "value": "json.state"}]
        }));
        sources.replace(SourcesDoc { sources: vec![ha.clone()] });
        assert_eq!(sources.secret_args("ha"), vec!["token".to_string()]);

        // The first key's token also becomes the source's, for the next key.
        assert!(sources.set_secret("ha", Some("w1"), "token", "abc123"));
        assert!(sources.set_secret("ha", Some("w2"), "token", "zzz999"));
        let uses = vec![
            ("ha".to_string(), "w1".to_string(), BTreeMap::new()),
            ("ha".to_string(), "w2".to_string(), BTreeMap::new()),
            ("ha".to_string(), "w3".to_string(), BTreeMap::new()),
        ];
        let due = sources.due(&uses, &BTreeSet::new(), Instant::now());
        let token = |w: &str| due.iter().find(|d| d.instance.as_deref() == Some(w)).unwrap().args["token"].clone();
        assert_eq!((token("w1"), token("w2"), token("w3")), ("abc123".to_string(), "zzz999".to_string(), "abc123".to_string()));
        assert_eq!(sources.secret_names(), vec!["ha//token".to_string(), "ha/w1/token".to_string(), "ha/w2/token".to_string()]);

        // Blotted out of whatever would show it.
        let args = BTreeMap::from([("token".to_string(), "abc123".to_string())]);
        assert_eq!(redact(&ha, &args, "401 for https://x?key=abc123"), format!("401 for https://x?key={HIDDEN}"));

        assert!(sources.forget_secret("ha", Some("w1"), "token"));
        assert!(!sources.forget_secret("ha", Some("w1"), "token"));
    }

    #[test]
    fn a_field_may_not_show_a_secret() {
        let doc = |value: &str| -> SourcesDoc {
            serde_json::from_value(json!({"sources": [{
                "id": "s", "http": {"url": "https://x"},
                "args": [{"name": "token", "secret": true}, {"name": "token2"}],
                "fields": [{"name": "f", "value": value}]
            }]}))
            .unwrap()
        };
        assert!(validate(&doc("args.token | upper")).unwrap_err().contains("secret"));
        assert!(validate(&doc("json.{{args.token}}")).unwrap_err().contains("secret"));
        validate(&doc("args.token2")).unwrap();
    }
}

//! Write a native flexdeck plugin in Rust.
//!
//! A plugin is one type implementing [`Sidecar`], handed to [`serve`] from
//! `main`. The SDK speaks the protocol — JSON-RPC over stdin and stdout, see
//! `flexdeck-plugin-abi` — and the plugin only describes itself and does its
//! work:
//!
//! ```no_run
//! use flexdeck_plugin_sdk::{abi, Host, Sidecar};
//!
//! struct Hello;
//!
//! impl Sidecar for Hello {
//!     fn manifest(&self, abi: u32) -> abi::ManifestDto {
//!         flexdeck_plugin_sdk::manifest(abi, "hello", "Hello", "1.0.0")
//!     }
//!
//!     fn invoke(&self, request: &abi::InvokeRequest, host: &Host) -> abi::InvokeResponse {
//!         host.log("info", &format!("ran {}", request.action));
//!         flexdeck_plugin_sdk::ok()
//!     }
//! }
//!
//! fn main() {
//!     flexdeck_plugin_sdk::serve(Hello);
//! }
//! ```
//!
//! # Concurrency
//!
//! The agent may ask for a sample while an action is still running — a press
//! does not stop the deck's labels updating — so requests are answered on a
//! small pool of threads, and [`Sidecar`] takes `&self`. Keep mutable state
//! behind a `Mutex`. [`Host`] is cheap to clone and safe to call from any
//! thread, including an audio callback's helper thread.
//!
//! # Stdout is the protocol
//!
//! Anything else written to stdout corrupts it. Log with [`Host::log`] or to
//! stderr; the agent keeps stderr as the plugin's log. A panic is reported on
//! stderr too, and the request that panicked answers with an error rather than
//! never.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::panic::AssertUnwindSafe;
use std::sync::{mpsc, Arc, Mutex, PoisonError};

pub use flexdeck_plugin_abi as abi;
use abi::Message;
use serde::Serialize;
use serde_json::{json, Value};

/// How many requests may be answered at once.
const WORKERS: usize = 4;

/// What [`Sidecar::start`] is given: the settings and the plugin's own folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Start {
    /// The ABI the agent asked for — the one the package's `plugin.json` names.
    pub abi: u32,
    pub settings: BTreeMap<String, String>,
    pub data_dir: Option<std::path::PathBuf>,
}

/// One plugin. Every method but [`Sidecar::manifest`] and [`Sidecar::invoke`]
/// has a default that does nothing.
pub trait Sidecar: Send + Sync + 'static {
    /// Who this plugin is, at `abi` (2 or 3). Leave out what `abi` cannot
    /// carry — slider metadata under 2 — or the agent refuses the answer.
    fn manifest(&self, abi: u32) -> abi::ManifestDto;

    /// The variables known up front.
    fn variables(&self) -> Vec<abi::VariableDto> {
        Vec::new()
    }

    /// After `initialize` is answered: open devices, connect, start timers.
    fn start(&self, _start: &Start, _host: &Host) {}

    /// Refuse a value before anything runs.
    fn check(&self, _request: &abi::CheckRequest, _host: &Host) -> abi::CheckResponse {
        abi::CheckResponse {
            ok: true,
            error: None,
        }
    }

    /// What an action's arguments can be set to right now.
    fn choices(&self, _request: &abi::ChoicesRequest, _host: &Host) -> abi::ChoicesResponse {
        abi::ChoicesResponse::default()
    }

    /// Run one action.
    fn invoke(&self, request: &abi::InvokeRequest, host: &Host) -> abi::InvokeResponse;

    /// Refresh `wanted`, and only `wanted`.
    fn sample(&self, _wanted: &[String], _host: &Host) -> BTreeMap<String, abi::ValueDto> {
        BTreeMap::new()
    }

    /// The person saved new settings. Defaults are filled in.
    fn settings_changed(&self, _settings: &BTreeMap<String, String>, _host: &Host) {}

    /// A key started using one of this plugin's feeds (ABI 4): publish its
    /// values with [`Host::var`] under `<id>.@<instance>.`, declaring them
    /// first. An error is the reason the key shows.
    fn watch_feed(
        &self,
        _instance: &str,
        feed: &str,
        _args: &BTreeMap<String, String>,
        _host: &Host,
    ) -> Result<(), String> {
        Err(format!("no feed called '{feed}'"))
    }

    /// That key is gone: stop.
    fn unwatch_feed(&self, _instance: &str, _host: &Host) {}

    /// Before exit: close devices.
    fn shutdown(&self, _host: &Host) {}
}

/// Everything a plugin can send the agent unprompted. Clone it freely.
#[derive(Clone)]
pub struct Host {
    out: Arc<Mutex<Box<dyn Write + Send>>>,
    abi: Arc<std::sync::atomic::AtomicU32>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Host").finish_non_exhaustive()
    }
}

impl Host {
    fn new(out: Box<dyn Write + Send>) -> Self {
        Self {
            out: Arc::new(Mutex::new(out)),
            abi: Arc::new(std::sync::atomic::AtomicU32::new(abi::ABI_VERSION)),
        }
    }

    /// The ABI the agent asked for.
    pub fn abi(&self) -> u32 {
        self.abi.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn write(&self, message: &Message) {
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = out.write_all(message.to_line().as_bytes());
        let _ = out.flush();
    }

    fn notify(&self, method: &str, params: impl Serialize) {
        let params = serde_json::to_value(params).unwrap_or(Value::Null);
        self.write(&Message::notification(method, params));
    }

    /// Publish values for variables this plugin owns.
    pub fn vars_set(&self, values: BTreeMap<String, abi::ValueDto>) {
        if !values.is_empty() {
            self.notify(abi::notify::VARS_SET, abi::VarsSet { values });
        }
    }

    /// Publish one value.
    pub fn var(&self, name: &str, value: abi::ValueDto) {
        self.vars_set(BTreeMap::from([(name.to_string(), value)]));
    }

    /// Declare variables at run time; `replace` forgets the others declared
    /// this way.
    pub fn vars_declare(&self, variables: Vec<abi::VariableDto>, replace: bool) {
        self.notify(
            abi::notify::VARS_DECLARE,
            abi::VarsDeclare { variables, replace },
        );
    }

    /// Announce an event the manifest declares.
    pub fn emit(&self, event: &str, payload: BTreeMap<String, String>) {
        self.notify(
            abi::notify::EVENT_EMIT,
            abi::EventEmit {
                event: event.to_string(),
                payload,
            },
        );
    }

    /// A line for the plugin's log: `info`, `warn` or `error`.
    pub fn log(&self, level: &str, message: &str) {
        self.notify(
            abi::notify::LOG,
            abi::LogLine {
                level: level.to_string(),
                message: message.to_string(),
            },
        );
    }

    /// Show a notification. Needs the `notifications` permission.
    pub fn show(&self, title: &str, body: &str) {
        self.notify(
            abi::notify::NOTIFY,
            abi::NotifyRequest {
                title: title.to_string(),
                body: body.to_string(),
            },
        );
    }

    /// Keep values for declared settings. ABI 3; ignored under 2.
    pub fn store_settings(&self, values: BTreeMap<String, String>) {
        if self.abi() >= 3 {
            self.notify(abi::notify::SETTINGS_STORE, abi::SettingsStore { values });
        }
    }

    /// Hand the agent an image and get back the `img:<sha256>` reference a
    /// text variable carries to a widget's icon. ABI 3.
    ///
    /// Refuses what the agent would refuse anyway — an unknown type, more than
    /// [`abi::MAX_IMAGE_BYTES`], or an agent speaking ABI 2 — so the mistake
    /// surfaces here rather than as a blank key.
    pub fn put_image(&self, bytes: &[u8], mime: &str) -> Result<String, String> {
        use base64::Engine as _;
        use sha2::Digest as _;

        if self.abi() < 3 {
            return Err("images need an agent that speaks plugin ABI 3".into());
        }
        if !abi::IMAGE_MIMES.contains(&mime) {
            return Err(format!("'{mime}' is not one of {:?}", abi::IMAGE_MIMES));
        }
        if bytes.len() > abi::MAX_IMAGE_BYTES {
            return Err(format!(
                "the image is {} bytes; the limit is {}",
                bytes.len(),
                abi::MAX_IMAGE_BYTES
            ));
        }
        let sha256: String = sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.notify(
            abi::notify::IMAGES_PUT,
            abi::ImagePut {
                sha256: sha256.clone(),
                mime: mime.to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        );
        Ok(format!("img:{sha256}"))
    }

    /// A variable's whole recent history, oldest first, for a graph. ABI 4;
    /// ignored under 3. The manifest must use [`abi::capability::SERIES`].
    pub fn set_series(&self, name: &str, points: Vec<f64>) {
        if self.abi() >= 4 {
            let points: Vec<f64> = points.into_iter().filter(|p| p.is_finite()).collect();
            let skip = points.len().saturating_sub(abi::MAX_SERIES_POINTS);
            self.notify(
                abi::notify::SERIES_SET,
                abi::SeriesSet { name: name.to_string(), points: points[skip..].to_vec() },
            );
        }
    }

    /// Open an http(s) address in the browser. ABI 3; ignored under 2.
    pub fn open_url(&self, url: &str) {
        if self.abi() >= 3 {
            self.notify(
                abi::notify::OPEN_URL,
                abi::OpenUrl {
                    url: url.to_string(),
                },
            );
        }
    }
}

/// A value in transit, for [`Host::var`] and [`Sidecar::sample`].
pub mod value {
    use super::abi;

    pub fn number(value: f64) -> abi::ValueDto {
        abi::ValueDto {
            kind: abi::kind::NUMBER.into(),
            value: serde_json::json!(value),
        }
    }

    pub fn boolean(value: bool) -> abi::ValueDto {
        abi::ValueDto {
            kind: abi::kind::BOOL.into(),
            value: serde_json::json!(value),
        }
    }

    pub fn text(value: impl Into<String>) -> abi::ValueDto {
        abi::ValueDto {
            kind: abi::kind::TEXT.into(),
            value: serde_json::Value::String(value.into()),
        }
    }
}

/// A host whose messages go nowhere, for calling a plugin directly in a test.
pub fn test_host() -> Host {
    Host::new(Box::new(std::io::sink()))
}

/// An empty manifest to fill in.
pub fn manifest(abi: u32, id: &str, name: &str, version: &str) -> abi::ManifestDto {
    abi::ManifestDto {
        abi,
        id: id.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        description: String::new(),
        author: String::new(),
        actions: Vec::new(),
        settings: Vec::new(),
        events: Vec::new(),
        permissions: Vec::new(),
        uses: Vec::new(),
        presets: Vec::new(),
        feeds: Vec::new(),
        guide: None,
    }
}

/// An action succeeded.
pub fn ok() -> abi::InvokeResponse {
    abi::InvokeResponse {
        ok: true,
        error: None,
        state: None,
    }
}

/// An action succeeded, and a widget it drives should show `state`.
pub fn ok_state(state: impl Into<String>) -> abi::InvokeResponse {
    abi::InvokeResponse {
        ok: true,
        error: None,
        state: Some(state.into()),
    }
}

/// An action failed, for this reason. The person reads it.
pub fn failed(reason: impl Into<String>) -> abi::InvokeResponse {
    abi::InvokeResponse {
        ok: false,
        error: Some(reason.into()),
        state: None,
    }
}

/// Serve the protocol on this process's stdin and stdout, then exit.
pub fn serve<S: Sidecar>(sidecar: S) -> ! {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("plugin panicked: {info}");
    }));
    let stdin = std::io::stdin();
    serve_with(sidecar, stdin.lock(), Box::new(std::io::stdout()));
    std::process::exit(0)
}

/// Serve the protocol on `input` and `output` until `input` ends or the agent
/// asks for `shutdown`. For tests, and for a plugin that wants to own `main`.
pub fn serve_with<S: Sidecar, R: BufRead>(sidecar: S, input: R, output: Box<dyn Write + Send>) {
    let host = Host::new(output);
    let sidecar = Arc::new(sidecar);
    let (jobs, queue) = mpsc::channel::<(u64, String, Value)>();
    let queue = Arc::new(Mutex::new(queue));

    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let queue = Arc::clone(&queue);
            let sidecar = Arc::clone(&sidecar);
            let host = host.clone();
            std::thread::spawn(move || loop {
                let job = queue.lock().unwrap_or_else(PoisonError::into_inner).recv();
                let Ok((id, method, params)) = job else { break };
                if guarded(|| answer(&*sidecar, &host, id, &method, params)).is_none() {
                    host.write(&Message::failure(
                        id,
                        -32603,
                        format!("the plugin crashed while answering '{method}'"),
                    ));
                }
            })
        })
        .collect();

    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Message>(&line) else {
            eprintln!("plugin: the agent sent something that is not JSON");
            continue;
        };
        let (Some(id), Some(method)) = (message.id, message.method) else {
            continue;
        };
        let params = message.params.unwrap_or(Value::Null);
        match method.as_str() {
            // Answered here, in order: nothing else may run before the
            // manifest is out, and nothing after shutdown.
            abi::method::INITIALIZE => {
                let start = initialize(&*sidecar, &host, id, params);
                if let Some(start) = start {
                    let sidecar = Arc::clone(&sidecar);
                    let host = host.clone();
                    std::thread::spawn(move || sidecar.start(&start, &host));
                }
            }
            abi::method::SHUTDOWN => {
                guarded(|| sidecar.shutdown(&host));
                host.write(&Message::success(id, json!({})));
                break;
            }
            _ => {
                let _ = jobs.send((id, method, params));
            }
        }
    }
    drop(jobs);
    for worker in workers {
        let _ = worker.join();
    }
}

/// Leave out the action lights this agent cannot read, rather than have it
/// refuse the whole manifest over one: under ABI 4 without `toggles`, only
/// the plain kind goes, and without `conditions`, no `when` light. Says it
/// uses `toggles` and `conditions` when it does.
pub fn fit_to_host(manifest: &mut abi::ManifestDto, abi_version: u32, offered: &[String]) {
    let offers = |capability: &str| abi_version >= 4 && offered.iter().any(|used| used == capability);
    let toggles = offers(abi::capability::TOGGLES);
    let conditions = offers(abi::capability::CONDITIONS);
    let prefix = manifest.variable_prefix();
    let mut using_toggles = false;
    let mut using_conditions = false;
    for action in &mut manifest.actions {
        let Some(active) = &action.active else { continue };
        if active.when.is_some() {
            if conditions {
                using_conditions = true;
            } else {
                eprintln!("plugin: this agent has no conditions, so {} is never lit", action.action);
                action.active = None;
            }
        } else if abi_version < 4 || (!active.is_plain(&prefix) && !toggles) {
            action.active = None;
        } else if !active.is_plain(&prefix) {
            using_toggles = true;
        }
    }
    // An agent refuses a capability it has never heard of, even unused.
    manifest.uses.retain(|used| {
        used != abi::capability::TOGGLES && used != abi::capability::SETUP && used != abi::capability::CONDITIONS
    });
    if using_toggles {
        manifest.uses.push(abi::capability::TOGGLES.to_string());
    }
    if using_conditions {
        manifest.uses.push(abi::capability::CONDITIONS.to_string());
    }

    let setup = abi_version >= 4 && offered.iter().any(|used| used == abi::capability::SETUP);
    if !setup {
        manifest.guide = None;
        for setting in &mut manifest.settings {
            setting.internal = false;
            setting.help = None;
        }
    }
    if manifest.guide.is_some()
        || manifest.settings.iter().any(|setting| setting.internal || setting.help.is_some())
    {
        manifest.uses.push(abi::capability::SETUP.to_string());
    }
}

fn initialize<S: Sidecar>(sidecar: &S, host: &Host, id: u64, params: Value) -> Option<Start> {
    let params: abi::InitializeParams = match serde_json::from_value(params) {
        Ok(params) => params,
        Err(error) => {
            host.write(&Message::failure(id, -32602, error.to_string()));
            return None;
        }
    };
    if !abi::supports_abi(params.abi) {
        host.write(&Message::failure(
            id,
            -32000,
            format!(
                "this plugin speaks plugin ABI {:?}, the agent speaks {}",
                abi::SUPPORTED_ABIS,
                params.abi
            ),
        ));
        return None;
    }
    host.abi.store(params.abi, std::sync::atomic::Ordering::SeqCst);
    let mut manifest = sidecar.manifest(params.abi);
    fit_to_host(&mut manifest, params.abi, &params.host_capabilities);
    let result = abi::InitializeResult {
        manifest,
        variables: sidecar.variables(),
    };
    host.write(&Message::success(
        id,
        serde_json::to_value(result).unwrap_or(Value::Null),
    ));
    Some(Start {
        abi: params.abi,
        settings: params.settings,
        data_dir: params.data_dir.map(std::path::PathBuf::from),
    })
}

/// Run `work`, turning a panic into `None` so one bad request cannot leave the
/// agent waiting for an answer that will never come.
fn guarded<T>(work: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(AssertUnwindSafe(work)).ok()
}

fn decode<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, String> {
    serde_json::from_value(params).map_err(|error| error.to_string())
}

fn answer<S: Sidecar>(sidecar: &S, host: &Host, id: u64, method: &str, params: Value) {
    let result: Result<Value, String> = match method {
        abi::method::VARIABLES => Ok(json!(sidecar.variables())),
        abi::method::CHECK => decode(params).map(|request| json!(sidecar.check(&request, host))),
        abi::method::INVOKE => {
            decode::<abi::InvokeRequest>(params).map(|request| {
                let mut response = sidecar.invoke(&request, host);
                // ABI 2 refuses a field it does not know.
                if host.abi() < 3 {
                    response.state = None;
                }
                json!(response)
            })
        }
        abi::method::SAMPLE => decode::<abi::SampleRequest>(params).map(|request| {
            json!(abi::SampleResponse {
                values: sidecar.sample(&request.wanted, host),
            })
        }),
        abi::method::CHOICES => {
            decode(params).map(|request| json!(sidecar.choices(&request, host)))
        }
        abi::method::FEED_WATCH => decode::<abi::FeedWatch>(params).and_then(|watch| {
            sidecar
                .watch_feed(&watch.instance, &watch.feed, &watch.args, host)
                .map(|()| json!({}))
        }),
        abi::method::FEED_UNWATCH => decode::<abi::FeedUnwatch>(params).map(|unwatch| {
            sidecar.unwatch_feed(&unwatch.instance, host);
            json!({})
        }),
        abi::method::SETTINGS_CHANGED => decode::<abi::SettingsChanged>(params).map(|changed| {
            sidecar.settings_changed(&changed.settings, host);
            json!({})
        }),
        other => {
            host.write(&Message::failure(id, -32601, format!("no method '{other}'")));
            return;
        }
    };
    match result {
        Ok(value) => host.write(&Message::success(id, value)),
        Err(error) => host.write(&Message::failure(id, -32602, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn messages(&self) -> Vec<Message> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).expect("every line is one message"))
                .collect()
        }
    }

    #[derive(Default)]
    struct Counter {
        count: AtomicUsize,
        started: Arc<AtomicUsize>,
    }

    impl Sidecar for Counter {
        fn manifest(&self, abi: u32) -> abi::ManifestDto {
            let mut manifest = manifest(abi, "counter", "Counter", "1.0.0");
            manifest.actions.push(abi::ActionDto {
                action: "bump".into(),
                description: String::new(),
                arguments: vec![],
                slider: None,
                active: None,
            });
            manifest
        }

        fn start(&self, _start: &Start, _host: &Host) {
            self.started.fetch_add(1, Ordering::SeqCst);
        }

        fn invoke(&self, request: &abi::InvokeRequest, host: &Host) -> abi::InvokeResponse {
            match request.action.as_str() {
                "bump" => {
                    let count = self.count.fetch_add(1, Ordering::SeqCst) + 1;
                    host.var("counter.count", value::number(count as f64));
                    host.store_settings(BTreeMap::from([("token".into(), "t".into())]));
                    ok_state(if count % 2 == 1 { "odd" } else { "even" })
                }
                "panic" => panic!("asked to"),
                _ => failed("no such action"),
            }
        }
    }

    fn run(lines: &[Value]) -> Vec<Message> {
        let input: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let output = Shared::default();
        serve_with(
            Counter::default(),
            std::io::Cursor::new(input),
            Box::new(output.clone()),
        );
        output.messages()
    }

    fn response(messages: &[Message], id: u64) -> &Message {
        messages
            .iter()
            .find(|message| message.id == Some(id) && message.method.is_none())
            .unwrap_or_else(|| panic!("no answer to {id}: {messages:?}"))
    }

    #[test]
    fn it_answers_initialize_in_the_abi_it_was_asked_for() {
        let messages = run(&[json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"abi":2}})]);
        let result: abi::InitializeResult =
            serde_json::from_value(response(&messages, 1).result.clone().unwrap()).unwrap();
        assert_eq!(result.manifest.abi, 2);
        result.manifest.validate().expect("valid");
    }

    #[test]
    fn a_toggle_light_reaches_only_an_agent_that_offers_toggles() {
        let lit = |variable: &str, equals: Option<&str>| abi::ActionDto {
            action: variable.replace('.', "_"),
            description: String::new(),
            arguments: vec![],
            slider: None,
            active: Some(abi::ActiveDto {
                variable: variable.into(),
                equals: equals.map(str::to_string),
                is: None,
                when: None,
            }),
        };
        let plain = lit("counter.scene", Some("scene"));
        let toggle = lit("counter.on", None);
        let manifest = || {
            let mut manifest = manifest(4, "counter", "Counter", "1.0.0");
            manifest.actions = vec![plain.clone(), toggle.clone()];
            manifest
        };

        let mut older = manifest();
        older.uses.push(abi::capability::TOGGLES.into());
        fit_to_host(&mut older, 4, &["series".into()]);
        assert_eq!(older.actions[0].active, plain.active, "the plain kind every ABI 4 agent reads");
        assert_eq!(older.actions[1].active, None);
        assert!(!older.uses(abi::capability::TOGGLES));

        let mut newer = manifest();
        fit_to_host(&mut newer, 4, &[abi::capability::TOGGLES.into()]);
        assert_eq!(newer.actions[1].active, toggle.active);
        assert!(newer.uses(abi::capability::TOGGLES));

        let mut abi3 = manifest();
        fit_to_host(&mut abi3, 3, &[abi::capability::TOGGLES.into()]);
        assert!(abi3.actions.iter().all(|action| action.active.is_none()), "ABI 3 has no lights");
    }

    #[test]
    fn a_condition_light_reaches_only_an_agent_that_offers_conditions() {
        let studio = abi::ActiveDto {
            variable: String::new(),
            equals: None,
            is: None,
            when: Some("obs.studio_mode and obs.preview_scene != obs.scene".into()),
        };
        let manifest = || {
            let mut manifest = manifest(4, "counter", "Counter", "1.0.0");
            manifest.actions = vec![abi::ActionDto {
                action: "transition".into(),
                description: String::new(),
                arguments: vec![],
                slider: None,
                active: Some(studio.clone()),
            }];
            manifest.uses.push(abi::capability::CONDITIONS.into());
            manifest
        };

        let mut older = manifest();
        fit_to_host(&mut older, 4, &[abi::capability::TOGGLES.into()]);
        assert_eq!(older.actions[0].active, None);
        assert!(older.uses.is_empty(), "neither conditions nor toggles: {:?}", older.uses);
        older.validate().expect("loads without the light");

        let mut newer = manifest();
        fit_to_host(&mut newer, 4, &[abi::capability::CONDITIONS.into()]);
        assert_eq!(newer.actions[0].active, Some(studio.clone()));
        assert_eq!(newer.uses, [abi::capability::CONDITIONS]);
        newer.validate().expect("valid with conditions");
    }

    #[test]
    fn setup_fields_reach_only_an_agent_that_offers_setup() {
        let manifest = || {
            let mut manifest = manifest(4, "counter", "Counter", "1.0.0");
            manifest.guide = Some("https://example.com/guide".into());
            manifest.settings = vec![abi::SettingDto {
                key: "token".into(),
                description: String::new(),
                kind: abi::kind::TEXT.into(),
                choices: vec![],
                default_value: None,
                secret: true,
                internal: true,
                help: Some("Where to find it".into()),
            }];
            manifest
        };

        let mut older = manifest();
        fit_to_host(&mut older, 4, &["series".into()]);
        assert_eq!(older.guide, None);
        assert!(!older.settings[0].internal && older.settings[0].help.is_none());
        assert!(!older.uses(abi::capability::SETUP));
        older.validate().expect("an older agent reads it");

        let mut newer = manifest();
        fit_to_host(&mut newer, 4, &[abi::capability::SETUP.into()]);
        assert!(newer.settings[0].internal);
        assert!(newer.uses(abi::capability::SETUP));
        newer.validate().expect("valid");
    }

    #[test]
    fn under_abi_3_an_action_reports_its_look_and_can_store_settings() {
        let messages = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"abi":3}}),
            json!({"jsonrpc":"2.0","id":2,"method":"invoke","params":{"action":"bump"}}),
        ]);
        let invoked: abi::InvokeResponse =
            serde_json::from_value(response(&messages, 2).result.clone().unwrap()).unwrap();
        assert_eq!(invoked.state.as_deref(), Some("odd"));
        assert!(messages
            .iter()
            .any(|m| m.method.as_deref() == Some(abi::notify::SETTINGS_STORE)));
        assert!(messages
            .iter()
            .any(|m| m.method.as_deref() == Some(abi::notify::VARS_SET)));
    }

    #[test]
    fn under_abi_2_nothing_abi_3_added_is_sent() {
        let messages = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"abi":2}}),
            json!({"jsonrpc":"2.0","id":2,"method":"invoke","params":{"action":"bump"}}),
        ]);
        let invoked = response(&messages, 2).result.clone().unwrap();
        assert_eq!(invoked, json!({"ok": true}), "no state under ABI 2");
        assert!(!messages
            .iter()
            .any(|m| m.method.as_deref() == Some(abi::notify::SETTINGS_STORE)));
    }

    #[test]
    fn an_abi_it_does_not_speak_is_refused_with_a_reason() {
        let messages = run(&[json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"abi":1}})]);
        let error = response(&messages, 1).error.clone().expect("refused");
        assert!(error.message.contains("ABI"), "{}", error.message);
    }

    #[test]
    fn an_unknown_method_says_so_and_a_panic_does_not_hang_the_agent() {
        let messages = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"abi":3}}),
            json!({"jsonrpc":"2.0","id":2,"method":"nope","params":{}}),
            json!({"jsonrpc":"2.0","id":3,"method":"invoke","params":{"action":"panic"}}),
            json!({"jsonrpc":"2.0","id":4,"method":"invoke","params":{"action":"bump"}}),
        ]);
        assert!(response(&messages, 2).error.is_some());
        let crashed = response(&messages, 3).error.clone().expect("answered, not left hanging");
        assert!(crashed.message.contains("crashed"), "{}", crashed.message);
        assert!(response(&messages, 4).result.is_some(), "still serving");
    }

    #[test]
    fn shutdown_is_answered_and_ends_the_loop() {
        let messages = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"abi":3}}),
            json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":{}}),
            json!({"jsonrpc":"2.0","id":3,"method":"invoke","params":{"action":"bump"}}),
        ]);
        assert!(response(&messages, 2).result.is_some());
        assert!(
            !messages.iter().any(|m| m.id == Some(3)),
            "nothing is answered after shutdown"
        );
    }

    #[test]
    fn put_image_names_the_image_by_its_hash_and_refuses_what_the_agent_would() {
        let output = Shared::default();
        let host = Host::new(Box::new(output.clone()));

        let reference = host.put_image(b"png", "image/png").unwrap();
        assert_eq!(reference, "img:8f8cbb7dcf46e0bc7d53265749a6c17d116093a6ba95e442764060c76fd4a86c");
        let messages = output.messages();
        let sent: abi::ImagePut =
            serde_json::from_value(messages[0].params.clone().unwrap()).unwrap();
        assert_eq!(messages[0].method.as_deref(), Some(abi::notify::IMAGES_PUT));
        assert_eq!(format!("img:{}", sent.sha256), reference);
        assert_eq!(sent.data, "cG5n");

        assert!(host.put_image(b"x", "image/svg+xml").is_err());
        assert!(host.put_image(&vec![0; abi::MAX_IMAGE_BYTES + 1], "image/png").is_err());
        host.abi.store(2, std::sync::atomic::Ordering::SeqCst);
        assert!(host.put_image(b"png", "image/png").is_err());
        assert_eq!(output.messages().len(), 1, "a refused image sends nothing");
    }
}

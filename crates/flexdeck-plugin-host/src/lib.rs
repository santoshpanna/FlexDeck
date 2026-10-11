//! Runs an installed plugin as a separate process.
//!
//! # The shape
//!
//! A plugin is a child process speaking JSON-RPC 2.0 over its stdin and stdout,
//! one message per line (see `flexdeck-plugin-abi`). [`SidecarPlugin`] starts
//! it, asks who it is, checks the answer against the package it came from, and
//! then implements [`flexdeck_plugin::Plugin`] by forwarding — so to the
//! registry, the editor and the automation engine an installed plugin is
//! indistinguishable from a built-in one.
//!
//! # What the host enforces, because nothing else will
//!
//! A process is not a sandbox: a plugin can open any socket the user can. What
//! the host *can* police is everything that passes through it, and it does:
//!
//! * a plugin may only publish variables under its own `<id>.` namespace;
//! * it may only emit events its manifest declared;
//! * it may only show notifications if it declared that permission;
//! * it may not claim a permission its package did not declare at install;
//! * it starts with none of the agent's environment but what any program
//!   needs to run ([`PASSED_ENV`]), so nothing the agent was started with —
//!   a token, a key — reaches it;
//! * every call has a deadline, and a plugin that misses one is restarted
//!   rather than allowed to hold up a button press forever.
//!
//! # Failure is always a value, never a panic
//!
//! A plugin that crashes is restarted with a growing delay, and one that keeps
//! crashing is stopped with a status that says so. Its actions then fail with
//! that status as the reason — "OBS plugin: stopped after crashing 5 times" —
//! rather than doing nothing.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The environment a plugin is started with, of the agent's own: what any
/// program needs to find its runtime, its temp folder, its home and the
/// network. Everything else the agent was started with stays with it.
pub const PASSED_ENV: &[&str] = &[
    "PATH", "PATHEXT", "SYSTEMROOT", "SYSTEMDRIVE", "WINDIR", "COMSPEC", "TEMP", "TMP", "TMPDIR", "HOME",
    "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMFILES",
    "PROGRAMFILES(X86)", "COMMONPROGRAMFILES", "USERNAME", "USER", "LOGNAME", "LANG", "LANGUAGE", "LC_ALL",
    "LC_CTYPE", "TZ", "OS", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "XDG_RUNTIME_DIR",
    "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy", "SSL_CERT_FILE",
    "SSL_CERT_DIR", "NODE_EXTRA_CA_CERTS",
];

/// How many of a plugin's last stderr lines explain why it died starting.
const LAST_WORDS: usize = 5;

#[doc(hidden)]
pub mod fixture;

use flexdeck_plugin::{
    abi, value_from_dto, variable_from_dto, Ctx, Origin, Outcome, Plugin, PluginError,
};
use flexdeck_vars::{VariableSpec, VariableStore};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Something a plugin did that the rest of the agent should hear about.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// The plugin announced an event its manifest declares.
    Event {
        plugin: String,
        event: String,
        payload: BTreeMap<String, String>,
    },
    /// The plugin asked to show a notification, and is allowed to.
    Notify {
        plugin: String,
        title: String,
        body: String,
    },
    /// A line for the plugin's log.
    Log {
        plugin: String,
        level: String,
        message: String,
    },
    /// The plugin's status or variables changed, so the catalogue moved.
    Changed { plugin: String },
    /// The plugin wants these values kept for settings it declares — a token
    /// it was just issued. Only declared keys get this far. The plugin already
    /// has them, so whoever stores them must not send `settings/changed` back.
    StoreSettings {
        plugin: String,
        values: BTreeMap<String, String>,
    },
    /// The plugin asked for an http(s) address to be opened in the browser,
    /// and declared the `network` permission.
    OpenUrl { plugin: String, url: String },
    /// An image the plugin handed over, checked: size, type and checksum.
    Image {
        plugin: String,
        sha256: String,
        mime: String,
        bytes: Vec<u8>,
    },
    /// A whole history for a variable the plugin owns, for a graph.
    Series {
        plugin: String,
        name: String,
        points: Vec<f64>,
    },
}

/// What the host gives every plugin.
#[derive(Debug, Clone)]
pub struct Services {
    pub vars: Arc<VariableStore>,
    pub events: mpsc::Sender<HostEvent>,
}

/// How to start a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Extra environment, on top of the agent's own.
    pub env: Vec<(String, String)>,
}

/// Where the runtimes a package may ask for live.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Runtimes {
    /// The Node binary. `None` means `node` on the `PATH`.
    pub node: Option<PathBuf>,
    /// The Python binary. `None` means `python` (Windows) or `python3` on the `PATH`.
    pub python: Option<PathBuf>,
}

impl Runtimes {
    /// Find the runtimes this installation ships.
    ///
    /// In order: `FLEXDECK_NODE`, then a `node` binary next to the running
    /// executable (where the desktop bundle puts it), then the `PATH`.
    pub fn discover() -> Self {
        let beside = |name: &str| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
                .filter(|path| path.is_file())
        };
        let node = std::env::var_os("FLEXDECK_NODE")
            .map(PathBuf::from)
            .or_else(|| beside(if cfg!(windows) { "node.exe" } else { "node" }));
        let python = std::env::var_os("FLEXDECK_PYTHON").map(PathBuf::from);
        Self { node, python }
    }
}

impl SpawnSpec {
    /// How to start the package installed at `root`.
    pub fn for_package(
        root: &Path,
        package: &abi::PackageManifest,
        runtimes: &Runtimes,
    ) -> Result<Self, HostError> {
        package.validate()?;
        // Absolute, always. The process is started *in* `root`, so a relative
        // entry would be looked up relative to itself — `plugins/x/plugins/x/
        // index.js` — and a plugin installed under a relative data directory
        // would die before saying a word.
        let root = std::path::absolute(root)
            .map_err(|error| HostError::Spawn(format!("{}: {error}", root.display())))?;
        let root = root.as_path();
        let platform = abi::current_platform();
        let entry = package.entry_for(platform).ok_or_else(|| {
            HostError::Spawn(format!("this plugin has no entry point for {platform}"))
        })?;
        let entry = root.join(entry);
        if !entry.is_file() {
            return Err(HostError::Spawn(format!(
                "the plugin's entry point {} is missing",
                entry.display()
            )));
        }
        Ok(match package.runtime.as_str() {
            // Unbuffered, so a message is never stuck in a pipe; no PYTHON*
            // variables and no user site-packages, so it runs as packed.
            abi::runtime::PYTHON => Self {
                program: runtimes
                    .python
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" })),
                args: vec!["-u".into(), "-E".into(), "-s".into(), entry.to_string_lossy().into_owned()],
                cwd: root.to_path_buf(),
                env: Vec::new(),
            },
            abi::runtime::NODE => Self {
                program: runtimes
                    .node
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("node")),
                args: vec![entry.to_string_lossy().into_owned()],
                cwd: root.to_path_buf(),
                env: Vec::new(),
            },
            _ => Self {
                program: entry,
                args: Vec::new(),
                cwd: root.to_path_buf(),
                env: Vec::new(),
            },
        })
    }
}

/// Deadlines and limits. The defaults are what the agent uses.
#[derive(Debug, Clone)]
pub struct Options {
    pub initialize_timeout: Duration,
    /// For `invoke`. Long enough for a plugin that talks to another program.
    pub invoke_timeout: Duration,
    /// For `check` and `sample`, which a person is not waiting on as directly.
    pub query_timeout: Duration,
    pub shutdown_grace: Duration,
    /// How many crashes within [`Options::restart_window`] before giving up.
    pub max_restarts: usize,
    pub restart_window: Duration,
    /// The first restart delay; it doubles per crash, up to 30 seconds.
    pub restart_delay: Duration,
    /// Where the plugin may keep files of its own.
    pub data_dir: Option<PathBuf>,
    /// How many log lines to keep for the UI.
    pub log_lines: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            initialize_timeout: Duration::from_secs(10),
            invoke_timeout: Duration::from_secs(10),
            query_timeout: Duration::from_secs(3),
            shutdown_grace: Duration::from_secs(2),
            max_restarts: 5,
            restart_window: Duration::from_secs(60),
            restart_delay: Duration::from_secs(1),
            data_dir: None,
            log_lines: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HostError {
    #[error("the plugin could not be started: {0}")]
    Spawn(String),

    #[error(transparent)]
    Abi(#[from] abi::AbiError),

    #[error("the plugin does not match its package: {0}")]
    Mismatch(String),

    #[error("the plugin did not answer '{method}' within {seconds}s, so it was restarted")]
    Timeout { method: String, seconds: u64 },

    #[error("the plugin is not running: {0}")]
    NotRunning(String),

    #[error("the plugin exited while answering '{0}'")]
    Exited(String),

    /// Exited while starting, with the last thing it wrote to stderr — almost
    /// always the reason ("Cannot find module …", a syntax error, a crash).
    #[error("the plugin exited while answering '{method}': {said}")]
    ExitedSaying { method: String, said: String },

    #[error("{message}")]
    Remote { method: String, message: String },

    #[error("the plugin's answer to '{method}' was not understood: {detail}")]
    Decode { method: String, detail: String },
}

impl From<HostError> for PluginError {
    fn from(error: HostError) -> Self {
        match error {
            HostError::NotRunning(reason) => PluginError::Unavailable(reason),
            HostError::Remote { message, .. } => PluginError::Failed(message),
            other => PluginError::Failed(other.to_string()),
        }
    }
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    generation: u64,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
    status: Option<String>,
    crashes: VecDeque<Instant>,
    generation: u64,
}

struct Shared {
    id: String,
    spec: SpawnSpec,
    options: Options,
    services: Services,
    /// What the package declared at install; the running plugin may not
    /// claim more.
    package: abi::PackageManifest,
    manifest: Mutex<Option<abi::ManifestDto>>,
    state: Mutex<State>,
    pending: Mutex<HashMap<u64, mpsc::Sender<abi::Message>>>,
    next_id: AtomicU64,
    variables: Mutex<BTreeMap<String, VariableSpec>>,
    settings: Mutex<BTreeMap<String, String>>,
    log: Mutex<VecDeque<String>>,
    stopping: AtomicBool,
    /// Notifications that arrived before this process's `initialize` answer
    /// was accepted. A plugin may start publishing the instant it has
    /// answered, and the host has not declared its variables yet — so they
    /// wait here and are replayed, in order, once it has.
    early: Mutex<Option<Vec<(String, serde_json::Value)>>>,
    /// How much of each kind of notification the plugin may still send now.
    budgets: Mutex<HashMap<&'static str, Budget>>,
}

/// A token bucket: `rate` a second, up to `burst` at once. What is over is
/// dropped, with one line in the plugin's log per second of it, so a plugin
/// in a loop slows nothing else down.
#[derive(Debug, Clone, Copy)]
struct Budget {
    tokens: f64,
    at: std::time::Instant,
    warned: Option<std::time::Instant>,
}

/// Per notification kind: (a second, at once).
fn rate_of(method: &str) -> Option<(&'static str, f64, f64)> {
    Some(match method {
        abi::notify::VARS_SET => (abi::notify::VARS_SET, 50.0, 200.0),
        abi::notify::VARS_DECLARE => (abi::notify::VARS_DECLARE, 10.0, 50.0),
        abi::notify::EVENT_EMIT => (abi::notify::EVENT_EMIT, 20.0, 50.0),
        abi::notify::LOG => (abi::notify::LOG, 20.0, 100.0),
        abi::notify::NOTIFY => (abi::notify::NOTIFY, 0.5, 3.0),
        abi::notify::IMAGES_PUT => (abi::notify::IMAGES_PUT, 2.0, 10.0),
        abi::notify::SERIES_SET => (abi::notify::SERIES_SET, 5.0, 20.0),
        abi::notify::OPEN_URL => (abi::notify::OPEN_URL, 0.2, 2.0),
        _ => return None,
    })
}

/// An installed plugin, running as a child process.
pub struct SidecarPlugin {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for SidecarPlugin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SidecarPlugin")
            .field("id", &self.shared.id)
            .field("program", &self.shared.spec.program)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SidecarPlugin {
    /// Start the plugin, read its manifest, and check it against `package`.
    ///
    /// Returns only once the plugin has answered `initialize` with a manifest
    /// this host accepts, so a plugin that is going to be refused is refused
    /// here, with a reason, rather than appearing and then failing.
    pub fn start(
        spec: SpawnSpec,
        package: abi::PackageManifest,
        settings: BTreeMap<String, String>,
        services: Services,
        options: Options,
    ) -> Result<Self, HostError> {
        let shared = Arc::new(Shared {
            id: package.id.clone(),
            spec,
            options,
            services,
            package,
            manifest: Mutex::new(None),
            state: Mutex::new(State::default()),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            variables: Mutex::new(BTreeMap::new()),
            settings: Mutex::new(settings),
            log: Mutex::new(VecDeque::new()),
            stopping: AtomicBool::new(false),
            early: Mutex::new(None),
            budgets: Mutex::new(HashMap::new()),
        });
        if let Err(error) = shared.launch() {
            shared.stopping.store(true, Ordering::SeqCst);
            shared.kill();
            return Err(error);
        }
        Ok(Self { shared })
    }

    pub fn id(&self) -> &str {
        &self.shared.id
    }

    /// The most recent log lines, oldest first.
    pub fn log_tail(&self) -> Vec<String> {
        lock(&self.shared.log).iter().cloned().collect()
    }

    /// Whether a process is running right now.
    pub fn is_running(&self) -> bool {
        lock(&self.shared.state).running.is_some()
    }

    /// The operating-system process id, for diagnostics and tests.
    pub fn pid(&self) -> Option<u32> {
        lock(&self.shared.state)
            .running
            .as_ref()
            .map(|running| running.child.id())
    }
}

impl Shared {
    /// Start a process and complete `initialize` with it.
    fn launch(self: &Arc<Self>) -> Result<(), HostError> {
        let mut command = Command::new(&self.spec.program);
        command.env_clear();
        for key in PASSED_ENV {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .args(&self.spec.args)
            .current_dir(&self.spec.cwd)
            .envs(self.spec.env.iter().map(|(key, value)| (key, value)))
            .env("FLEXDECK_PLUGIN_ID", &self.id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the desktop app has no console, and a plugin
            // must not flash one up on every start.
            command.creation_flags(0x0800_0000);
        }

        let mut child = command.spawn().map_err(|error| {
            HostError::Spawn(format!("{}: {error}", self.spec.program.display()))
        })?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");

        *lock(&self.early) = Some(Vec::new());
        let generation = {
            let mut state = lock(&self.state);
            state.generation += 1;
            let generation = state.generation;
            state.running = Some(Running {
                child,
                stdin,
                generation,
            });
            generation
        };

        let reader = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("plugin-{}-out", self.id))
            .spawn(move || reader.read_stdout(stdout, generation))
            .map_err(|error| HostError::Spawn(error.to_string()))?;
        let logger = Arc::clone(self);
        // The last few stderr lines of *this* start, kept apart from the log so
        // a plugin that dies starting can be reported with its own words.
        let last_words: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let (stderr_done, stderr_closed) = mpsc::channel::<()>();
        let tail = Arc::clone(&last_words);
        std::thread::Builder::new()
            .name(format!("plugin-{}-err", self.id))
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    {
                        let mut tail = lock(&tail);
                        tail.push_back(line.clone());
                        while tail.len() > LAST_WORDS {
                            tail.pop_front();
                        }
                    }
                    logger.log("stderr", &line);
                }
                let _ = stderr_done.send(());
            })
            .map_err(|error| HostError::Spawn(error.to_string()))?;

        let params = abi::InitializeParams {
            // The ABI the package was built against, not the newest: a plugin
            // that predates ABI 3 is spoken to exactly as it expects.
            abi: self.package.abi,
            settings: lock(&self.settings).clone(),
            data_dir: self
                .options
                .data_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
            host_capabilities: if self.package.abi >= 4 {
                abi::capability::ALL.iter().map(|c| (*c).to_string()).collect()
            } else {
                Vec::new()
            },
        };
        let result: abi::InitializeResult = match self.call(
            abi::method::INITIALIZE,
            &params,
            self.options.initialize_timeout,
        ) {
            Ok(result) => result,
            Err(HostError::Exited(method)) => {
                // Its stderr closes as it dies; give the reader a moment to
                // catch the last line before reporting.
                let _ = stderr_closed.recv_timeout(Duration::from_millis(500));
                let said: Vec<String> = lock(&last_words)
                    .iter()
                    .map(|line| line.trim().to_string())
                    .filter(|line| !line.is_empty())
                    .collect();
                return Err(if said.is_empty() {
                    HostError::Exited(method)
                } else {
                    HostError::ExitedSaying {
                        method,
                        said: said.join(" | "),
                    }
                });
            }
            Err(error) => return Err(error),
        };
        self.accept(result)?;

        // Accepted: replay what arrived while we were deciding, then stop
        // buffering.
        let early = lock(&self.early).take().unwrap_or_default();
        for (method, params) in early {
            self.handle_notification(&method, params);
        }

        lock(&self.state).status = None;
        self.changed();
        Ok(())
    }

    /// Check an `initialize` answer and, if it is acceptable, adopt it.
    fn accept(&self, result: abi::InitializeResult) -> Result<(), HostError> {
        let manifest = result.manifest;
        manifest.validate()?;
        if manifest.abi != self.package.abi {
            return Err(HostError::Mismatch(format!(
                "the package speaks plugin ABI {} but the plugin answered in {}",
                self.package.abi, manifest.abi
            )));
        }
        if manifest.id != self.package.id {
            return Err(HostError::Mismatch(format!(
                "the package is '{}' but the plugin says it is '{}'",
                self.package.id, manifest.id
            )));
        }
        if manifest.version != self.package.version {
            return Err(HostError::Mismatch(format!(
                "the package is version {} but the plugin says {}",
                self.package.version, manifest.version
            )));
        }
        if let Some(extra) = manifest
            .permissions
            .iter()
            .find(|permission| !self.package.permissions.contains(permission))
        {
            return Err(HostError::Mismatch(format!(
                "the plugin asks for '{extra}', which its package did not declare"
            )));
        }

        let mut specs = Vec::with_capacity(result.variables.len());
        for variable in &result.variables {
            abi::validate_variable(&manifest.id, variable)?;
            specs.push(variable_from_dto(variable)?);
        }
        {
            let mut variables = lock(&self.variables);
            for spec in specs {
                self.services.vars.declare(spec.clone());
                variables.insert(spec.name.clone(), spec);
            }
        }
        *lock(&self.manifest) = Some(manifest);
        Ok(())
    }

    /// Send a request and wait for its answer, up to `timeout`.
    fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        timeout: Duration,
    ) -> Result<R, HostError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        lock(&self.pending).insert(id, sender);

        let params = serde_json::to_value(params).map_err(|error| HostError::Decode {
            method: method.to_string(),
            detail: error.to_string(),
        })?;
        let line = abi::Message::request(id, method, params).to_line();

        {
            let mut state = lock(&self.state);
            let Some(running) = state.running.as_mut() else {
                lock(&self.pending).remove(&id);
                let reason = state
                    .status
                    .clone()
                    .unwrap_or_else(|| "it has stopped".to_string());
                return Err(HostError::NotRunning(reason));
            };
            let written = running
                .stdin
                .write_all(line.as_bytes())
                .and_then(|()| running.stdin.flush());
            if written.is_err() {
                lock(&self.pending).remove(&id);
                return Err(HostError::Exited(method.to_string()));
            }
        }

        let answer = match receiver.recv_timeout(timeout) {
            Ok(answer) => answer,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&id);
                let seconds = timeout.as_secs().max(1);
                self.log(
                    "error",
                    &format!("did not answer '{method}' within {seconds}s; restarting"),
                );
                // Killing it is the only way to get the next press a plugin
                // that is listening; the reader sees the exit and restarts it.
                self.kill_running();
                return Err(HostError::Timeout {
                    method: method.to_string(),
                    seconds,
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(HostError::Exited(method.to_string()))
            }
        };

        if let Some(error) = answer.error {
            return Err(HostError::Remote {
                method: method.to_string(),
                message: error.message,
            });
        }
        serde_json::from_value(answer.result.unwrap_or(serde_json::Value::Null)).map_err(|error| {
            HostError::Decode {
                method: method.to_string(),
                detail: error.to_string(),
            }
        })
    }

    fn read_stdout(self: Arc<Self>, stdout: std::process::ChildStdout, generation: u64) {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let message: abi::Message = match serde_json::from_str(trimmed) {
                Ok(message) => message,
                Err(_) => {
                    // A stray `console.log` must not kill the plugin; it is
                    // kept as a log line instead.
                    self.log("stdout", trimmed);
                    continue;
                }
            };
            if message.is_response() {
                let id = message.id.expect("a response has an id");
                if let Some(sender) = lock(&self.pending).remove(&id) {
                    let _ = sender.send(message);
                }
            } else if let Some(method) = message.method.clone() {
                let params = message.params.unwrap_or_default();
                {
                    let mut early = lock(&self.early);
                    if let Some(buffer) = early.as_mut() {
                        // Bounded: a plugin that floods before it is even
                        // accepted loses the excess, not the agent's memory.
                        if buffer.len() < 1_000 {
                            buffer.push((method, params));
                        }
                        continue;
                    }
                }
                self.handle_notification(&method, params);
            }
        }
        self.exited(generation);
    }

    /// Whether one more `method` fits this plugin's budget now.
    fn within_budget(&self, method: &str) -> bool {
        let Some((key, rate, burst)) = rate_of(method) else { return true };
        let now = std::time::Instant::now();
        let mut budgets = lock(&self.budgets);
        let budget = budgets.entry(key).or_insert(Budget { tokens: burst, at: now, warned: None });
        budget.tokens = (budget.tokens + now.duration_since(budget.at).as_secs_f64() * rate).min(burst);
        budget.at = now;
        if budget.tokens >= 1.0 {
            budget.tokens -= 1.0;
            return true;
        }
        let warn = budget.warned.is_none_or(|at| now.duration_since(at).as_secs() >= 1);
        if warn {
            budget.warned = Some(now);
        }
        drop(budgets);
        if warn && method != abi::notify::LOG {
            self.log("warn", &format!("sending '{method}' faster than {rate} a second; the rest are dropped"));
        }
        false
    }

    fn handle_notification(&self, method: &str, params: serde_json::Value) {
        if !self.within_budget(method) {
            return;
        }
        let outcome = match method {
            abi::notify::VARS_SET => decode(params)
                .and_then(|set: abi::VarsSet| self.publish(&set.values, &self.services.vars)),
            abi::notify::VARS_DECLARE => decode(params).and_then(|declare| self.declare(declare)),
            abi::notify::EVENT_EMIT => decode(params).and_then(|emit| self.emit(emit)),
            abi::notify::LOG => decode(params).map(|line: abi::LogLine| {
                self.log(&line.level, &line.message);
            }),
            abi::notify::NOTIFY => decode(params).and_then(|notice| self.notify(notice)),
            abi::notify::SETTINGS_STORE if self.package.abi >= 3 => {
                decode(params).and_then(|store| self.store_settings(store))
            }
            abi::notify::OPEN_URL if self.package.abi >= 3 => {
                decode(params).and_then(|open| self.open_url(open))
            }
            abi::notify::IMAGES_PUT if self.package.abi >= 3 => {
                decode(params).and_then(|image| self.put_image(image))
            }
            abi::notify::SERIES_SET if self.uses(abi::capability::SERIES) => {
                decode(params).and_then(|series| self.series(series))
            }
            other => Err(format!("'{other}' is not something a plugin can send")),
        };
        if let Err(reason) = outcome {
            self.log("error", &format!("refused '{method}': {reason}"));
        }
    }

    /// Write values the plugin owns into `vars`.
    ///
    /// `vars` is a parameter rather than always the live store because a
    /// dry run samples into a scratch store.
    fn publish(
        &self,
        values: &BTreeMap<String, abi::ValueDto>,
        vars: &VariableStore,
    ) -> Result<(), String> {
        let prefix = format!("{}.", self.id);
        let mut refused = Vec::new();
        for (name, dto) in values {
            if !name.starts_with(&prefix) {
                refused.push(format!("'{name}' is not under '{prefix}'"));
                continue;
            }
            if vars.spec(name).is_none() {
                // Declared on the live store but not on a scratch one: copy
                // the declaration across so a dry run can see it.
                if let Some(spec) = lock(&self.variables).get(name) {
                    vars.declare(spec.clone());
                }
            }
            let written = value_from_dto(dto)
                .map_err(|error| error.to_string())
                .and_then(|value| vars.refresh(name, value).map_err(|e| e.to_string()));
            if let Err(error) = written {
                refused.push(format!("'{name}': {error}"));
            }
        }
        if refused.is_empty() {
            Ok(())
        } else {
            Err(refused.join("; "))
        }
    }

    fn declare(&self, declare: abi::VarsDeclare) -> Result<(), String> {
        let mut specs = Vec::with_capacity(declare.variables.len());
        for variable in &declare.variables {
            abi::validate_variable(&self.id, variable).map_err(|error| error.to_string())?;
            specs.push(variable_from_dto(variable).map_err(|error| error.to_string())?);
        }
        {
            let mut variables = lock(&self.variables);
            if declare.replace {
                let keep: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
                variables.retain(|name, _| {
                    let kept = keep.contains(&name.as_str());
                    if !kept {
                        self.services.vars.undeclare(name);
                    }
                    kept
                });
            }
            for spec in specs {
                self.services.vars.declare(spec.clone());
                variables.insert(spec.name.clone(), spec);
            }
        }
        self.changed();
        Ok(())
    }

    fn emit(&self, emit: abi::EventEmit) -> Result<(), String> {
        let declared = lock(&self.manifest)
            .as_ref()
            .is_some_and(|manifest| manifest.events.iter().any(|e| e.name == emit.event));
        if !declared {
            return Err(format!(
                "'{}' is not an event this plugin declares",
                emit.event
            ));
        }
        let _ = self.services.events.send(HostEvent::Event {
            plugin: self.id.clone(),
            event: emit.event,
            payload: emit.payload,
        });
        Ok(())
    }

    fn notify(&self, notice: abi::NotifyRequest) -> Result<(), String> {
        if !self.has_permission(abi::permission::NOTIFICATIONS) {
            return Err("this plugin did not ask for the notifications permission".into());
        }
        let _ = self.services.events.send(HostEvent::Notify {
            plugin: self.id.clone(),
            title: notice.title,
            body: notice.body,
        });
        Ok(())
    }

    fn has_permission(&self, wanted: &str) -> bool {
        lock(&self.manifest)
            .as_ref()
            .is_some_and(|manifest| manifest.permissions.iter().any(|p| p == wanted))
    }

    fn store_settings(&self, store: abi::SettingsStore) -> Result<(), String> {
        {
            let manifest = lock(&self.manifest);
            let declared = manifest.as_ref().map(|m| &m.settings);
            if let Some(key) = store.values.keys().find(|key| {
                !declared.is_some_and(|settings| settings.iter().any(|s| &&s.key == key))
            }) {
                return Err(format!("'{key}' is not a setting this plugin declares"));
            }
        }
        if store.values.is_empty() {
            return Ok(());
        }
        // Kept here too, so a restart hands the plugin what it stored even
        // before the agent has written it anywhere.
        lock(&self.settings).extend(store.values.clone());
        let _ = self.services.events.send(HostEvent::StoreSettings {
            plugin: self.id.clone(),
            values: store.values,
        });
        Ok(())
    }

    fn open_url(&self, open: abi::OpenUrl) -> Result<(), String> {
        if !self.has_permission(abi::permission::NETWORK) {
            return Err("this plugin did not ask for the network permission".into());
        }
        if !open.is_allowed() {
            return Err(format!("'{}' is not an http(s) address", open.url));
        }
        let _ = self.services.events.send(HostEvent::OpenUrl {
            plugin: self.id.clone(),
            url: open.url,
        });
        Ok(())
    }

    /// Whether the running plugin said it uses `capability`.
    fn uses(&self, capability: &str) -> bool {
        lock(&self.manifest).as_ref().is_some_and(|manifest| manifest.uses(capability))
    }

    /// A whole history for a variable this plugin owns.
    fn series(&self, series: abi::SeriesSet) -> Result<(), String> {
        let prefix = format!("{}.", self.id);
        if !series.name.starts_with(&prefix) {
            return Err(format!("'{}' is not under '{prefix}'", series.name));
        }
        if series.points.len() > abi::MAX_SERIES_POINTS {
            return Err(format!("at most {} points", abi::MAX_SERIES_POINTS));
        }
        let _ = self.services.events.send(HostEvent::Series {
            plugin: self.id.clone(),
            name: series.name,
            points: series.points.into_iter().filter(|p| p.is_finite()).collect(),
        });
        Ok(())
    }

    fn put_image(&self, image: abi::ImagePut) -> Result<(), String> {
        use base64::Engine as _;
        use sha2::Digest as _;

        if !abi::IMAGE_MIMES.contains(&image.mime.as_str()) {
            return Err(format!("'{}' is not an image type the agent shows", image.mime));
        }
        // Checked on the encoded length first, so a huge payload is refused
        // without decoding it.
        if image.data.len() > abi::MAX_IMAGE_BYTES.div_ceil(3) * 4 {
            return Err(format!("the image is over {} bytes", abi::MAX_IMAGE_BYTES));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.data.as_bytes())
            .map_err(|error| format!("the image is not base64: {error}"))?;
        if bytes.len() > abi::MAX_IMAGE_BYTES {
            return Err(format!("the image is over {} bytes", abi::MAX_IMAGE_BYTES));
        }
        let sha256 = hex::encode(sha2::Sha256::digest(&bytes));
        if sha256 != image.sha256.to_ascii_lowercase() {
            return Err("the image's checksum does not match its bytes".into());
        }
        let _ = self.services.events.send(HostEvent::Image {
            plugin: self.id.clone(),
            sha256,
            mime: image.mime,
            bytes,
        });
        Ok(())
    }

    fn log(&self, level: &str, message: &str) {
        {
            let mut log = lock(&self.log);
            log.push_back(format!("[{level}] {message}"));
            while log.len() > self.options.log_lines {
                log.pop_front();
            }
        }
        let _ = self.services.events.send(HostEvent::Log {
            plugin: self.id.clone(),
            level: level.to_string(),
            message: message.to_string(),
        });
    }

    fn changed(&self) {
        let _ = self.services.events.send(HostEvent::Changed {
            plugin: self.id.clone(),
        });
    }

    /// The process behind `generation` has gone. Restart it unless we are
    /// stopping or it has crashed too often.
    fn exited(self: Arc<Self>, generation: u64) {
        let code = {
            let mut state = lock(&self.state);
            match state.running.take() {
                Some(mut running) if running.generation == generation => {
                    // Closing stdout is not the same as exiting. Kill first so
                    // the wait cannot hang while holding the state lock; on a
                    // process that already exited the kill is a no-op.
                    let _ = running.child.kill();
                    running.child.wait().ok().and_then(|status| status.code())
                }
                // Not the current process — an old one finally closing.
                other => {
                    state.running = other;
                    return;
                }
            }
        };
        // Everyone waiting on an answer gets "exited" now, not at their
        // deadline.
        lock(&self.pending).clear();

        if self.stopping.load(Ordering::SeqCst) {
            return;
        }

        let described = code.map_or_else(
            || "it was stopped".to_string(),
            |c| format!("exit code {c}"),
        );
        let delay = {
            let mut state = lock(&self.state);
            let now = Instant::now();
            state.crashes.push_back(now);
            while state
                .crashes
                .front()
                .is_some_and(|at| now.duration_since(*at) > self.options.restart_window)
            {
                state.crashes.pop_front();
            }
            let crashes = state.crashes.len();
            if crashes > self.options.max_restarts {
                state.status = Some(format!(
                    "stopped after crashing {crashes} times in {}s ({described}); reload it to try again",
                    self.options.restart_window.as_secs()
                ));
                None
            } else {
                state.status = Some(format!("restarting after it stopped ({described})"));
                let factor = 1u32 << (crashes.saturating_sub(1)).min(5);
                Some((self.options.restart_delay * factor).min(Duration::from_secs(30)))
            }
        };
        self.log("error", &format!("stopped unexpectedly ({described})"));
        self.changed();

        let Some(delay) = delay else { return };
        let _ = std::thread::Builder::new()
            .name(format!("plugin-{}-restart", self.id))
            .spawn(move || {
                std::thread::sleep(delay);
                if self.stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Err(error) = self.launch() {
                    lock(&self.state).status = Some(error.to_string());
                    self.log("error", &format!("could not restart: {error}"));
                    self.kill_running();
                    self.changed();
                }
            });
    }

    fn kill_running(&self) {
        if let Some(running) = lock(&self.state).running.as_mut() {
            let _ = running.child.kill();
        }
    }

    /// Stop for good: kill, reap, and leave nothing behind.
    fn kill(&self) {
        let running = lock(&self.state).running.take();
        if let Some(mut running) = running {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }

    fn shutdown(&self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        let _: Result<serde_json::Value, _> = self.call(
            abi::method::SHUTDOWN,
            &serde_json::json!({}),
            self.options.shutdown_grace,
        );
        // Give a polite plugin its grace period to exit by itself.
        let deadline = Instant::now() + self.options.shutdown_grace;
        while Instant::now() < deadline {
            let exited = lock(&self.state)
                .running
                .as_mut()
                .is_none_or(|running| matches!(running.child.try_wait(), Ok(Some(_))));
            if exited {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.kill();
        lock(&self.state).status = Some("stopped".to_string());
    }
}

fn decode<T: DeserializeOwned>(params: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(params).map_err(|error| error.to_string())
}

fn ordered(args: &HashMap<String, String>) -> BTreeMap<String, String> {
    args.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

impl Plugin for SidecarPlugin {
    fn manifest(&self) -> abi::ManifestDto {
        lock(&self.shared.manifest)
            .clone()
            .expect("start() only returns once a manifest is accepted")
    }

    fn variables(&self) -> Vec<VariableSpec> {
        lock(&self.shared.variables).values().cloned().collect()
    }

    fn check(&self, action: &str, args: &HashMap<String, String>) -> Result<(), PluginError> {
        let response: abi::CheckResponse = self.shared.call(
            abi::method::CHECK,
            &abi::CheckRequest {
                action: action.to_string(),
                args: ordered(args),
            },
            self.shared.options.query_timeout,
        )?;
        if response.ok {
            Ok(())
        } else {
            Err(PluginError::Failed(response.error.unwrap_or_else(|| {
                format!("the plugin refused '{action}' without saying why")
            })))
        }
    }

    fn choices(
        &self,
        action: &str,
        args: &HashMap<String, String>,
    ) -> Result<abi::ChoicesResponse, PluginError> {
        match self.shared.call::<_, abi::ChoicesResponse>(
            abi::method::CHOICES,
            &abi::ChoicesRequest {
                action: action.to_string(),
                args: ordered(args),
            },
            self.shared.options.query_timeout,
        ) {
            Ok(response) => Ok(response),
            // A plugin built before `choices` existed answers "no method".
            // That is "nothing to suggest", not a failure: a plugin that has
            // suggestions but cannot get them says so in `error` instead.
            Err(HostError::Remote { .. }) => Ok(abi::ChoicesResponse::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn watch_feed(&mut self, instance: &str, feed: &str, args: &BTreeMap<String, String>) -> Result<(), PluginError> {
        if !self.shared.uses(abi::capability::FEEDS) {
            return Err(PluginError::Failed(format!("{} does not offer feeds", self.shared.id)));
        }
        self.shared
            .call::<_, serde_json::Value>(
                abi::method::FEED_WATCH,
                &abi::FeedWatch { instance: instance.to_string(), feed: feed.to_string(), args: args.clone() },
                self.shared.options.query_timeout,
            )
            .map(drop)
            .map_err(Into::into)
    }

    fn unwatch_feed(&mut self, instance: &str) -> Result<(), PluginError> {
        if !self.shared.uses(abi::capability::FEEDS) {
            return Ok(());
        }
        // The key's values go too, so a picker does not offer a key long gone.
        self.shared.services.vars.undeclare_prefix(&format!("{}.@{instance}.", self.shared.id));
        self.shared
            .call::<_, serde_json::Value>(
                abi::method::FEED_UNWATCH,
                &abi::FeedUnwatch { instance: instance.to_string() },
                self.shared.options.query_timeout,
            )
            .map(drop)
            .map_err(Into::into)
    }

    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let response: abi::SampleResponse = self.shared.call(
            abi::method::SAMPLE,
            &abi::SampleRequest {
                wanted: wanted.iter().map(|name| (*name).to_string()).collect(),
            },
            self.shared.options.query_timeout,
        )?;
        self.shared
            .publish(&response.values, ctx.vars)
            .map_err(PluginError::Failed)
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        _ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        let response: abi::InvokeResponse = self.shared.call(
            abi::method::INVOKE,
            &abi::InvokeRequest {
                action: action.to_string(),
                args: ordered(args),
            },
            self.shared.options.invoke_timeout,
        )?;
        if response.ok {
            Ok(Outcome {
                input: None,
                state: response.state,
            })
        } else {
            Err(PluginError::Failed(response.error.unwrap_or_else(|| {
                format!("'{action}' failed without saying why")
            })))
        }
    }

    fn settings_changed(&mut self, settings: &BTreeMap<String, String>) {
        *lock(&self.shared.settings) = settings.clone();
        let sent: Result<serde_json::Value, HostError> = self.shared.call(
            abi::method::SETTINGS_CHANGED,
            &abi::SettingsChanged {
                settings: settings.clone(),
            },
            self.shared.options.query_timeout,
        );
        if let Err(error) = sent {
            self.shared
                .log("error", &format!("could not apply new settings: {error}"));
        }
    }

    fn status(&self) -> Option<String> {
        lock(&self.shared.state).status.clone()
    }

    fn origin(&self) -> Origin {
        Origin::Installed
    }

    fn shutdown(&mut self) {
        self.shared.shutdown();
    }
}

impl Drop for SidecarPlugin {
    /// A plugin dropped without `shutdown` must not outlive the agent as an
    /// orphan process.
    fn drop(&mut self) {
        self.shared.shutdown();
    }
}

//! Will this plugin work in FlexDeck? Asked of a plugin before anyone installs
//! it, by running it the way the agent runs it.
//!
//! Not a linter: the plugin is started by the real host, with the real
//! deadlines and the real rules, and taken through what the agent does with
//! a plugin — introducing itself, its catalogue checked as the agent checks
//! it, every action's example arguments checked by the plugin itself, its
//! values sampled, its feeds watched and dropped, its settings changed, its
//! gallery keys checked as a saved deck is, and a clean stop. Anything it
//! sends that the host refuses — a value outside its namespace, an event it
//! never declared, a notification without the permission, too much too fast
//! — fails the check it happened in. A recipe is read and checked as the
//! agent reads one.
//!
//! `flexdeck verify-plugin <folder | .fdpkg>`, and both SDKs' `verify`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flexdeck_plugin::abi;
use flexdeck_plugin::{Ctx, Plugin, RecordingNotifier};
use flexdeck_plugin_host::{HostEvent, Options, Runtimes, Services, SidecarPlugin, SpawnSpec};
use flexdeck_vars::VariableStore;
use serde::Serialize;

/// One thing checked, and how it went.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    /// Why not; or, when it passed, anything worth knowing (timings, counts).
    pub detail: String,
}

/// Everything checked about one plugin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub plugin: String,
    pub runtime: String,
    pub checks: Vec<Check>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|check| check.ok)
    }

    /// The report as a person reads it: a line per check.
    pub fn text(&self) -> String {
        let mut out = format!("{} ({})\n", if self.plugin.is_empty() { "?" } else { &self.plugin }, self.runtime);
        for check in &self.checks {
            let mark = if check.ok { "  ok  " } else { "  FAIL" };
            out.push_str(&format!("{mark}  {}", check.name));
            if !check.detail.is_empty() {
                out.push_str(&format!(" — {}", check.detail));
            }
            out.push('\n');
        }
        let failed = self.checks.iter().filter(|c| !c.ok).count();
        out.push_str(&if failed == 0 {
            "\nIt will work in FlexDeck.\n".to_string()
        } else {
            format!("\n{failed} check{} failed: fix {} before anyone installs it.\n", if failed == 1 { "" } else { "s" }, if failed == 1 { "it" } else { "them" })
        });
        out
    }

    fn pass(&mut self, name: &str, detail: impl Into<String>) {
        self.checks.push(Check { name: name.into(), ok: true, detail: detail.into() });
    }

    fn fail(&mut self, name: &str, detail: impl Into<String>) {
        self.checks.push(Check { name: name.into(), ok: false, detail: detail.into() });
    }

    fn record(&mut self, name: &str, outcome: Result<String, String>) {
        match outcome {
            Ok(detail) => self.pass(name, detail),
            Err(detail) => self.fail(name, detail),
        }
    }
}

/// How to verify.
#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    /// Also run every action with its example arguments. Off by default: an
    /// action does real things (posts, switches, sends).
    pub run_actions: bool,
    pub runtimes: Option<Runtimes>,
    /// Deadlines; the agent's own when absent.
    pub host: Option<Options>,
}

/// A folder to work in, removed when done.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("flexdeck-verify-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Verify the plugin at `path`: a package folder (with `plugin.json`) or a
/// `.fdpkg`.
pub fn verify(path: &Path, options: &VerifyOptions) -> Report {
    let mut report = Report { plugin: String::new(), runtime: String::new(), checks: Vec::new() };
    let scratch = match Scratch::new() {
        Ok(scratch) => scratch,
        Err(error) => {
            report.fail("a folder to work in", error.to_string());
            return report;
        }
    };

    // ---- The package -----------------------------------------------------------------
    let root = if path.is_file() {
        let unpacked = scratch.0.join("package");
        let outcome = std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| crate::plugins::unpack_into(&bytes, &unpacked).map_err(|error| error.to_string()));
        if let Err(error) = outcome {
            report.fail("the package opens", error);
            return report;
        }
        report.pass("the package opens", "");
        unpacked
    } else {
        path.to_path_buf()
    };
    let package = match std::fs::read_to_string(root.join(abi::PACKAGE_MANIFEST))
        .map_err(|error| format!("{}: {error}", abi::PACKAGE_MANIFEST))
        .and_then(|text| serde_json::from_str::<abi::PackageManifest>(&text).map_err(|error| format!("{}: {error}", abi::PACKAGE_MANIFEST)))
        .and_then(|package| package.validate().map(|()| package).map_err(|error| error.to_string()))
    {
        Ok(package) => package,
        Err(error) => {
            report.fail("plugin.json is one the agent accepts", error);
            return report;
        }
    };
    report.plugin = format!("{} {}", package.id, package.version);
    report.runtime = package.runtime.clone();
    report.pass("plugin.json is one the agent accepts", format!("ABI {}, permissions: {}", package.abi, if package.permissions.is_empty() { "none".into() } else { package.permissions.join(", ") }));

    if package.runtime == abi::runtime::RECIPE {
        verify_recipe(&root, &package, &mut report);
        return report;
    }

    // ---- Running it -------------------------------------------------------------------
    let runtimes = options.runtimes.clone().unwrap_or_else(Runtimes::discover);
    let spec = match SpawnSpec::for_package(&root, &package, &runtimes) {
        Ok(spec) => spec,
        Err(error) => {
            report.fail("it can be started", error.to_string());
            return report;
        }
    };
    let vars = Arc::new(VariableStore::new());
    let (sender, events) = mpsc::channel();
    let data = scratch.0.join("data");
    let _ = std::fs::create_dir_all(&data);
    let host = Options { data_dir: Some(data), ..options.host.clone().unwrap_or_default() };
    let started = Instant::now();
    let mut plugin = match SidecarPlugin::start(spec, package.clone(), BTreeMap::new(), Services { vars: Arc::clone(&vars), events: sender }, host) {
        Ok(plugin) => plugin,
        Err(error) => {
            report.fail("it starts and introduces itself", error.to_string());
            return report;
        }
    };
    report.pass("it starts and introduces itself", format!("in {} ms", started.elapsed().as_millis()));
    let mut lane = Lane { events, seen: Vec::new(), emitted: Vec::new() };

    let manifest = plugin.manifest();
    report.record(
        "its catalogue is one the agent accepts",
        manifest.validate().map(|()| summary(&manifest)).map_err(|error| error.to_string()),
    );
    report.record("its gallery keys could be saved in a deck", presets(&manifest));
    report.record("every argument has an example that fits it", examples(&manifest));
    report.record("its variables are its own", own_variables(&package.id, &plugin));
    lane.settle(&mut report, "it stays in its own lane while starting");

    // Every action, asked of the plugin as the editor does before saving a key.
    let mut failures = Vec::new();
    for action in &manifest.actions {
        let args = example_args(&action.arguments);
        if let Err(error) = plugin.check(&action.action, &args) {
            failures.push(format!("{}: {error}", action.action));
        }
        if let Err(error) = plugin.choices(&action.action, &args) {
            failures.push(format!("{} (its choices): {error}", action.action));
        }
    }
    report.record(
        "it accepts every action's example arguments",
        if failures.is_empty() { Ok(format!("{} action{}", manifest.actions.len(), plural(manifest.actions.len()))) } else { Err(failures.join("; ")) },
    );
    lane.settle(&mut report, "it stays in its own lane while checking actions");

    if options.run_actions {
        let mut failures = Vec::new();
        for action in &manifest.actions {
            let args = example_args(&action.arguments);
            let began = Instant::now();
            if let Err(error) = with_ctx(&vars, |ctx| plugin.invoke(&action.action, &args, ctx).map(drop)) {
                failures.push(format!("{}: {error}", action.action));
            } else if began.elapsed() > Duration::from_secs(5) {
                failures.push(format!("{}: took {} s — a press should answer within a few", action.action, began.elapsed().as_secs()));
            }
        }
        report.record(
            "every action runs with its example arguments",
            if failures.is_empty() { Ok(String::new()) } else { Err(failures.join("; ")) },
        );
        lane.settle(&mut report, "it stays in its own lane while running actions");
    }

    // Sampled as a deck showing every value would.
    let names: Vec<String> = plugin.variables().into_iter().map(|spec| spec.name).collect();
    let wanted: Vec<&str> = names.iter().map(String::as_str).collect();
    let began = Instant::now();
    let sampled = with_ctx(&vars, |ctx| plugin.sample(&wanted, ctx));
    report.record(
        "it answers when its values are wanted",
        sampled.map(|()| format!("{} value{} in {} ms", names.len(), plural(names.len()), began.elapsed().as_millis())).map_err(|error| error.to_string()),
    );
    lane.settle(&mut report, "every value it sends is declared, its own and of its kind");

    // Each feed watched for a key, then dropped, as keys come and go.
    if !manifest.feeds.is_empty() {
        let mut failures = Vec::new();
        for feed in &manifest.feeds {
            let args: BTreeMap<String, String> = example_args(&feed.arguments).into_iter().collect();
            if let Err(error) = plugin.watch_feed("verify", &feed.name, &args) {
                failures.push(format!("{} (watch): {error}", feed.name));
                continue;
            }
            std::thread::sleep(Duration::from_millis(300));
            if let Err(error) = plugin.unwatch_feed("verify") {
                failures.push(format!("{} (unwatch): {error}", feed.name));
            }
        }
        let prefix = format!("{}.@verify.", package.id);
        let left: Vec<String> = vars.specs().into_iter().map(|s| s.name).filter(|n| n.starts_with(&prefix)).collect();
        if !left.is_empty() {
            failures.push(format!("values left behind after the key went: {}", left.join(", ")));
        }
        report.record(
            "its feeds start and stop for a key",
            if failures.is_empty() { Ok(format!("{} feed{}", manifest.feeds.len(), plural(manifest.feeds.len()))) } else { Err(failures.join("; ")) },
        );
        lane.settle(&mut report, "it stays in its own lane while feeding keys");
    }

    // Settings changed, as when someone saves the plugin's settings form.
    if !manifest.settings.is_empty() {
        let settings: BTreeMap<String, String> = manifest
            .settings
            .iter()
            .map(|s| (s.key.clone(), s.default_value.clone().unwrap_or_default()))
            .collect();
        plugin.settings_changed(&settings);
        std::thread::sleep(Duration::from_millis(200));
        report.record(
            "it carries on when its settings change",
            match plugin.status() {
                Some(status) => Err(status),
                None => Ok(String::new()),
            },
        );
        lane.settle(&mut report, "it stays in its own lane after a settings change");
    }

    let alive = plugin.status();
    report.record("it is still running after all that", alive.map_or(Ok(String::new()), Err));

    let began = Instant::now();
    plugin.shutdown();
    let gone = plugin.pid().is_none();
    report.record(
        "it stops when asked",
        if gone { Ok(format!("in {} ms", began.elapsed().as_millis())) } else { Err("its process was still there".into()) },
    );
    if !lane.emitted.is_empty() {
        report.pass("events it announced", lane.emitted.join(", "));
    }
    report
}

fn verify_recipe(root: &Path, package: &abi::PackageManifest, report: &mut Report) {
    let file = package.recipe.as_deref().unwrap_or_default();
    let text = match std::fs::read_to_string(root.join(file)) {
        Ok(text) => text,
        Err(error) => return report.fail("its recipe is there", format!("{file}: {error}")),
    };
    match crate::recipes::parse(&package.id, &text) {
        Ok(recipe) => {
            report.pass(
                "its recipe is one the agent runs",
                format!("{} source{}, {} gallery key{}", recipe.sources.len(), plural(recipe.sources.len()), recipe.presets.len(), plural(recipe.presets.len())),
            );
            if !recipe.sources.is_empty() && !package.permissions.iter().any(|p| p == abi::permission::NETWORK) {
                report.fail("it asks for what it uses", "it reads the web, so it must declare the network permission");
            } else {
                report.pass("it asks for what it uses", "");
            }
            let secrets: Vec<String> = recipe
                .sources
                .iter()
                .flat_map(|s| s.args.iter().filter(|a| a.secret && !a.default.is_empty()).map(|a| format!("{}.{}", s.id, a.name)))
                .collect();
            if secrets.is_empty() {
                report.pass("it carries no secret", "");
            } else {
                report.fail("it carries no secret", format!("these secret settings have a value in the package: {}", secrets.join(", ")));
            }
        }
        Err(error) => report.fail("its recipe is one the agent runs", error),
    }
}

/// Host events, read as they come: refusals fail the check in hand.
struct Lane {
    events: mpsc::Receiver<HostEvent>,
    seen: Vec<String>,
    emitted: Vec<String>,
}

impl Lane {
    fn settle(&mut self, report: &mut Report, name: &str) {
        std::thread::sleep(Duration::from_millis(150));
        let mut refused = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                HostEvent::Log { level, message, .. } if is_refusal(&level, &message) => refused.push(format!("{level}: {message}")),
                HostEvent::Event { event, .. } => {
                    if !self.emitted.contains(&event) {
                        self.emitted.push(event);
                    }
                }
                HostEvent::Log { message, .. } => self.seen.push(message),
                _ => {}
            }
        }
        if refused.is_empty() {
            report.pass(name, "");
        } else {
            report.fail(name, refused.join("; "));
        }
    }
}

/// A host log line saying it refused or dropped something the plugin sent,
/// or that the plugin wrote to stdout what is not the protocol (a stray print
/// there could one day be read as a message).
fn is_refusal(level: &str, message: &str) -> bool {
    (level == "error" && message.starts_with("refused")) || (level == "warn" && message.contains("faster than")) || level == "stdout"
}

/// Keys pressed by an action while verifying go nowhere.
struct NoInput;

impl flexdeck_plugin::Executor for NoInput {
    fn perform(&mut self, _action: &flexdeck_plugin::CoreAction) -> Result<(), flexdeck_actions::ActionError> {
        Ok(())
    }
}

fn with_ctx<T>(vars: &VariableStore, run: impl FnOnce(&mut Ctx<'_>) -> T) -> T {
    let mut input = NoInput;
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx { input: &mut input, vars, notifier: &mut notifier, settings: &settings, now_ms: 0 };
    run(&mut ctx)
}

fn example_args(arguments: &[abi::ArgumentDto]) -> HashMap<String, String> {
    arguments.iter().filter(|a| !(a.optional && a.example.is_empty())).map(|a| (a.name.clone(), a.example.clone())).collect()
}

fn summary(manifest: &abi::ManifestDto) -> String {
    let count = |n: usize, what: &str| format!("{n} {what}{}", plural(n));
    [
        count(manifest.actions.len(), "action"),
        count(manifest.events.len(), "event"),
        count(manifest.settings.len(), "setting"),
        count(manifest.feeds.len(), "feed"),
        count(manifest.presets.len(), "gallery key"),
    ]
    .join(", ")
}

fn presets(manifest: &abi::ManifestDto) -> Result<String, String> {
    let mut problems = Vec::new();
    for preset in &manifest.presets {
        let mut widget = preset.widget.clone();
        widget["id"] = serde_json::Value::String(format!("{}-{}", manifest.id, preset.id));
        if let Err(problem) = crate::widgets::check_widget(&widget) {
            problems.push(format!("'{}': {problem}", preset.id));
        }
    }
    if problems.is_empty() {
        Ok(if manifest.presets.is_empty() { "none".into() } else { format!("{} checked", manifest.presets.len()) })
    } else {
        Err(problems.join("; "))
    }
}

fn examples(manifest: &abi::ManifestDto) -> Result<String, String> {
    let mut problems = Vec::new();
    let arguments = manifest
        .actions
        .iter()
        .flat_map(|a| a.arguments.iter().map(move |arg| (a.action.as_str(), arg)))
        .chain(manifest.feeds.iter().flat_map(|f| f.arguments.iter().map(move |arg| (f.name.as_str(), arg))));
    for (owner, arg) in arguments {
        let at = format!("{owner}.{}", arg.name);
        if arg.example.is_empty() {
            if !arg.optional {
                problems.push(format!("{at} has no example: the editor fills it in, and this check runs with it"));
            }
            continue;
        }
        match arg.kind.as_str() {
            abi::kind::NUMBER if arg.example.trim().parse::<f64>().is_err() => problems.push(format!("{at}: '{}' is not a number", arg.example)),
            abi::kind::BOOL if !["true", "false"].contains(&arg.example.as_str()) => problems.push(format!("{at}: '{}' is not true or false", arg.example)),
            abi::kind::CHOICE if !arg.choices.is_empty() && !arg.choices.contains(&arg.example) => {
                problems.push(format!("{at}: '{}' is not one of its choices", arg.example))
            }
            _ => {}
        }
    }
    if problems.is_empty() {
        Ok(String::new())
    } else {
        Err(problems.join("; "))
    }
}

fn own_variables(id: &str, plugin: &SidecarPlugin) -> Result<String, String> {
    let prefix = format!("{id}.");
    let names: Vec<String> = plugin.variables().into_iter().map(|s| s.name).collect();
    let stray: Vec<&String> = names.iter().filter(|n| !n.starts_with(&prefix)).collect();
    if stray.is_empty() {
        Ok(format!("{} under {prefix}", names.len()))
    } else {
        Err(format!("not under {prefix}: {}", stray.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

pub const USAGE: &str = "Usage: flexdeck verify-plugin <folder | .fdpkg> [--run-actions] [--json] [--out <file>]";

/// `verify-plugin`'s command line, for every binary that offers it (the
/// agent and the desktop app): the arguments after `verify-plugin`; the
/// process's exit code back. `--out` writes the JSON report to a file, for an
/// SDK to read.
pub fn cli(arguments: &[String]) -> i32 {
    let mut path = None;
    let mut options = VerifyOptions::default();
    let mut json = false;
    let mut out = None;
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--run-actions" => options.run_actions = true,
            "--json" => json = true,
            "--out" => match arguments.next() {
                Some(file) => out = Some(PathBuf::from(file)),
                None => {
                    eprintln!("--out needs a file");
                    return 2;
                }
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                println!();
                println!("Runs the plugin the way FlexDeck does and checks it at every step:");
                println!("its package and catalogue, its gallery keys, its actions' examples,");
                println!("its values, feeds and settings, that it keeps to its own namespace,");
                println!("and that it stops cleanly. --run-actions also runs every action with");
                println!("its examples (they do real things).");
                return 0;
            }
            other if path.is_none() && !other.starts_with("--") => path = Some(PathBuf::from(other)),
            other => {
                eprintln!("unrecognised argument '{other}'
{USAGE}");
                return 2;
            }
        }
    }
    let Some(path) = path else {
        eprintln!("{USAGE}");
        return 2;
    };
    let report = verify(&path, &options);
    let as_json = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Some(out) = out {
        if let Err(error) = std::fs::write(&out, &as_json) {
            eprintln!("{}: {error}", out.display());
            return 2;
        }
    }
    if json {
        println!("{as_json}");
    } else {
        print!("{}", report.text());
    }
    i32::from(!report.ok())
}

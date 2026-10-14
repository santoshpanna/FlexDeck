//! The `launcher` plugin: open a website, a file, a folder or an app, run a
//! command, or close a program.
//!
//! Like `system`, it takes its capability as an argument: [`LiveLauncher`]
//! really starts things, [`RecordingLauncher`] writes down what it would have
//! started, and [`UnavailableLauncher`] refuses with a reason. No test may hold
//! the first.

use std::collections::HashMap;
use std::time::Duration;

use flexdeck_actions::{ActionSpec, ArgumentSpec};
use flexdeck_vars::VariableSpec;

use std::sync::{Mutex, PoisonError};

use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const LAUNCHER_PLUGIN: &str = "launcher";

/// Starts things on the machine.
pub trait Launcher: Send + std::fmt::Debug {
    /// Open with whatever the system associates: a URL in the browser, a
    /// document in its app, a folder in the file manager, an app by path.
    fn open(&mut self, target: &str) -> Result<(), PluginError>;

    /// Start `program` with `arguments`, without waiting for it.
    fn run(&mut self, program: &str, arguments: &[String]) -> Result<(), PluginError>;

    /// The apps in the Start menu, for the editor's picker.
    fn apps(&mut self) -> Result<Vec<InstalledApp>, PluginError> {
        Err(PluginError::Unavailable(
            "this build cannot list installed apps".into(),
        ))
    }

    /// The programs with a window open, by executable, for the close picker.
    fn windowed_processes(&mut self) -> Result<Vec<String>, PluginError> {
        Err(PluginError::Unavailable(
            "this build cannot list running programs".into(),
        ))
    }

    /// Close every process running `exe`: ask its windows to close, wait up
    /// to `wait`, and — if `force` — end whatever is left.
    fn close(&mut self, exe: &str, force: bool, wait: Duration) -> Result<Closed, PluginError> {
        let _ = (exe, force, wait);
        Err(PluginError::Unavailable("this build cannot close programs".into()))
    }
}

/// One app the Start menu offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// What the person sees: "Spotify".
    pub name: String,
    /// What opens it: `shell:AppsFolder\…`.
    pub target: String,
}

/// How a close went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// Every window closed when asked.
    Asked,
    /// Something had to be ended by force.
    Forced,
}

/// Programs that must never be closed from a deck: ending one logs the person
/// out, blue-screens the machine, or takes the desktop away.
pub const PROTECTED: &[&str] = &[
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "lsass.exe",
    "services.exe",
    "smss.exe",
    "svchost.exe",
    "dwm.exe",
    "system",
    "registry",
    "fontdrvhost.exe",
    "sihost.exe",
];

/// The longest a close may wait for a program to agree, so a press never holds
/// up the deck for long.
pub const MAX_CLOSE_WAIT: Duration = Duration::from_secs(8);

/// The executable a close argument means, refusing the protected ones.
pub fn close_target(app: &str) -> Result<String, PluginError> {
    let exe = super::audio::exe_name(app);
    let own = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));
    if PROTECTED.contains(&exe.as_str()) || own.as_deref() == Some(exe.as_str()) {
        return Err(PluginError::Failed(format!(
            "{exe} keeps Windows (or flexdeck) running, so it cannot be closed from a deck"
        )));
    }
    Ok(exe)
}

/// Opens and runs for real, detached so a long-running program never holds up a
/// button press.
#[derive(Debug, Default)]
pub struct LiveLauncher;

impl Launcher for LiveLauncher {
    fn open(&mut self, target: &str) -> Result<(), PluginError> {
        open::that_detached(target)
            .map_err(|error| PluginError::Failed(format!("could not open '{target}': {error}")))
    }

    #[cfg(windows)]
    fn apps(&mut self) -> Result<Vec<InstalledApp>, PluginError> {
        super::launcher_windows::apps()
    }

    #[cfg(windows)]
    fn windowed_processes(&mut self) -> Result<Vec<String>, PluginError> {
        super::launcher_windows::windowed_processes()
    }

    #[cfg(windows)]
    fn close(&mut self, exe: &str, force: bool, wait: Duration) -> Result<Closed, PluginError> {
        super::launcher_windows::close(exe, force, wait)
    }

    fn run(&mut self, program: &str, arguments: &[String]) -> Result<(), PluginError> {
        std::process::Command::new(program)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|error| PluginError::Failed(format!("could not start '{program}': {error}")))
    }
}

/// Writes down what would have been started. Pretends Spotify and Notepad are
/// running, and that Word will not close without being forced (it has an
/// unsaved file).
#[derive(Debug, Default)]
pub struct RecordingLauncher {
    started: Vec<String>,
}

impl RecordingLauncher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn started(&self) -> &[String] {
        &self.started
    }
}

impl Launcher for RecordingLauncher {
    fn open(&mut self, target: &str) -> Result<(), PluginError> {
        self.started.push(format!("open {target}"));
        Ok(())
    }

    fn run(&mut self, program: &str, arguments: &[String]) -> Result<(), PluginError> {
        self.started.push(
            format!("run {program} {}", arguments.join(" "))
                .trim_end()
                .to_string(),
        );
        Ok(())
    }

    fn apps(&mut self) -> Result<Vec<InstalledApp>, PluginError> {
        Ok(vec![
            InstalledApp {
                name: "Spotify".into(),
                target: "shell:AppsFolder\\SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify".into(),
            },
            InstalledApp {
                name: "Notepad".into(),
                target: "shell:AppsFolder\\Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into(),
            },
        ])
    }

    fn windowed_processes(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(vec!["notepad.exe".into(), "spotify.exe".into(), "winword.exe".into()])
    }

    fn close(&mut self, exe: &str, force: bool, _wait: Duration) -> Result<Closed, PluginError> {
        let running = self.windowed_processes()?;
        if !running.iter().any(|known| known == exe) {
            return Err(PluginError::Failed(format!("{exe} is not running")));
        }
        self.started.push(format!("close {exe} force={force}"));
        match (exe, force) {
            ("winword.exe", false) => Err(PluginError::Failed(format!(
                "{exe} did not close (it may be asking to save); close it with force to end it"
            ))),
            ("winword.exe", true) => Ok(Closed::Forced),
            _ => Ok(Closed::Asked),
        }
    }
}

/// The honest default before a live launcher is installed.
#[derive(Debug, Default)]
pub struct UnavailableLauncher;

impl Launcher for UnavailableLauncher {
    fn open(&mut self, _target: &str) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(
            "this build cannot start programs".to_string(),
        ))
    }

    fn run(&mut self, _program: &str, _arguments: &[String]) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(
            "this build cannot start programs".to_string(),
        ))
    }
}

static LAUNCHER_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: LAUNCHER_PLUGIN,
        action: "open",
        description: "Open a website, file, folder or app",
        arguments: &[ArgumentSpec::text(
            "target",
            "A URL, or the path to a file, folder or program",
            "https://example.com",
        )],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: LAUNCHER_PLUGIN,
        action: "open_url",
        description: "Open a website",
        arguments: &[ArgumentSpec::text(
            "url",
            "The address, starting https://",
            "https://example.com",
        )],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: LAUNCHER_PLUGIN,
        action: "open_app",
        description: "Open an app from the Start menu",
        arguments: &[ArgumentSpec::text(
            "app",
            "The app; pick it from the list",
            "notepad",
        )],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: LAUNCHER_PLUGIN,
        action: "close_app",
        description: "Close a program, or end it if it will not close",
        arguments: &[
            ArgumentSpec::text("app", "The program, by its executable", "notepad.exe"),
            ArgumentSpec::choice(
                "force",
                "End it if it does not close when asked (unsaved work is lost)",
                "false",
                &["false", "true"],
            ),
            ArgumentSpec::number(
                "wait",
                "Seconds to wait for it to close before giving up or forcing",
                "3",
            )
            .optional(),
        ],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: LAUNCHER_PLUGIN,
        action: "run",
        description: "Run a command",
        arguments: &[
            ArgumentSpec::text("program", "The program to run", "notepad"),
            ArgumentSpec::text(
                "arguments",
                "Arguments, separated by spaces; quote one that contains a space",
                "\"C:\\notes.txt\"",
            )
            .optional(),
        ],
        slider: None,
        active: None,
    },
];

static LAUNCHER_MANIFEST: PluginManifest = PluginManifest {
    id: LAUNCHER_PLUGIN,
    name: "Launcher",
    version: "1.0.0",
    description: "Open websites, files, folders and apps, run a command, or close a program.",
    actions: LAUNCHER_ACTIONS,
    settings: &[],
};

pub struct LauncherPlugin {
    /// Behind a lock so `choices` (which only gets `&self`) can list apps.
    backend: Mutex<Box<dyn Launcher>>,
}

impl std::fmt::Debug for LauncherPlugin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LauncherPlugin")
            .finish_non_exhaustive()
    }
}

impl LauncherPlugin {
    pub fn new(backend: Box<dyn Launcher>) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }
}

impl Plugin for LauncherPlugin {
    fn manifest(&self) -> crate::abi::ManifestDto {
        LAUNCHER_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        Vec::new()
    }

    fn check(&self, action: &str, args: &HashMap<String, String>) -> Result<(), PluginError> {
        match action {
            "run" => {
                split_arguments(argument(args, "arguments"))?;
            }
            "open_url" => {
                web_address(argument(args, "url"))?;
            }
            "close_app" => {
                close_target(argument(args, "app"))?;
                wait_of(args)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn choices(
        &self,
        action: &str,
        _args: &HashMap<String, String>,
    ) -> Result<crate::abi::ChoicesResponse, PluginError> {
        let mut answer = crate::abi::ChoicesResponse::default();
        let mut backend = self.backend.lock().unwrap_or_else(PoisonError::into_inner);
        let found = match action {
            "open_app" => backend.apps().map(|apps| {
                apps.into_iter()
                    .map(|app| crate::abi::ChoiceDto {
                        value: app.target,
                        label: Some(app.name),
                    })
                    .collect()
            }),
            "close_app" => backend.windowed_processes().map(|names| {
                names
                    .into_iter()
                    .filter(|name| close_target(name).is_ok())
                    .map(|value| crate::abi::ChoiceDto { value, label: None })
                    .collect()
            }),
            _ => return Ok(answer),
        };
        match found {
            Ok(choices) => {
                answer.arguments.insert("app".into(), choices);
            }
            Err(error) => answer.error = Some(error.to_string()),
        }
        Ok(answer)
    }

    fn sample(&mut self, _wanted: &[&str], _ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        _ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        let mut backend = self.backend.lock().unwrap_or_else(PoisonError::into_inner);
        match action {
            "open" => backend.open(argument(args, "target").trim())?,
            "open_url" => backend.open(&web_address(argument(args, "url"))?)?,
            // A picked `shell:AppsFolder…` target, a path, or a bare name
            // like `notepad`, which Windows resolves the way Run (Win+R) does.
            "open_app" => backend.open(argument(args, "app").trim())?,
            "run" => {
                let arguments = split_arguments(argument(args, "arguments"))?;
                backend.run(argument(args, "program").trim(), &arguments)?;
            }
            "close_app" => {
                let exe = close_target(argument(args, "app"))?;
                let force = argument(args, "force") == "true";
                let closed = backend.close(&exe, force, wait_of(args)?)?;
                return Ok(Outcome::state(match closed {
                    Closed::Asked => "closed",
                    Closed::Forced => "forced",
                }));
            }
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: LAUNCHER_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        }
        Ok(Outcome::default())
    }
}

/// An http(s) address; one typed without a scheme gets `https://`. Anything
/// else — `file:`, a protocol handler — belongs to `open`, which says so.
fn web_address(raw: &str) -> Result<String, PluginError> {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    let url = if lower.starts_with("https://") || lower.starts_with("http://") {
        trimmed.to_string()
    } else if !trimmed.contains("://") && !trimmed.contains(char::is_whitespace) && trimmed.contains('.') {
        format!("https://{trimmed}")
    } else {
        return Err(PluginError::Failed(format!(
            "'{trimmed}' is not a web address (use the Open action for files and other links)"
        )));
    };
    if url.contains(char::is_whitespace) {
        return Err(PluginError::Failed(format!("'{url}' has a space in it")));
    }
    Ok(url)
}

/// The `wait` argument, as a duration within bounds.
fn wait_of(args: &HashMap<String, String>) -> Result<Duration, PluginError> {
    let raw = argument(args, "wait").trim();
    if raw.is_empty() {
        return Ok(Duration::from_secs(3));
    }
    let seconds: f64 = raw
        .parse()
        .map_err(|_| PluginError::Failed(format!("'wait' must be a number of seconds, not '{raw}'")))?;
    if !(0.0..=MAX_CLOSE_WAIT.as_secs_f64()).contains(&seconds) {
        return Err(PluginError::Failed(format!(
            "'wait' must be between 0 and {} seconds",
            MAX_CLOSE_WAIT.as_secs()
        )));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// Split on spaces, keeping a double-quoted run together.
///
/// Deliberately not a shell: no variables, no globbing, no pipes. The program
/// is started directly, so a stray `&` is an argument rather than a second
/// command.
fn split_arguments(text: &str) -> Result<Vec<String>, PluginError> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for character in text.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            ' ' | '\t' if !quoted => {
                if started {
                    arguments.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            other => {
                current.push(other);
                started = true;
            }
        }
    }
    if quoted {
        return Err(PluginError::Failed(
            "the arguments have an unmatched quote".to_string(),
        ));
    }
    if started {
        arguments.push(current);
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    fn run(plugin: &mut LauncherPlugin, action: &str, pairs: &[(&str, &str)]) -> Result<Outcome, PluginError> {
        let args: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        plugin.check(action, &args)?;
        let vars = VariableStore::new();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx {
            input: &mut input,
            vars: &vars,
            notifier: &mut notifier,
            settings: &settings,
            now_ms: 0,
        };
        plugin.invoke(action, &args, &mut ctx)
    }

    #[test]
    fn a_website_is_opened_only_as_a_web_address() {
        let mut plugin = LauncherPlugin::new(Box::new(RecordingLauncher::new()));
        run(&mut plugin, "open_url", &[("url", "example.com/a")]).unwrap();
        run(&mut plugin, "open_url", &[("url", "HTTP://example.com")]).unwrap();
        for bad in ["file:///C:/x.exe", "ms-settings:privacy", "not a site"] {
            assert!(run(&mut plugin, "open_url", &[("url", bad)]).is_err(), "{bad}");
        }
    }

    #[test]
    fn closing_asks_first_and_forces_only_when_told() {
        let mut plugin = LauncherPlugin::new(Box::new(RecordingLauncher::new()));
        let closed = run(&mut plugin, "close_app", &[("app", "Spotify"), ("force", "false")]).unwrap();
        assert_eq!(closed.state.as_deref(), Some("closed"));
        let error = run(&mut plugin, "close_app", &[("app", "winword.exe"), ("force", "false")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("with force"), "{error}");
        let forced = run(&mut plugin, "close_app", &[("app", "winword"), ("force", "true")]).unwrap();
        assert_eq!(forced.state.as_deref(), Some("forced"));
        assert!(run(&mut plugin, "close_app", &[("app", "winamp"), ("force", "true")]).is_err());
    }

    #[test]
    fn what_keeps_windows_running_cannot_be_closed() {
        let mut plugin = LauncherPlugin::new(Box::new(RecordingLauncher::new()));
        for protected in ["lsass", "csrss.exe", "SVCHOST.EXE", "dwm"] {
            let error = run(&mut plugin, "close_app", &[("app", protected), ("force", "true")])
                .unwrap_err()
                .to_string();
            assert!(error.contains("cannot be closed"), "{protected}: {error}");
        }
        assert!(run(&mut plugin, "close_app", &[("app", "spotify"), ("force", "false"), ("wait", "60")]).is_err());
    }

    #[test]
    fn the_pickers_offer_start_menu_apps_and_open_windows() {
        let plugin = LauncherPlugin::new(Box::new(RecordingLauncher::new()));
        let apps = plugin.choices("open_app", &HashMap::new()).unwrap();
        assert_eq!(apps.arguments["app"][0].label.as_deref(), Some("Spotify"));
        assert!(apps.arguments["app"][0].value.starts_with("shell:AppsFolder"));
        let running = plugin.choices("close_app", &HashMap::new()).unwrap();
        assert_eq!(running.arguments["app"].len(), 3);
    }

    #[test]
    fn arguments_split_on_spaces_and_keep_quoted_runs_together() {
        assert_eq!(
            split_arguments(r#"-a "two words" ""  x"#).unwrap(),
            vec!["-a", "two words", "", "x"]
        );
        assert!(split_arguments("").unwrap().is_empty());
        assert!(split_arguments(r#""open"#).is_err());
    }
}

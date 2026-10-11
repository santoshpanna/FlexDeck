//! The soundboard as a flexdeck plugin: its manifest, its actions, and the
//! variables and events that show what is playing.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use flexdeck_plugin_sdk::{abi, failed, ok, ok_state, value, Host, Sidecar, Start};
use sha2::Digest as _;

use crate::engine::{find_cable, Config, Engine, Mode, Output, NO_CABLE};
use crate::mix::Clip;

const ID: &str = "soundboard";
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Decoded clips kept in memory, in bytes.
const CLIP_BUDGET: usize = 64 * 1024 * 1024;
/// The largest file read or downloaded.
const MAX_FILE: u64 = 25 * 1024 * 1024;
/// How many recently played clips the editor offers.
const RECENT: usize = 30;

pub struct Soundboard {
    engine: Arc<Engine>,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    settings: BTreeMap<String, String>,
    data_dir: Option<PathBuf>,
    clips: HashMap<String, Arc<Clip>>,
    /// Oldest first, for evicting.
    order: Vec<String>,
    recent: Vec<String>,
    gains: HashMap<String, f32>,
    declared: BTreeSet<String>,
    passthrough: bool,
    mic_gain: f32,
    master: f32,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn argument(name: &str, kind: &str, description: &str, example: &str) -> abi::ArgumentDto {
    abi::ArgumentDto {
        name: name.into(),
        description: description.into(),
        example: example.into(),
        kind: kind.into(),
        choices: vec![],
        optional: false,
    }
}

fn choice(name: &str, description: &str, example: &str, choices: &[&str]) -> abi::ArgumentDto {
    abi::ArgumentDto {
        choices: choices.iter().map(|c| c.to_string()).collect(),
        ..argument(name, abi::kind::CHOICE, description, example)
    }
}

fn optional(mut argument: abi::ArgumentDto) -> abi::ArgumentDto {
    argument.optional = true;
    argument
}

fn action(name: &str, description: &str, arguments: Vec<abi::ArgumentDto>) -> abi::ActionDto {
    abi::ActionDto {
        action: name.into(),
        description: description.into(),
        arguments,
        slider: None,
        active: None,
    }
}

fn slider(mut action: abi::ActionDto, argument: &str, max: i64, feedback: &str, abi: u32) -> abi::ActionDto {
    if abi >= 3 {
        action.slider = Some(abi::SliderDto {
            argument: argument.into(),
            min: 0,
            max,
            step: 1,
            feedback: Some(feedback.into()),
            mute: None,
        });
    }
    action
}

fn setting(key: &str, description: &str, default: &str) -> abi::SettingDto {
    abi::SettingDto {
        key: key.into(),
        description: description.into(),
        kind: abi::kind::TEXT.into(),
        choices: vec![],
        default_value: Some(default.into()),
        secret: false,
    }
}

fn variable(name: &str, kind: &str, description: &str) -> abi::VariableDto {
    abi::VariableDto {
        name: name.into(),
        description: description.into(),
        kind: kind.into(),
        unit: None,
        choices: vec![],
        read_only: true,
    }
}

fn clip_argument() -> abi::ArgumentDto {
    argument(
        "clip",
        abi::kind::TEXT,
        "A sound file's path, or an https:// address",
        "C:\\Sounds\\airhorn.mp3",
    )
}

/// A percentage argument as a gain: `percent / 100`, within `0..=max`.
fn gain(raw: &str, max: f64) -> Result<f32, String> {
    if raw.trim().is_empty() {
        return Ok(1.0);
    }
    let value: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("'{raw}' is not a number"))?;
    if !(0.0..=max).contains(&value) {
        return Err(format!("the volume must be 0 to {max}"));
    }
    Ok((value / 100.0) as f32)
}

fn arg<'a>(request: &'a abi::InvokeRequest, name: &str) -> &'a str {
    request.args.get(name).map(String::as_str).unwrap_or("").trim()
}

impl Soundboard {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine: Arc::new(engine),
            state: Arc::new(Mutex::new(State {
                mic_gain: 1.0,
                master: 1.0,
                ..State::default()
            })),
        }
    }

    fn config(settings: &BTreeMap<String, String>) -> Config {
        let device = |key: &str| {
            settings
                .get(key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("default") && !value.eq_ignore_ascii_case("auto"))
        };
        Config {
            monitor: device("monitor_device"),
            cable: device("cable_device"),
            mic: device("mic_device"),
            passthrough: settings.get("mic_passthrough").map(String::as_str) == Some("true"),
        }
    }

    /// Open the devices the settings name, and say what happened.
    fn apply(&self, host: &Host) {
        let (config, master) = {
            let state = lock(&self.state);
            let master = state
                .settings
                .get("master_volume")
                .and_then(|value| gain(value, 100.0).ok())
                .unwrap_or(1.0);
            (Self::config(&state.settings), master)
        };
        let opened = self.engine.configure(config.clone());
        self.engine.set_master(master);
        {
            let mut state = lock(&self.state);
            state.passthrough = opened.mic.is_some();
            state.master = master;
        }
        for problem in &opened.problems {
            host.log("warn", problem);
        }
        host.vars_set(BTreeMap::from([
            ("soundboard.cable.present".to_string(), value::boolean(opened.cable.is_some())),
            ("soundboard.cable.device".to_string(), value::text(opened.cable.clone().unwrap_or_default())),
            ("soundboard.mic_passthrough".to_string(), value::boolean(opened.mic.is_some())),
            ("soundboard.master_volume".to_string(), value::number((f64::from(master) * 100.0).round())),
        ]));
        host.log(
            "info",
            &format!(
                "listening on {}, cable {}, microphone {}",
                opened.monitor.as_deref().unwrap_or("nothing"),
                opened.cable.as_deref().unwrap_or("none"),
                opened.mic.as_deref().unwrap_or("not mixed in"),
            ),
        );
    }

    /// The clip `source` names, decoded once and kept within the budget.
    fn load(&self, source: &str) -> Result<Arc<Clip>, String> {
        if let Some(clip) = lock(&self.state).clips.get(source) {
            return Ok(Arc::clone(clip));
        }
        let data_dir = lock(&self.state).data_dir.clone();
        let bytes = if source.starts_with("https://") || source.starts_with("http://") {
            download(source, data_dir.as_deref())?
        } else {
            read_file(source)?
        };
        let clip = Arc::new(crate::decode::decode(bytes, source)?);
        let mut state = lock(&self.state);
        state.clips.insert(source.to_string(), Arc::clone(&clip));
        state.order.push(source.to_string());
        while state.clips.values().map(|clip| clip.size()).sum::<usize>() > CLIP_BUDGET && state.order.len() > 1 {
            let oldest = state.order.remove(0);
            state.clips.remove(&oldest);
        }
        Ok(clip)
    }

    fn remember(&self, source: &str) {
        let mut state = lock(&self.state);
        state.recent.retain(|known| known != source);
        state.recent.insert(0, source.to_string());
        state.recent.truncate(RECENT);
        if let Some(dir) = &state.data_dir {
            let _ = std::fs::write(dir.join("recent.json"), serde_json::to_string(&state.recent).unwrap_or_default());
        }
    }

    /// Declare a clip's variables the first time it is used.
    fn declare(&self, host: &Host, clip: &str) {
        let mut state = lock(&self.state);
        if !state.declared.insert(clip.to_string()) {
            return;
        }
        let variables = state
            .declared
            .iter()
            .flat_map(|name| {
                let mut volume = variable(&format!("soundboard.clip.{name}.volume"), abi::kind::NUMBER, "That clip's volume");
                volume.unit = Some("%".into());
                [
                    variable(&format!("soundboard.clip.{name}.playing"), abi::kind::BOOL, "Whether that clip is playing"),
                    volume,
                ]
            })
            .collect();
        drop(state);
        host.vars_declare(variables, false);
    }
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{path} could not be opened: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{path} could not be read: {error}"))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("{path} is over {} MB", MAX_FILE / 1024 / 1024));
    }
    Ok(bytes)
}

/// Fetch a sound once and keep it in the plugin's folder.
fn download(url: &str, data_dir: Option<&std::path::Path>) -> Result<Vec<u8>, String> {
    let name = hex::encode(sha2::Sha256::digest(url.as_bytes()));
    let cached = data_dir.map(|dir| dir.join("cache").join(&name));
    if let Some(path) = &cached {
        if let Ok(bytes) = std::fs::read(path) {
            return Ok(bytes);
        }
    }
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let response = agent
        .get(url)
        .call()
        .map_err(|error| format!("{url} could not be downloaded: {error}"))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{url} could not be downloaded: {error}"))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("{url} is over {} MB", MAX_FILE / 1024 / 1024));
    }
    if let Some(path) = cached {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &bytes);
    }
    Ok(bytes)
}

impl Sidecar for Soundboard {
    fn manifest(&self, abi: u32) -> abi::ManifestDto {
        let mut manifest = flexdeck_plugin_sdk::manifest(abi, ID, "Soundboard", VERSION);
        manifest.description =
            "Play sound clips to your headphones and, through a virtual audio cable, into your microphone.".into();
        manifest.author = "flexdeck".into();
        manifest.permissions = vec![abi::permission::FILESYSTEM.into(), abi::permission::NETWORK.into()];
        manifest.settings = vec![
            setting("monitor_device", "Where you hear the clips (blank: the default output)", ""),
            setting("cable_device", "The virtual cable's input (blank: found automatically)", ""),
            setting("mic_device", "Your real microphone, for passthrough (blank: the default)", ""),
            abi::SettingDto {
                kind: abi::kind::CHOICE.into(),
                choices: vec!["false".into(), "true".into()],
                ..setting("mic_passthrough", "Mix your microphone into the cable", "false")
            },
            setting("master_volume", "The volume of every clip, 0 to 100", "100"),
        ];
        manifest.events = vec![
            abi::EventDto {
                name: "clip_started".into(),
                description: "A clip started playing".into(),
                fields: vec!["clip".into()],
            },
            abi::EventDto {
                name: "clip_finished".into(),
                description: "A clip finished or was stopped".into(),
                fields: vec!["clip".into()],
            },
        ];
        manifest.actions = vec![
            action(
                "play",
                "Play a clip",
                vec![
                    clip_argument(),
                    optional(argument("volume", abi::kind::NUMBER, "Its volume, 0 to 100", "100")),
                    choice(
                        "mode",
                        "If it is already playing: start again, play over it, or stop it",
                        "restart",
                        &["restart", "overlap", "toggle"],
                    ),
                    choice("output", "Where it plays", "both", &["both", "monitor", "cable"]),
                ],
            ),
            action("stop", "Stop a clip", vec![clip_argument()]),
            action("stop_all", "Stop every clip", vec![]),
            slider(
                action(
                    "set_master_volume",
                    "Set the volume of every clip",
                    vec![argument("volume", abi::kind::NUMBER, "0 to 100", "100")],
                ),
                "volume",
                100,
                "soundboard.master_volume",
                abi,
            ),
            slider(
                action(
                    "set_clip_volume",
                    "Set one clip's volume, even while it plays",
                    vec![clip_argument(), argument("volume", abi::kind::NUMBER, "0 to 100", "100")],
                ),
                "volume",
                100,
                "soundboard.clip.{{clip}}.volume",
                abi,
            ),
            action(
                "set_mic_passthrough",
                "Mix your microphone into the cable, or stop",
                vec![choice("enabled", "On, off, or the other way", "toggle", &["true", "false", "toggle"])],
            ),
            slider(
                action(
                    "set_mic_gain",
                    "Set the microphone's volume in the mix",
                    vec![argument("volume", abi::kind::NUMBER, "0 to 200", "100")],
                ),
                "volume",
                200,
                "soundboard.mic_gain",
                abi,
            ),
        ];
        manifest
    }

    fn variables(&self) -> Vec<abi::VariableDto> {
        let mut volume = variable("soundboard.master_volume", abi::kind::NUMBER, "The volume of every clip");
        volume.unit = Some("%".into());
        let mut mic = variable("soundboard.mic_gain", abi::kind::NUMBER, "The microphone's volume in the mix");
        mic.unit = Some("%".into());
        vec![
            variable("soundboard.playing", abi::kind::BOOL, "Whether any clip is playing"),
            variable("soundboard.playing_count", abi::kind::NUMBER, "How many clips are playing"),
            volume,
            mic,
            variable("soundboard.mic_passthrough", abi::kind::BOOL, "Whether the microphone is mixed into the cable"),
            variable("soundboard.cable.present", abi::kind::BOOL, "Whether a virtual audio cable is connected"),
            variable("soundboard.cable.device", abi::kind::TEXT, "The cable being used"),
        ]
    }

    fn start(&self, start: &Start, host: &Host) {
        {
            let mut state = lock(&self.state);
            state.settings = start.settings.clone();
            state.data_dir = start.data_dir.clone();
            if let Some(dir) = &state.data_dir {
                if let Ok(text) = std::fs::read_to_string(dir.join("recent.json")) {
                    state.recent = serde_json::from_str(&text).unwrap_or_default();
                }
            }
        }
        self.apply(host);
        host.var("soundboard.mic_gain", value::number(100.0));

        // Watch what plays, and whether a cable comes or goes.
        let engine = Arc::clone(&self.engine);
        let state = Arc::clone(&self.state);
        let host = host.clone();
        std::thread::spawn(move || {
            let mut before: Vec<String> = Vec::new();
            let mut cable = engine.opened().cable;
            for tick in 0u64.. {
                std::thread::sleep(Duration::from_millis(100));
                let now = engine.playing();
                if now != before {
                    let mut values = BTreeMap::new();
                    for clip in now.iter().filter(|clip| !before.contains(clip)) {
                        host.emit("clip_started", BTreeMap::from([("clip".into(), clip.clone())]));
                        values.insert(format!("soundboard.clip.{clip}.playing"), value::boolean(true));
                    }
                    for clip in before.iter().filter(|clip| !now.contains(clip)) {
                        host.emit("clip_finished", BTreeMap::from([("clip".into(), clip.clone())]));
                        values.insert(format!("soundboard.clip.{clip}.playing"), value::boolean(false));
                    }
                    values.insert("soundboard.playing".into(), value::boolean(!now.is_empty()));
                    values.insert("soundboard.playing_count".into(), value::number(now.len() as f64));
                    host.vars_set(values);
                    before = now;
                }
                // Every five seconds: a cable plugged in or removed is picked
                // up when the cable is found automatically.
                if tick % 50 == 49 {
                    let auto = Soundboard::config(&lock(&state).settings).cable.is_none();
                    let found = find_cable(&engine.devices().0);
                    if auto && found != cable {
                        host.log(
                            "info",
                            &match &found {
                                Some(name) => format!("found a virtual cable: {name}"),
                                None => "the virtual cable went away".into(),
                            },
                        );
                        let config = Soundboard::config(&lock(&state).settings);
                        let opened = engine.configure(config);
                        lock(&state).passthrough = opened.mic.is_some();
                        host.vars_set(BTreeMap::from([
                            ("soundboard.cable.present".to_string(), value::boolean(opened.cable.is_some())),
                            ("soundboard.cable.device".to_string(), value::text(opened.cable.clone().unwrap_or_default())),
                            ("soundboard.mic_passthrough".to_string(), value::boolean(opened.mic.is_some())),
                        ]));
                        cable = opened.cable;
                    }
                }
            }
        });
    }

    fn check(&self, request: &abi::CheckRequest, _host: &Host) -> abi::CheckResponse {
        let refuse = |error: String| abi::CheckResponse {
            ok: false,
            error: Some(error),
        };
        let get = |name: &str| request.args.get(name).map(String::as_str).unwrap_or("");
        match request.action.as_str() {
            "play" => {
                if let Err(error) = gain(get("volume"), 100.0) {
                    return refuse(error);
                }
                let clip = get("clip").trim();
                let remote = clip.starts_with("http://") || clip.starts_with("https://");
                if !remote && !std::path::Path::new(clip).is_file() {
                    return refuse(format!("there is no file at {clip}"));
                }
            }
            "set_master_volume" | "set_clip_volume" => {
                if let Err(error) = gain(get("volume"), 100.0) {
                    return refuse(error);
                }
            }
            "set_mic_gain" => {
                if let Err(error) = gain(get("volume"), 200.0) {
                    return refuse(error);
                }
            }
            _ => {}
        }
        abi::CheckResponse { ok: true, error: None }
    }

    fn choices(&self, request: &abi::ChoicesRequest, _host: &Host) -> abi::ChoicesResponse {
        let mut answer = abi::ChoicesResponse::default();
        if request.action == "play" || request.action == "stop" || request.action == "set_clip_volume" {
            let recent = lock(&self.state).recent.clone();
            answer.arguments.insert(
                "clip".into(),
                recent
                    .into_iter()
                    .map(|value| abi::ChoiceDto {
                        label: std::path::Path::new(&value)
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned()),
                        value,
                    })
                    .collect(),
            );
        }
        answer
    }

    fn invoke(&self, request: &abi::InvokeRequest, host: &Host) -> abi::InvokeResponse {
        let result: Result<Option<&str>, String> = (|| match request.action.as_str() {
            "play" => {
                let source = arg(request, "clip");
                let mode = Mode::parse(arg(request, "mode")).ok_or("mode must be restart, overlap or toggle")?;
                let output = Output::parse(arg(request, "output")).ok_or("output must be both, monitor or cable")?;
                let clip = self.load(source)?;
                let level = match arg(request, "volume") {
                    "" => lock(&self.state).gains.get(source).copied().unwrap_or(1.0),
                    raw => gain(raw, 100.0)?,
                };
                lock(&self.state).gains.insert(source.to_string(), level);
                self.declare(host, source);
                self.remember(source);
                let playing = self.engine.play(source, &clip, level, mode, output)?;
                Ok(Some(if playing { "playing" } else { "stopped" }))
            }
            "stop" => {
                self.engine.stop(arg(request, "clip"));
                Ok(Some("stopped"))
            }
            "stop_all" => {
                self.engine.stop_all();
                Ok(None)
            }
            "set_master_volume" => {
                let level = gain(arg(request, "volume"), 100.0)?;
                self.engine.set_master(level);
                lock(&self.state).master = level;
                host.var("soundboard.master_volume", value::number((f64::from(level) * 100.0).round()));
                Ok(None)
            }
            "set_clip_volume" => {
                let source = arg(request, "clip");
                let level = gain(arg(request, "volume"), 100.0)?;
                self.engine.set_clip_gain(source, level);
                lock(&self.state).gains.insert(source.to_string(), level);
                self.declare(host, source);
                host.var(
                    &format!("soundboard.clip.{source}.volume"),
                    value::number((f64::from(level) * 100.0).round()),
                );
                Ok(None)
            }
            "set_mic_passthrough" => {
                let current = lock(&self.state).passthrough;
                let wanted = match arg(request, "enabled") {
                    "true" => true,
                    "false" => false,
                    _ => !current,
                };
                if wanted && self.engine.opened().cable.is_none() {
                    return Err(NO_CABLE.to_string());
                }
                lock(&self.state)
                    .settings
                    .insert("mic_passthrough".into(), wanted.to_string());
                // Kept, so it survives a restart, and reopened now.
                host.store_settings(BTreeMap::from([("mic_passthrough".to_string(), wanted.to_string())]));
                self.apply(host);
                let on = lock(&self.state).passthrough;
                if wanted && !on {
                    return Err("the microphone could not be opened; see the plugin's log".into());
                }
                Ok(Some(if on { "on" } else { "off" }))
            }
            "set_mic_gain" => {
                let level = gain(arg(request, "volume"), 200.0)?;
                self.engine.set_mic_gain(level);
                lock(&self.state).mic_gain = level;
                host.var("soundboard.mic_gain", value::number((f64::from(level) * 100.0).round()));
                Ok(None)
            }
            other => Err(format!("no action called '{other}'")),
        })();
        match result {
            Ok(Some(state)) => ok_state(state),
            Ok(None) => ok(),
            Err(error) => failed(error),
        }
    }

    fn settings_changed(&self, settings: &BTreeMap<String, String>, host: &Host) {
        lock(&self.state).settings = settings.clone();
        self.apply(host);
    }

    fn shutdown(&self, _host: &Host) {
        self.engine.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::NullDevices;

    fn board(outputs: &[&str]) -> Soundboard {
        let outputs: Vec<String> = outputs.iter().map(|name| name.to_string()).collect();
        Soundboard::new(Engine::new(move || {
            Box::new(NullDevices {
                outputs,
                inputs: vec!["Microphone".into()],
                ..NullDevices::default()
            })
        }))
    }

    /// A host that writes nowhere, for calling the plugin directly.
    fn host() -> Host {
        flexdeck_plugin_sdk::test_host()
    }

    fn invoke(board: &Soundboard, action: &str, args: &[(&str, &str)]) -> abi::InvokeResponse {
        board.invoke(
            &abi::InvokeRequest {
                action: action.into(),
                args: args.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            },
            &host(),
        )
    }

    fn wav_file() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("beep.wav");
        std::fs::write(&path, crate::decode::tests::wav(48_000, &[8_000; 4_800])).unwrap();
        (dir, path.to_string_lossy().into_owned())
    }

    #[test]
    fn the_manifest_is_valid_at_both_abis_and_offers_sliders_only_at_3() {
        let board = board(&["Speakers"]);
        board.manifest(3).validate().expect("valid at 3");
        board.manifest(2).validate().expect("valid at 2");
        assert!(board.manifest(3).actions.iter().any(|a| a.slider.is_some()));
        assert!(board.manifest(2).actions.iter().all(|a| a.slider.is_none()));
    }

    #[test]
    fn a_clip_plays_reports_its_look_and_toggles_off() {
        let board = board(&["Speakers"]);
        board.apply(&host());
        let (_dir, path) = wav_file();
        let played = invoke(&board, "play", &[("clip", &path), ("mode", "toggle"), ("output", "both")]);
        assert_eq!(played.state.as_deref(), Some("playing"), "{:?}", played.error);
        assert_eq!(board.engine.playing(), [path.clone()]);
        let again = invoke(&board, "play", &[("clip", &path), ("mode", "toggle"), ("output", "both")]);
        assert_eq!(again.state.as_deref(), Some("stopped"));
        assert!(board.engine.playing().is_empty());
        assert_eq!(lock(&board.state).recent, [path], "offered next time");
    }

    #[test]
    fn the_cable_is_explained_when_it_is_missing() {
        let board = board(&["Speakers"]);
        board.apply(&host());
        let (_dir, path) = wav_file();
        let refused = invoke(&board, "play", &[("clip", &path), ("mode", "restart"), ("output", "cable")]);
        assert!(refused.error.unwrap().contains("VB-CABLE"));
        let passthrough = invoke(&board, "set_mic_passthrough", &[("enabled", "true")]);
        assert!(passthrough.error.unwrap().contains("VB-CABLE"));
    }

    #[test]
    fn passthrough_needs_a_cable_and_then_opens_the_microphone() {
        let board = board(&["Speakers", "CABLE Input (VB-Audio Virtual Cable)"]);
        board.apply(&host());
        let on = invoke(&board, "set_mic_passthrough", &[("enabled", "toggle")]);
        assert_eq!(on.state.as_deref(), Some("on"), "{:?}", on.error);
        let off = invoke(&board, "set_mic_passthrough", &[("enabled", "toggle")]);
        assert_eq!(off.state.as_deref(), Some("off"));
    }

    #[test]
    fn bad_volumes_and_missing_files_are_refused_before_anything_plays() {
        let board = board(&["Speakers"]);
        let check = |action: &str, args: &[(&str, &str)]| {
            board.check(
                &abi::CheckRequest {
                    action: action.into(),
                    args: args.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                },
                &host(),
            )
        };
        assert!(!check("play", &[("clip", "C:/nope/none.mp3")]).ok);
        assert!(!check("set_master_volume", &[("volume", "150")]).ok);
        assert!(check("set_mic_gain", &[("volume", "150")]).ok, "the mic goes to 200");
        assert!(check("play", &[("clip", "https://example.com/a.mp3")]).ok, "an address is fetched on press");
    }
}

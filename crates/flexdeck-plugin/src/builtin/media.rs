//! The `media` plugin: play, pause, skip, and the volume keys — and, per
//! player, what is playing and its own controls.
//!
//! These are the keyboard's own media keys, sent through the same input
//! executor `core` uses, so they reach whichever player the operating system
//! routes media keys to — Spotify, a browser tab, a video player — without this
//! plugin knowing which. That is also why it needs no backend of its own: a
//! test sees exactly which key was pressed through `RecordingExecutor`.
//!
//! The `session_*` actions and the `media.<app>.*` variables go further:
//! through Windows' media sessions (see `media_sessions.rs`) they reach one
//! named player — `spotify`, `tidal`, `apple music`, or `current` — and read
//! back its title, artist, position and album art for the deck to show.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use flexdeck_actions::{ActionSpec, ActiveSpec, ArgumentSpec, CoreAction, Hotkey, KeyName, SliderSpec};
use flexdeck_vars::{Kind, Value, VariableSpec};

use super::media_sessions::{Command, ImageSink, MediaSessions};
use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const MEDIA_PLUGIN: &str = "media";

/// Each action is one key. Kept as one table so the catalogue and the dispatch
/// cannot disagree about which actions exist.
const KEYS: &[(&str, &str, KeyName)] = &[
    ("play_pause", "Play or pause", KeyName::MediaPlayPause),
    ("next", "Next track", KeyName::MediaNext),
    ("previous", "Previous track", KeyName::MediaPrevious),
    ("stop", "Stop playback", KeyName::MediaStop),
    ("volume_up", "Volume up one step", KeyName::VolumeUp),
    ("volume_down", "Volume down one step", KeyName::VolumeDown),
    ("mute", "Mute or unmute", KeyName::VolumeMute),
];

static MEDIA_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "play_pause",
        description: "Play or pause",
        arguments: &[],
        slider: None,
        active: Some(ActiveSpec::while_true("media.current.playing")),
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "next",
        description: "Next track",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "previous",
        description: "Previous track",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "stop",
        description: "Stop playback",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "volume_up",
        description: "Volume up one step",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "volume_down",
        description: "Volume down one step",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "mute",
        description: "Mute or unmute",
        arguments: &[],
        slider: None,
        active: Some(ActiveSpec::while_true(super::system::MUTED)),
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "session_play_pause",
        description: "Play or pause one player",
        arguments: &[APP],
        slider: None,
        active: Some(ActiveSpec::while_true("media.{{app}}.playing")),
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "session_next",
        description: "Skip to one player's next track",
        arguments: &[APP],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "session_previous",
        description: "Go to one player's previous track",
        arguments: &[APP],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "session_stop",
        description: "Stop one player",
        arguments: &[APP],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "seek",
        description: "Jump to a point in the track",
        arguments: &[APP, ArgumentSpec::number("seconds", "Seconds from the start", "60")],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "seek_by",
        description: "Jump forward or back",
        arguments: &[APP, ArgumentSpec::number("seconds", "Seconds; negative goes back", "10")],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "seek_percent",
        description: "Jump to a point in the track, as a percentage",
        arguments: &[APP, ArgumentSpec::number("percent", "0 to 100", "50")],
        slider: Some(SliderSpec::percent("percent", "media.{{app}}.progress")),
        active: None,
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "toggle_shuffle",
        description: "Turn one player's shuffle on or off",
        arguments: &[APP],
        slider: None,
        active: Some(ActiveSpec::while_true("media.{{app}}.shuffle")),
    },
    ActionSpec {
        plugin: MEDIA_PLUGIN,
        action: "cycle_repeat",
        description: "Change one player's repeat: off, all, one",
        arguments: &[APP],
        slider: None,
        active: None,
    },
];

const APP: ArgumentSpec = ArgumentSpec::text(
    "app",
    "The player: spotify, tidal, apple music, or current",
    "current",
);

/// What `media.<app>.<field>` can be.
const FIELDS: &[(&str, Kind, &str)] = &[
    ("title", Kind::Text, "The track's title"),
    ("artist", Kind::Text, "The artist"),
    ("album", Kind::Text, "The album"),
    ("playing", Kind::Bool, "Whether it is playing"),
    ("shuffle", Kind::Bool, "Whether shuffle is on"),
    ("repeat", Kind::Text, "Repeat: none, list or track"),
    ("position", Kind::Number, "Seconds into the track"),
    ("duration", Kind::Number, "The track's length in seconds"),
    ("progress", Kind::Number, "How far through the track, in percent"),
    ("art", Kind::Text, "The album art, for a widget's icon"),
];

static MEDIA_MANIFEST: PluginManifest = PluginManifest {
    id: MEDIA_PLUGIN,
    name: "Media",
    version: "1.0.0",
    description: "Play, pause and skip — any player, or Spotify, TIDAL or Apple Music by name — and show what is playing.",
    actions: MEDIA_ACTIONS,
    settings: &[],
};

pub struct Media {
    sessions: Mutex<Box<dyn MediaSessions>>,
    images: Option<Arc<dyn ImageSink>>,
    /// The track each player's art was last fetched for, and its image.
    art: Mutex<HashMap<String, (String, String)>>,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Media").finish_non_exhaustive()
    }
}

impl Media {
    pub fn new(sessions: Box<dyn MediaSessions>, images: Option<Arc<dyn ImageSink>>) -> Self {
        Self {
            sessions: Mutex::new(sessions),
            images,
            art: Mutex::new(HashMap::new()),
        }
    }

    fn sessions(&self) -> MutexGuard<'_, Box<dyn MediaSessions>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read `app`'s player and publish every `media.<app>.*` variable.
    fn publish(&self, ctx: &mut Ctx<'_>, app: &str) -> Result<(), PluginError> {
        let now = self.sessions().now_playing(app)?;
        let set = |field: &str, value: Value| -> Result<(), PluginError> {
            let name = format!("media.{app}.{field}");
            if ctx.vars.spec(&name).is_none() {
                let (_, kind, description) = FIELDS
                    .iter()
                    .find(|(known, _, _)| *known == field)
                    .expect("a known field");
                ctx.vars.declare(VariableSpec::new(&name, *kind, *description).read_only());
            }
            ctx.vars.refresh(&name, value)?;
            Ok(())
        };
        let Some(now) = now else {
            // Nothing playing there: clear the text, and say it is not playing.
            set("playing", Value::Bool(false))?;
            for field in ["title", "artist", "album", "art"] {
                set(field, Value::Text(String::new()))?;
            }
            return Ok(());
        };
        set("title", Value::Text(now.title.clone()))?;
        set("artist", Value::Text(now.artist.clone()))?;
        set("album", Value::Text(now.album.clone()))?;
        set("playing", Value::Bool(now.playing))?;
        set("shuffle", Value::Bool(now.shuffle.unwrap_or(false)))?;
        set("repeat", Value::Text(now.repeat.clone().unwrap_or_else(|| "none".into())))?;
        set("position", Value::Number(now.position.round()))?;
        set("duration", Value::Number(now.duration.round()))?;
        let progress = if now.duration > 0.0 {
            (now.position / now.duration * 100.0).clamp(0.0, 100.0).round()
        } else {
            0.0
        };
        set("progress", Value::Number(progress))?;

        // Art is fetched once per track, not once per second.
        if let Some(images) = &self.images {
            let track = format!("{}{}{}", now.app, now.title, now.album);
            let known = self
                .art
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(app)
                .filter(|(seen, _)| *seen == track)
                .map(|(_, image)| image.clone());
            let image = match known {
                Some(image) => image,
                None => {
                    let image = self
                        .sessions()
                        .art(app)?
                        .map(|(bytes, mime)| images.put(&mime, bytes))
                        .unwrap_or_default();
                    self.art
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(app.to_string(), (track, image.clone()));
                    image
                }
            };
            set("art", Value::Text(image))?;
        }
        Ok(())
    }
}

/// `media.<app>.<field>` as `(app, field)`; the app may hold dots.
fn parse_variable(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("media.")?;
    let (app, field) = rest.rsplit_once('.')?;
    (!app.trim().is_empty() && FIELDS.iter().any(|(known, _, _)| *known == field)).then_some((app, field))
}

impl Plugin for Media {
    fn manifest(&self) -> crate::abi::ManifestDto {
        MEDIA_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        FIELDS
            .iter()
            .map(|(field, kind, description)| {
                VariableSpec::new(format!("media.current.{field}"), *kind, *description).read_only()
            })
            .collect()
    }

    fn choices(
        &self,
        action: &str,
        _args: &HashMap<String, String>,
    ) -> Result<crate::abi::ChoicesResponse, PluginError> {
        let mut answer = crate::abi::ChoicesResponse::default();
        if !action.starts_with("session_") && !action.starts_with("seek") && !matches!(action, "toggle_shuffle" | "cycle_repeat") {
            return Ok(answer);
        }
        match self.sessions().sessions() {
            Ok(ids) => {
                let mut choices = vec![crate::abi::ChoiceDto {
                    value: "current".into(),
                    label: Some("Whichever is playing".into()),
                }];
                choices.extend(ids.into_iter().map(|id| crate::abi::ChoiceDto { value: id, label: None }));
                answer.arguments.insert("app".into(), choices);
            }
            Err(error) => answer.error = Some(error.to_string()),
        }
        Ok(answer)
    }

    fn sample(&mut self, wanted: &[&str], ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        let mut apps: Vec<&str> = wanted.iter().filter_map(|name| parse_variable(name).map(|(app, _)| app)).collect();
        apps.sort_unstable();
        apps.dedup();
        for app in apps {
            self.publish(ctx, app)?;
        }
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        if let Some((_, _, key)) = KEYS.iter().find(|(name, _, _)| *name == action) {
            let pressed = CoreAction::Hotkey(Hotkey::new(Vec::new(), *key));
            ctx.input.perform(&pressed)?;
            return Ok(Some(pressed).into());
        }
        let app = argument(args, "app").trim();
        let number = |name: &str| -> Result<f64, PluginError> {
            let raw = argument(args, name);
            raw.trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| PluginError::Failed(format!("'{name}' must be a number, not '{raw}'")))
        };
        let command = match action {
            "session_play_pause" => Command::PlayPause,
            "session_next" => Command::Next,
            "session_previous" => Command::Previous,
            "session_stop" => Command::Stop,
            "toggle_shuffle" => Command::ToggleShuffle,
            "cycle_repeat" => Command::CycleRepeat,
            "seek" => Command::Seek(number("seconds")?.max(0.0)),
            "seek_by" | "seek_percent" => {
                let now = self
                    .sessions()
                    .now_playing(app)?
                    .ok_or_else(|| PluginError::Failed(format!("{app} is not playing anything")))?;
                let to = if action == "seek_by" {
                    now.position + number("seconds")?
                } else {
                    let percent = number("percent")?;
                    if !(0.0..=100.0).contains(&percent) {
                        return Err(PluginError::Failed("'percent' must be 0 to 100".into()));
                    }
                    now.duration * percent / 100.0
                };
                Command::Seek(to.clamp(0.0, now.duration.max(0.0)))
            }
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: MEDIA_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        };
        self.sessions().command(app, command)?;
        // Shown at once rather than at the next sample.
        let _ = self.publish(ctx, app);
        let state = match action {
            "session_play_pause" => {
                let playing = ctx
                    .vars
                    .get(&format!("media.{app}.playing"))
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                Some(if playing { "playing" } else { "paused" })
            }
            _ => None,
        };
        Ok(Outcome {
            input: None,
            state: state.map(str::to_string),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_and_its_lights_are_valid() {
        let manifest = MEDIA_MANIFEST.to_dto();
        manifest.validate().expect("valid");
        assert!(manifest.uses(crate::abi::capability::TOGGLES), "its toggles light up");
    }

    #[test]
    fn the_catalogue_and_the_key_table_name_the_same_actions() {
        let catalogued: Vec<(&str, &str)> = MEDIA_ACTIONS
            .iter()
            .take(KEYS.len())
            .map(|spec| (spec.action, spec.description))
            .collect();
        let keyed: Vec<(&str, &str)> = KEYS.iter().map(|(a, d, _)| (*a, *d)).collect();
        assert_eq!(catalogued, keyed);
    }

    use super::super::media_sessions::{RecordingImages, RecordingSessions};
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    fn run(media: &mut Media, vars: &VariableStore, action: &str, pairs: &[(&str, &str)]) -> Result<Outcome, PluginError> {
        let args = pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx { input: &mut input, vars, notifier: &mut notifier, settings: &settings, now_ms: 0 };
        media.invoke(action, &args, &mut ctx)
    }

    fn sample(media: &mut Media, vars: &VariableStore, names: &[&str]) {
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx { input: &mut input, vars, notifier: &mut notifier, settings: &settings, now_ms: 0 };
        media.sample(names, &mut ctx).expect("samples");
    }

    #[test]
    fn a_named_players_track_is_shown_and_its_art_fetched_once() {
        let images = Arc::new(RecordingImages::default());
        let mut media = Media::new(Box::new(RecordingSessions::new()), Some(images.clone()));
        let vars = VariableStore::new();
        vars.declare_all(media.variables());
        sample(&mut media, &vars, &["media.tidal.title", "media.tidal.art"]);
        assert_eq!(vars.get("media.tidal.title"), Some(Value::Text("Song B".into())));
        assert_eq!(vars.get("media.tidal.progress"), Some(Value::Number(15.0)));
        let art = vars.get("media.tidal.art").and_then(|v| v.as_text().map(str::to_string)).unwrap();
        assert!(art.starts_with("img:"), "{art}");
        sample(&mut media, &vars, &["media.tidal.art"]);
        assert_eq!(images.given.lock().unwrap().len(), 1, "same track, same picture");
        sample(&mut media, &vars, &["media.current.title"]);
        assert_eq!(vars.get("media.current.title"), Some(Value::Text("Song A".into())));
    }

    #[test]
    fn controls_reach_the_player_they_name() {
        let mut media = Media::new(Box::new(RecordingSessions::new()), None);
        let vars = VariableStore::new();
        let paused = run(&mut media, &vars, "session_play_pause", &[("app", "spotify")]).unwrap();
        assert_eq!(paused.state.as_deref(), Some("paused"));
        run(&mut media, &vars, "seek_percent", &[("app", "tidal"), ("percent", "50")]).unwrap();
        assert_eq!(vars.get("media.tidal.position"), Some(Value::Number(100.0)));
        run(&mut media, &vars, "seek_by", &[("app", "tidal"), ("seconds", "-500")]).unwrap();
        assert_eq!(vars.get("media.tidal.position"), Some(Value::Number(0.0)), "clamped at the start");
        run(&mut media, &vars, "cycle_repeat", &[("app", "current")]).unwrap();
        assert_eq!(vars.get("media.current.repeat"), Some(Value::Text("list".into())));
        assert!(run(&mut media, &vars, "session_next", &[("app", "winamp")]).is_err());
    }

    #[test]
    fn variable_names_keep_dots_in_the_app() {
        assert_eq!(parse_variable("media.Spotify.exe.title"), Some(("Spotify.exe", "title")));
        assert_eq!(parse_variable("media.spotify.colour"), None);
        assert_eq!(parse_variable("system.audio.volume"), None);
    }
}

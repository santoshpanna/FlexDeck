//! The players on this computer, one by one: what each is playing, and its own
//! play, skip, seek, shuffle and repeat.
//!
//! The media *keys* (in `media.rs`) reach whichever player Windows picks. This
//! reaches a particular one — Spotify, TIDAL, Apple Music, a browser — through
//! Windows' media sessions (the same ones the volume flyout shows), and reads
//! back the title, the artist, the position and the album art, so a deck can
//! show them. Live in `media_windows.rs`; [`RecordingSessions`] for tests.

use std::sync::Arc;

use crate::PluginError;

/// What a player says it is playing.
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    /// The session's app id: `Spotify.exe`, `AppleInc.AppleMusicWin_…!App`.
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// `None` when the player does not say.
    pub shuffle: Option<bool>,
    /// `none`, `track` or `list`; `None` when the player does not say.
    pub repeat: Option<String>,
    /// Seconds, extrapolated to now while playing.
    pub position: f64,
    pub duration: f64,
}

/// Something one player can be told to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    Stop,
    /// To this many seconds in.
    Seek(f64),
    ToggleShuffle,
    /// none → list → track → none.
    CycleRepeat,
}

/// The players. `app` is matched forgivingly (see [`matches_app`]); blank or
/// `current` means the session Windows considers current.
pub trait MediaSessions: Send + std::fmt::Debug {
    /// Every session's app id.
    fn sessions(&mut self) -> Result<Vec<String>, PluginError>;
    /// What `app` is playing, or `None` when it has no session.
    fn now_playing(&mut self, app: &str) -> Result<Option<NowPlaying>, PluginError>;
    /// The album art `app` shows, as `(bytes, mime type)`.
    fn art(&mut self, app: &str) -> Result<Option<(Vec<u8>, String)>, PluginError>;
    fn command(&mut self, app: &str, command: Command) -> Result<(), PluginError>;
}

/// Hands a picture to the agent and gets back the `img:<sha256>` a widget can
/// show it by. Built-ins get one from the agent; plugins use `images/put`.
pub trait ImageSink: Send + Sync + std::fmt::Debug {
    fn put(&self, mime: &str, bytes: Vec<u8>) -> String;
}

/// Whether a session's app id is the player `wanted` names. Spoken names map to
/// the ids the apps really use — "Apple Music" is `AppleInc.AppleMusicWin…`.
pub fn matches_app(id: &str, wanted: &str) -> bool {
    let id = id.to_lowercase();
    let wanted: String = wanted
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.')
        .collect();
    !wanted.is_empty() && id.replace([' ', '_'], "").contains(&wanted)
}

/// Whether `app` means "whichever player is current".
pub fn is_current(app: &str) -> bool {
    let app = app.trim();
    app.is_empty() || app.eq_ignore_ascii_case("current")
}

const NO_SESSIONS: &str = "this build cannot reach media players on this platform";

#[derive(Debug, Default)]
pub struct UnavailableSessions;

impl MediaSessions for UnavailableSessions {
    fn sessions(&mut self) -> Result<Vec<String>, PluginError> {
        Err(PluginError::Unavailable(NO_SESSIONS.into()))
    }
    fn now_playing(&mut self, _app: &str) -> Result<Option<NowPlaying>, PluginError> {
        Err(PluginError::Unavailable(NO_SESSIONS.into()))
    }
    fn art(&mut self, _app: &str) -> Result<Option<(Vec<u8>, String)>, PluginError> {
        Err(PluginError::Unavailable(NO_SESSIONS.into()))
    }
    fn command(&mut self, _app: &str, _command: Command) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_SESSIONS.into()))
    }
}

/// Spotify (current) and TIDAL, playing pretend tracks; commands change them.
#[derive(Debug)]
pub struct RecordingSessions {
    pub players: Vec<NowPlaying>,
    pub calls: Vec<String>,
}

impl Default for RecordingSessions {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingSessions {
    pub fn new() -> Self {
        let track = |app: &str, title: &str, artist: &str| NowPlaying {
            app: app.into(),
            title: title.into(),
            artist: artist.into(),
            album: "Album".into(),
            playing: true,
            shuffle: Some(false),
            repeat: Some("none".into()),
            position: 30.0,
            duration: 200.0,
        };
        Self {
            players: vec![
                track("Spotify.exe", "Song A", "Artist A"),
                track("TIDAL.exe", "Song B", "Artist B"),
            ],
            calls: Vec::new(),
        }
    }

    fn find(&mut self, app: &str) -> Option<&mut NowPlaying> {
        if is_current(app) {
            return self.players.first_mut();
        }
        self.players.iter_mut().find(|player| matches_app(&player.app, app))
    }
}

impl MediaSessions for RecordingSessions {
    fn sessions(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(self.players.iter().map(|player| player.app.clone()).collect())
    }

    fn now_playing(&mut self, app: &str) -> Result<Option<NowPlaying>, PluginError> {
        Ok(self.find(app).cloned())
    }

    fn art(&mut self, app: &str) -> Result<Option<(Vec<u8>, String)>, PluginError> {
        Ok(self
            .find(app)
            .map(|player| (player.title.clone().into_bytes(), "image/png".to_string())))
    }

    fn command(&mut self, app: &str, command: Command) -> Result<(), PluginError> {
        let player = self
            .find(app)
            .ok_or_else(|| PluginError::Failed(format!("{app} is not playing anything")))?;
        match command {
            Command::PlayPause => player.playing = !player.playing,
            Command::Next => player.title = format!("{} (next)", player.title),
            Command::Previous => player.position = 0.0,
            Command::Stop => player.playing = false,
            Command::Seek(seconds) => player.position = seconds.clamp(0.0, player.duration),
            Command::ToggleShuffle => player.shuffle = Some(!player.shuffle.unwrap_or(false)),
            Command::CycleRepeat => {
                player.repeat = Some(
                    match player.repeat.as_deref() {
                        Some("none") | None => "list",
                        Some("list") => "track",
                        _ => "none",
                    }
                    .into(),
                )
            }
        }
        let app = player.app.clone();
        self.calls.push(format!("{command:?} {app}"));
        Ok(())
    }
}

/// An image sink that remembers what it was given, for tests.
#[derive(Debug, Default)]
pub struct RecordingImages {
    pub given: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
}

impl ImageSink for RecordingImages {
    fn put(&self, mime: &str, bytes: Vec<u8>) -> String {
        let mut given = self.given.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        given.push((mime.to_string(), bytes));
        format!("img:{:064x}", given.len())
    }
}

/// No images go anywhere: art variables stay empty.
pub fn no_images() -> Option<Arc<dyn ImageSink>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_are_named_the_way_people_say_them() {
        assert!(matches_app("Spotify.exe", "spotify"));
        assert!(matches_app("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App", "Apple Music"));
        assert!(matches_app("TIDAL.exe", "Tidal"));
        assert!(!matches_app("Spotify.exe", "tidal"));
        assert!(!matches_app("Spotify.exe", "  "));
        assert!(is_current("") && is_current("Current") && !is_current("spotify"));
    }
}

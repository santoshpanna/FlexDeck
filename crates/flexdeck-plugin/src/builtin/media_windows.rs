//! The live [`MediaSessions`] for Windows: `Windows.Media.Control`'s
//! `GlobalSystemMediaTransportControlsSessionManager` — the sessions the volume
//! flyout's media controls show.
//!
//! Its calls are asynchronous; each is waited on here with `.join()`, which is
//! fine on the sampler's and the press handler's threads (never a UI thread).
//! As with the other backends, nothing is cached between calls.

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::MediaPlaybackAutoRepeatMode as Repeat;
use windows::Storage::Streams::DataReader;

use super::media_sessions::{is_current, matches_app, Command, MediaSessions, NowPlaying};
use crate::PluginError;

#[derive(Debug, Default)]
pub struct WindowsSessions;

impl WindowsSessions {
    pub fn new() -> Self {
        Self
    }
}

fn failed(what: &str, error: windows::core::Error) -> PluginError {
    PluginError::Failed(format!("{what}: {}", error.message()))
}

fn manager() -> Result<Manager, PluginError> {
    Manager::RequestAsync()
        .and_then(|operation| operation.join())
        .map_err(|error| failed("could not reach Windows' media controls", error))
}

/// The session `app` names, or `None` when that player has none.
fn session(app: &str) -> Result<Option<Session>, PluginError> {
    let manager = manager()?;
    if is_current(app) {
        return Ok(manager.GetCurrentSession().ok());
    }
    let sessions = manager
        .GetSessions()
        .map_err(|error| failed("could not list the players", error))?;
    for session in sessions {
        let id = session.SourceAppUserModelId().map(|id| id.to_string()).unwrap_or_default();
        if matches_app(&id, app) {
            return Ok(Some(session));
        }
    }
    Ok(None)
}

fn required(app: &str) -> Result<Session, PluginError> {
    session(app)?.ok_or_else(|| {
        PluginError::Failed(if is_current(app) {
            "nothing is playing".to_string()
        } else {
            format!("{app} is not playing anything (or is not running)")
        })
    })
}

/// 100-nanosecond ticks as seconds.
fn seconds(ticks: i64) -> f64 {
    ticks as f64 / 10_000_000.0
}

/// A timeline's `(position, duration)` in seconds, `now` in seconds since 1601.
fn timeline_seconds(start: i64, end: i64, at: i64, updated: i64, playing: bool, now: f64) -> (f64, f64) {
    // Browsers sometimes report an empty timeline; there is nothing to count from.
    if end <= start {
        return (0.0, 0.0);
    }
    let duration = seconds(end - start);
    // Players report the position only now and then; while playing
    // it has moved on since, by however long ago that was.
    let mut position = seconds(at - start);
    if playing && updated > 0 {
        let since = now - seconds(updated);
        if (0.0..3600.0).contains(&since) {
            position += since;
        }
    }
    (position.clamp(0.0, duration), duration)
}

impl MediaSessions for WindowsSessions {
    fn sessions(&mut self) -> Result<Vec<String>, PluginError> {
        let sessions = manager()?
            .GetSessions()
            .map_err(|error| failed("could not list the players", error))?;
        Ok(sessions
            .into_iter()
            .filter_map(|session| session.SourceAppUserModelId().ok().map(|id| id.to_string()))
            .collect())
    }

    fn now_playing(&mut self, app: &str) -> Result<Option<NowPlaying>, PluginError> {
        let Some(session) = session(app)? else { return Ok(None) };
        let id = session.SourceAppUserModelId().map(|id| id.to_string()).unwrap_or_default();
        let properties = session
            .TryGetMediaPropertiesAsync()
            .and_then(|operation| operation.join())
            .map_err(|error| failed("could not read what is playing", error))?;
        let text = |value: windows::core::Result<windows::core::HSTRING>| {
            value.map(|text| text.to_string()).unwrap_or_default()
        };
        let info = session
            .GetPlaybackInfo()
            .map_err(|error| failed("could not read the player's state", error))?;
        let playing = info.PlaybackStatus().map(|status| status == Status::Playing).unwrap_or(false);
        let shuffle = info.IsShuffleActive().ok().and_then(|value| value.Value().ok());
        let repeat = info.AutoRepeatMode().ok().and_then(|value| value.Value().ok()).map(|mode| {
            match mode {
                Repeat::Track => "track",
                Repeat::List => "list",
                _ => "none",
            }
            .to_string()
        });
        let (position, duration) = match session.GetTimelineProperties() {
            Ok(timeline) => {
                let start = timeline.StartTime().map(|time| time.Duration).unwrap_or(0);
                let end = timeline.EndTime().map(|time| time.Duration).unwrap_or(0);
                let at = timeline.Position().map(|time| time.Duration).unwrap_or(0);
                let updated = timeline.LastUpdatedTime().map(|time| time.UniversalTime).unwrap_or(0);
                // DateTime ticks count from 1601; the Unix epoch is 11644473600 s later.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_secs_f64())
                    .unwrap_or(0.0)
                    + 11_644_473_600.0;
                timeline_seconds(start, end, at, updated, playing, now)
            }
            Err(_) => (0.0, 0.0),
        };
        Ok(Some(NowPlaying {
            app: id,
            title: text(properties.Title()),
            artist: text(properties.Artist()),
            album: text(properties.AlbumTitle()),
            playing,
            shuffle,
            repeat,
            position,
            duration,
        }))
    }

    fn art(&mut self, app: &str) -> Result<Option<(Vec<u8>, String)>, PluginError> {
        let Some(session) = session(app)? else { return Ok(None) };
        let properties = session
            .TryGetMediaPropertiesAsync()
            .and_then(|operation| operation.join())
            .map_err(|error| failed("could not read what is playing", error))?;
        let Ok(thumbnail) = properties.Thumbnail() else { return Ok(None) };
        let stream = thumbnail
            .OpenReadAsync()
            .and_then(|operation| operation.join())
            .map_err(|error| failed("could not open the album art", error))?;
        let size = stream.Size().unwrap_or(0);
        if size == 0 || size > 4 * 1024 * 1024 {
            return Ok(None);
        }
        let mime = stream
            .ContentType()
            .map(|kind| kind.to_string())
            .ok()
            .filter(|kind| kind.starts_with("image/"))
            .unwrap_or_else(|| "image/png".into());
        let reader = DataReader::CreateDataReader(&stream).map_err(|error| failed("could not read the album art", error))?;
        reader
            .LoadAsync(size as u32)
            .and_then(|operation| operation.join())
            .map_err(|error| failed("could not read the album art", error))?;
        let mut bytes = vec![0u8; size as usize];
        reader
            .ReadBytes(&mut bytes)
            .map_err(|error| failed("could not read the album art", error))?;
        Ok(Some((bytes, mime)))
    }

    fn command(&mut self, app: &str, command: Command) -> Result<(), PluginError> {
        let session = required(app)?;
        let done = match command {
            Command::PlayPause => session.TryTogglePlayPauseAsync().and_then(|op| op.join()),
            Command::Next => session.TrySkipNextAsync().and_then(|op| op.join()),
            Command::Previous => session.TrySkipPreviousAsync().and_then(|op| op.join()),
            Command::Stop => session.TryStopAsync().and_then(|op| op.join()),
            Command::Seek(to) => {
                let start = session
                    .GetTimelineProperties()
                    .and_then(|timeline| timeline.StartTime())
                    .map(|time| time.Duration)
                    .unwrap_or(0);
                session
                    .TryChangePlaybackPositionAsync(start + (to * 10_000_000.0) as i64)
                    .and_then(|op| op.join())
            }
            Command::ToggleShuffle => {
                let now = session
                    .GetPlaybackInfo()
                    .and_then(|info| info.IsShuffleActive())
                    .and_then(|value| value.Value())
                    .unwrap_or(false);
                session.TryChangeShuffleActiveAsync(!now).and_then(|op| op.join())
            }
            Command::CycleRepeat => {
                let now = session
                    .GetPlaybackInfo()
                    .and_then(|info| info.AutoRepeatMode())
                    .and_then(|value| value.Value())
                    .unwrap_or(Repeat::None);
                let next = match now {
                    Repeat::None => Repeat::List,
                    Repeat::List => Repeat::Track,
                    _ => Repeat::None,
                };
                session.TryChangeAutoRepeatModeAsync(next).and_then(|op| op.join())
            }
        }
        .map_err(|error| failed("the player did not answer", error))?;
        if done {
            Ok(())
        } else {
            Err(PluginError::Failed(format!("the player refused {command:?} (it may not support it)")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICKS: i64 = 10_000_000;

    #[test]
    fn an_empty_timeline_does_not_count_up() {
        let updated = 1_000 * TICKS;
        assert_eq!(timeline_seconds(0, 0, 0, updated, true, 1_019.0), (0.0, 0.0));
    }

    #[test]
    fn a_playing_timeline_moves_on_since_its_last_update() {
        let updated = 1_000 * TICKS;
        assert_eq!(timeline_seconds(0, 300 * TICKS, 100 * TICKS, updated, true, 1_010.0), (110.0, 300.0));
        assert_eq!(timeline_seconds(0, 300 * TICKS, 100 * TICKS, updated, false, 1_010.0), (100.0, 300.0));
        assert_eq!(timeline_seconds(0, 300 * TICKS, 295 * TICKS, updated, true, 1_010.0), (300.0, 300.0), "clamped at the end");
    }

    #[test]
    #[ignore = "reads the real players; run by hand on Windows"]
    fn live_lists_players_and_reads_what_is_playing() {
        let mut sessions = WindowsSessions::new();
        let ids = sessions.sessions().expect("sessions");
        eprintln!("players: {ids:?}");
        for id in ids {
            let now = sessions.now_playing(&id).expect("now playing");
            eprintln!("{now:?}");
            if let Some(now) = now {
                assert!(now.duration >= 0.0 && now.position >= 0.0);
            }
            let art = sessions.art(&id).expect("art");
            eprintln!("art: {:?}", art.map(|(bytes, mime)| (bytes.len(), mime)));
        }
    }
}

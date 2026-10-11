//! The plugins this build ships.
//!
//! Every one of them is constructed with its capabilities injected, so a test
//! builds the whole set with recorders and exercises the engine without touching
//! the machine.

pub mod core;
pub mod deck;
pub mod audio;
pub mod display;
#[cfg(windows)]
pub mod display_windows;
pub mod launcher;
#[cfg(windows)]
pub mod launcher_windows;
pub mod media;
pub mod media_sessions;
#[cfg(windows)]
pub mod media_windows;
pub mod notifications;
pub mod sensors;
#[cfg(windows)]
pub mod sensors_windows;
pub mod system;
pub mod time;
pub mod timer;
pub mod vars;
#[cfg(windows)]
pub mod audio_windows;
#[cfg(windows)]
pub mod system_windows;

use crate::{Plugin, SystemBackend, UnavailableSystem};
use audio::{AudioBackend, RecordingAudio, UnavailableAudio};
use display::{DisplayBackend, RecordingDisplay, UnavailableDisplay};
use media_sessions::{ImageSink, MediaSessions, RecordingSessions, UnavailableSessions};
use launcher::{Launcher, LiveLauncher, RecordingLauncher, UnavailableLauncher};

/// The capabilities the built-ins need from the operating system.
#[derive(Debug)]
pub struct Backends {
    pub system: Box<dyn SystemBackend>,
    pub audio: Box<dyn AudioBackend>,
    pub display: Box<dyn DisplayBackend>,
    pub launcher: Box<dyn Launcher>,
    pub sessions: Box<dyn MediaSessions>,
    /// Where built-ins put pictures (album art). `None`: nowhere.
    pub images: Option<std::sync::Arc<dyn ImageSink>>,
    /// Where "go to page" goes: the tablets. `None`: it cannot be done here.
    pub pages: Option<std::sync::Arc<dyn deck::PageControl>>,
    pub sensors: Box<dyn sensors::SensorBackend>,
}

impl Backends {
    /// Every capability refuses with a reason. The default before startup has
    /// decided this is a real machine.
    pub fn unavailable() -> Self {
        Self {
            system: Box::new(UnavailableSystem),
            audio: Box::new(UnavailableAudio),
            display: Box::new(UnavailableDisplay),
            launcher: Box::new(UnavailableLauncher),
            sessions: Box::new(UnavailableSessions),
            images: None,
            pages: None,
            sensors: Box::new(sensors::UnavailableSensors),
        }
    }

    /// Every capability writes down what it would have done. For tests.
    pub fn recording() -> Self {
        Self {
            system: Box::new(system::RecordingSystem::new()),
            audio: Box::new(RecordingAudio::new()),
            display: Box::new(RecordingDisplay::new()),
            launcher: Box::new(RecordingLauncher::new()),
            sessions: Box::new(RecordingSessions::new()),
            images: None,
            pages: Some(std::sync::Arc::new(deck::RecordingPages {
                pages: vec![("home".into(), "Home".into()), ("stream".into(), "Stream".into())],
                ..Default::default()
            })),
            sensors: Box::new(sensors::FixedSensors(sensors::Readings {
                memory_used_percent: Some(50.0),
                cores: vec![25.0, 75.0],
                ..Default::default()
            })),
        }
    }

    /// The real thing, as far as this platform has one.
    pub fn live() -> Self {
        Self {
            system: live_system(),
            audio: live_audio(),
            display: live_display(),
            launcher: Box::new(LiveLauncher),
            sessions: live_sessions(),
            images: None,
            pages: None,
            sensors: live_sensors(),
        }
    }
}

#[cfg(windows)]
fn live_sensors() -> Box<dyn sensors::SensorBackend> {
    Box::new(sensors_windows::WindowsSensors::new())
}

#[cfg(not(windows))]
fn live_sensors() -> Box<dyn sensors::SensorBackend> {
    Box::new(sensors::UnavailableSensors)
}

#[cfg(windows)]
fn live_system() -> Box<dyn SystemBackend> {
    Box::new(system_windows::WindowsSystem::new())
}

#[cfg(not(windows))]
fn live_system() -> Box<dyn SystemBackend> {
    Box::new(UnavailableSystem)
}

#[cfg(windows)]
fn live_audio() -> Box<dyn AudioBackend> {
    Box::new(audio_windows::WindowsAudio::new())
}

#[cfg(not(windows))]
fn live_audio() -> Box<dyn AudioBackend> {
    Box::new(UnavailableAudio)
}

#[cfg(windows)]
fn live_sessions() -> Box<dyn MediaSessions> {
    Box::new(media_windows::WindowsSessions::new())
}

#[cfg(not(windows))]
fn live_sessions() -> Box<dyn MediaSessions> {
    Box::new(UnavailableSessions)
}

#[cfg(windows)]
fn live_display() -> Box<dyn DisplayBackend> {
    Box::new(display_windows::WindowsDisplay::new())
}

#[cfg(not(windows))]
fn live_display() -> Box<dyn DisplayBackend> {
    Box::new(UnavailableDisplay)
}

/// The built-in plugin set.
pub fn builtins(backends: Backends) -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(core::Core::new()),
        Box::new(system::System::new(backends.system, backends.audio)),
        Box::new(notifications::Notifications::new()),
        Box::new(media::Media::new(backends.sessions, backends.images)),
        Box::new(launcher::LauncherPlugin::new(backends.launcher)),
        Box::new(display::Display::new(backends.display)),
        Box::new(time::Time::new()),
        Box::new(deck::Deck::new(backends.pages)),
        Box::new(vars::Vars::new()),
        Box::new(timer::Timers::new()),
        Box::new(sensors::Sensors::new(backends.sensors)),
    ]
}

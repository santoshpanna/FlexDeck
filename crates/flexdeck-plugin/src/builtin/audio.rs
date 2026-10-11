//! Per-device and per-application audio, for the `system` plugin.
//!
//! [`crate::SystemBackend`] answers for "the" output: the default device's
//! volume and mute. A slider wants more than that — the microphone, the
//! headphones that are not the default right now, Spotify alone — so this is a
//! second capability beside it, with the same three shapes: live (Windows, in
//! `audio_windows.rs`), [`RecordingAudio`] for tests, and [`UnavailableAudio`].
//!
//! # Naming things the way people do
//!
//! A device is named by its friendly name and matched forgivingly (see
//! [`pick`]): people type "Headphones", Windows calls it "Headphones
//! (WH-1000XM4)". An application is named by its executable, lowercased —
//! `spotify.exe` — because that is stable across languages and versions where a
//! window title is not.

use crate::PluginError;

/// Which way audio flows through a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flow {
    /// Speakers, headphones: what the machine plays.
    Output,
    /// Microphones: what the machine hears.
    Input,
}

impl Flow {
    pub fn noun(self) -> &'static str {
        match self {
            Flow::Output => "output",
            Flow::Input => "input",
        }
    }
}

/// One audio device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    /// Whether it is the default for its flow right now.
    pub default: bool,
}

/// One application that has audio open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioApp {
    /// Its executable, lowercased: `spotify.exe`.
    pub exe: String,
    /// What it calls itself, when it says: "Spotify".
    pub title: Option<String>,
}

/// Per-device and per-application volume. Percentages are 0-100.
///
/// A blank `device` means the default device for that flow.
pub trait AudioBackend: Send + std::fmt::Debug {
    fn devices(&mut self, flow: Flow) -> Result<Vec<AudioDevice>, PluginError>;
    fn device_volume(&mut self, flow: Flow, device: &str) -> Result<f64, PluginError>;
    fn set_device_volume(&mut self, flow: Flow, device: &str, percent: f64)
        -> Result<(), PluginError>;
    fn device_muted(&mut self, flow: Flow, device: &str) -> Result<bool, PluginError>;
    fn set_device_muted(&mut self, flow: Flow, device: &str, muted: bool)
        -> Result<(), PluginError>;
    /// Make `device` the default for its flow.
    fn set_default_device(&mut self, flow: Flow, device: &str) -> Result<(), PluginError>;

    fn apps(&mut self) -> Result<Vec<AudioApp>, PluginError>;
    fn app_volume(&mut self, exe: &str) -> Result<f64, PluginError>;
    fn set_app_volume(&mut self, exe: &str, percent: f64) -> Result<(), PluginError>;
    fn app_muted(&mut self, exe: &str) -> Result<bool, PluginError>;
    fn set_app_muted(&mut self, exe: &str, muted: bool) -> Result<(), PluginError>;
}

/// Pick the one device `wanted` means, out of `names`: exactly, then ignoring
/// case, then as the only name containing it. A blank `wanted` picks nothing,
/// since that means "the default" and is the caller's to resolve.
///
/// Refuses with every name listed, so the person can see what to type.
pub fn pick<'a>(names: &'a [String], wanted: &str, flow: Flow) -> Result<&'a str, PluginError> {
    let wanted = wanted.trim();
    let lower = wanted.to_lowercase();
    let exact = names.iter().find(|name| *name == wanted);
    let insensitive = || names.iter().find(|name| name.eq_ignore_ascii_case(wanted));
    let partial = || {
        let matches: Vec<&String> = names
            .iter()
            .filter(|name| name.to_lowercase().contains(&lower))
            .collect();
        (matches.len() == 1).then(|| matches[0])
    };
    exact
        .or_else(insensitive)
        .or_else(partial)
        .filter(|_| !wanted.is_empty())
        .map(String::as_str)
        .ok_or_else(|| {
            PluginError::Failed(format!(
                "no single {} device matches '{wanted}' (there are: {})",
                flow.noun(),
                names.join(", ")
            ))
        })
}

/// The executable an app argument means: `Spotify` and `spotify.exe` both mean
/// `spotify.exe`.
pub fn exe_name(app: &str) -> String {
    let lower = app.trim().to_lowercase();
    if lower.ends_with(".exe") {
        lower
    } else {
        format!("{lower}.exe")
    }
}

const NO_AUDIO: &str = "this build has no audio backend for this platform";

/// Every call refuses with a reason.
#[derive(Debug, Default)]
pub struct UnavailableAudio;

impl AudioBackend for UnavailableAudio {
    fn devices(&mut self, _flow: Flow) -> Result<Vec<AudioDevice>, PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn device_volume(&mut self, _flow: Flow, _device: &str) -> Result<f64, PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn set_device_volume(&mut self, _: Flow, _: &str, _: f64) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn device_muted(&mut self, _flow: Flow, _device: &str) -> Result<bool, PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn set_device_muted(&mut self, _: Flow, _: &str, _: bool) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn set_default_device(&mut self, _flow: Flow, _device: &str) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn apps(&mut self) -> Result<Vec<AudioApp>, PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn app_volume(&mut self, _exe: &str) -> Result<f64, PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn set_app_volume(&mut self, _exe: &str, _percent: f64) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn app_muted(&mut self, _exe: &str) -> Result<bool, PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
    fn set_app_muted(&mut self, _exe: &str, _muted: bool) -> Result<(), PluginError> {
        Err(PluginError::Unavailable(NO_AUDIO.into()))
    }
}

/// One recorded device: its level and mute.
#[derive(Debug, Clone)]
struct Level {
    volume: f64,
    muted: bool,
}

/// Remembers what it was told, for tests. Starts with two outputs, one
/// microphone, and Spotify and Discord playing.
#[derive(Debug)]
pub struct RecordingAudio {
    outputs: Vec<(String, Level)>,
    inputs: Vec<(String, Level)>,
    default_output: String,
    default_input: String,
    apps: Vec<(String, Level)>,
    calls: Vec<String>,
}

impl Default for RecordingAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingAudio {
    pub fn new() -> Self {
        let level = |volume| Level {
            volume,
            muted: false,
        };
        Self {
            outputs: vec![
                ("Speakers (Realtek Audio)".into(), level(40.0)),
                ("Headphones (WH-1000XM4)".into(), level(60.0)),
            ],
            inputs: vec![("Microphone (Yeti)".into(), level(70.0))],
            default_output: "Speakers (Realtek Audio)".into(),
            default_input: "Microphone (Yeti)".into(),
            apps: vec![
                ("spotify.exe".into(), level(80.0)),
                ("discord.exe".into(), level(100.0)),
            ],
            calls: Vec::new(),
        }
    }

    pub fn calls(&self) -> &[String] {
        &self.calls
    }

    fn device(&mut self, flow: Flow, device: &str) -> Result<&mut Level, PluginError> {
        let (list, default) = match flow {
            Flow::Output => (&mut self.outputs, &self.default_output),
            Flow::Input => (&mut self.inputs, &self.default_input),
        };
        let names: Vec<String> = list.iter().map(|(name, _)| name.clone()).collect();
        let name = if device.trim().is_empty() {
            default.clone()
        } else {
            pick(&names, device, flow)?.to_string()
        };
        Ok(&mut list
            .iter_mut()
            .find(|(known, _)| *known == name)
            .expect("picked from this list")
            .1)
    }

    fn app(&mut self, exe: &str) -> Result<&mut Level, PluginError> {
        let exe = exe_name(exe);
        self.apps
            .iter_mut()
            .find(|(known, _)| *known == exe)
            .map(|(_, level)| level)
            .ok_or_else(|| PluginError::Failed(format!("{exe} is not playing anything")))
    }
}

impl AudioBackend for RecordingAudio {
    fn devices(&mut self, flow: Flow) -> Result<Vec<AudioDevice>, PluginError> {
        let (list, default) = match flow {
            Flow::Output => (&self.outputs, &self.default_output),
            Flow::Input => (&self.inputs, &self.default_input),
        };
        Ok(list
            .iter()
            .map(|(name, _)| AudioDevice {
                name: name.clone(),
                default: name == default,
            })
            .collect())
    }

    fn device_volume(&mut self, flow: Flow, device: &str) -> Result<f64, PluginError> {
        Ok(self.device(flow, device)?.volume)
    }

    fn set_device_volume(
        &mut self,
        flow: Flow,
        device: &str,
        percent: f64,
    ) -> Result<(), PluginError> {
        self.device(flow, device)?.volume = percent;
        self.calls
            .push(format!("set_device_volume {} {device} {percent}", flow.noun()));
        Ok(())
    }

    fn device_muted(&mut self, flow: Flow, device: &str) -> Result<bool, PluginError> {
        Ok(self.device(flow, device)?.muted)
    }

    fn set_device_muted(&mut self, flow: Flow, device: &str, muted: bool) -> Result<(), PluginError> {
        self.device(flow, device)?.muted = muted;
        self.calls
            .push(format!("set_device_muted {} {device} {muted}", flow.noun()));
        Ok(())
    }

    fn set_default_device(&mut self, flow: Flow, device: &str) -> Result<(), PluginError> {
        let names: Vec<String> = self
            .devices(flow)?
            .into_iter()
            .map(|device| device.name)
            .collect();
        let name = pick(&names, device, flow)?.to_string();
        self.calls
            .push(format!("set_default_device {} {name}", flow.noun()));
        match flow {
            Flow::Output => self.default_output = name,
            Flow::Input => self.default_input = name,
        }
        Ok(())
    }

    fn apps(&mut self) -> Result<Vec<AudioApp>, PluginError> {
        Ok(self
            .apps
            .iter()
            .map(|(exe, _)| AudioApp {
                exe: exe.clone(),
                // What a version resource would say: the name, capitalised.
                title: exe
                    .strip_suffix(".exe")
                    .map(|name| name[..1].to_uppercase() + &name[1..]),
            })
            .collect())
    }

    fn app_volume(&mut self, exe: &str) -> Result<f64, PluginError> {
        Ok(self.app(exe)?.volume)
    }

    fn set_app_volume(&mut self, exe: &str, percent: f64) -> Result<(), PluginError> {
        self.app(exe)?.volume = percent;
        self.calls
            .push(format!("set_app_volume {} {percent}", exe_name(exe)));
        Ok(())
    }

    fn app_muted(&mut self, exe: &str) -> Result<bool, PluginError> {
        Ok(self.app(exe)?.muted)
    }

    fn set_app_muted(&mut self, exe: &str, muted: bool) -> Result<(), PluginError> {
        self.app(exe)?.muted = muted;
        self.calls
            .push(format!("set_app_muted {} {muted}", exe_name(exe)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec![
            "Speakers (Realtek Audio)".into(),
            "Headphones (WH-1000XM4)".into(),
            "CABLE Input (VB-Audio Virtual Cable)".into(),
        ]
    }

    #[test]
    fn a_device_is_picked_exactly_then_by_case_then_by_a_unique_part() {
        let names = names();
        assert_eq!(
            pick(&names, "Speakers (Realtek Audio)", Flow::Output).unwrap(),
            "Speakers (Realtek Audio)"
        );
        assert_eq!(
            pick(&names, "headphones (wh-1000xm4)", Flow::Output).unwrap(),
            "Headphones (WH-1000XM4)"
        );
        assert_eq!(
            pick(&names, "cable", Flow::Output).unwrap(),
            "CABLE Input (VB-Audio Virtual Cable)"
        );
    }

    #[test]
    fn an_ambiguous_or_missing_device_is_refused_listing_them_all() {
        let names = names();
        let error = pick(&names, "(", Flow::Output).unwrap_err().to_string();
        assert!(error.contains("Speakers"), "{error}");
        assert!(pick(&names, "monitor", Flow::Output).is_err());
        assert!(pick(&names, "  ", Flow::Output).is_err(), "blank is the caller's default");
    }

    #[test]
    fn an_app_is_named_by_its_executable_however_it_is_typed() {
        assert_eq!(exe_name("Spotify"), "spotify.exe");
        assert_eq!(exe_name(" Discord.EXE "), "discord.exe");
    }
}

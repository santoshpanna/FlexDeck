//! Playing clips on up to two outputs at once, and mixing a microphone in.
//!
//! # Two outputs
//!
//! * **Monitor** — your headphones or speakers, so you hear what you play.
//! * **Cable** — a virtual audio cable (VB-CABLE, VoiceMeeter, Virtual Audio
//!   Cable). Whatever plays into its *input* comes out of its *output*, which
//!   you choose as your microphone in Discord or OBS — so others hear the clip.
//!   With **mic passthrough** on, your real microphone is mixed into the cable
//!   too, so one virtual microphone carries your voice *and* the clips. It is
//!   never mixed into the monitor: you would hear yourself, late.
//!
//! # Threads
//!
//! Sound-card streams cannot move between threads on every platform, so one
//! audio thread owns them, and everything else talks to it — or to the two
//! [`Mixer`]s, which the stream callbacks read from. The device layer is a trait
//! ([`Devices`]), so the tests run the whole engine against [`NullDevices`] and
//! pull audio out of the mixers by hand.

use std::sync::{mpsc, Arc, Mutex, PoisonError};

use crate::mix::{Clip, DriftBuffer, Mixer};

/// Where a clip plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Both,
    Monitor,
    Cable,
}

impl Output {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "both" | "" => Some(Self::Both),
            "monitor" => Some(Self::Monitor),
            "cable" => Some(Self::Cable),
            _ => None,
        }
    }
}

/// What playing a clip that is already playing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Start again from the top.
    Restart,
    /// Play another copy over it.
    Overlap,
    /// Stop it instead.
    Toggle,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "restart" | "" => Some(Self::Restart),
            "overlap" => Some(Self::Overlap),
            "toggle" => Some(Self::Toggle),
            _ => None,
        }
    }
}

/// Which devices to use. `None` is "the system default" for the monitor and
/// microphone, and "find a virtual cable" for the cable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub monitor: Option<String>,
    pub cable: Option<String>,
    pub mic: Option<String>,
    pub passthrough: bool,
}

/// What the devices layer actually opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Opened {
    pub monitor: Option<String>,
    pub cable: Option<String>,
    pub mic: Option<String>,
    /// Why something asked for could not be opened, in words.
    pub problems: Vec<String>,
}

/// The names virtual audio cables give their playback side.
const CABLE_NAMES: &[&str] = &[
    "cable input",
    "voicemeeter input",
    "voicemeeter aux input",
    "voicemeeter vaio3 input",
    "line 1 (virtual audio cable)",
    "virtual audio cable",
];

/// The output that is a virtual cable's input, if there is one.
pub fn find_cable(outputs: &[String]) -> Option<String> {
    CABLE_NAMES.iter().find_map(|wanted| {
        outputs
            .iter()
            .find(|name| name.to_lowercase().contains(wanted))
            .cloned()
    })
}

/// The one output or input `wanted` means: exact, then ignoring case, then the
/// only name containing it.
pub fn pick(names: &[String], wanted: &str) -> Option<String> {
    let lower = wanted.trim().to_lowercase();
    names
        .iter()
        .find(|name| name.as_str() == wanted.trim())
        .or_else(|| names.iter().find(|name| name.to_lowercase() == lower))
        .or_else(|| {
            let matches: Vec<&String> = names.iter().filter(|name| name.to_lowercase().contains(&lower)).collect();
            (matches.len() == 1).then(|| matches[0])
        })
        .cloned()
}

/// The sound cards. Implemented over cpal for real, and by [`NullDevices`].
pub trait Devices {
    fn outputs(&self) -> Vec<String>;
    fn inputs(&self) -> Vec<String>;
    /// Close what is open and open what `config` asks for, feeding `monitor`
    /// and `cable` (setting their rates) and filling `mic` from the microphone.
    fn open(
        &mut self,
        config: &Config,
        monitor: &Arc<Mutex<Mixer>>,
        cable: &Arc<Mutex<Mixer>>,
        mic: &Arc<Mutex<DriftBuffer>>,
    ) -> Opened;
}

enum Command {
    Configure(Config, mpsc::Sender<Opened>),
    Outputs(mpsc::Sender<(Vec<String>, Vec<String>)>),
}

/// Plays clips. Cheap to share; every method takes `&self`.
pub struct Engine {
    commands: Mutex<mpsc::Sender<Command>>,
    monitor: Arc<Mutex<Mixer>>,
    cable: Arc<Mutex<Mixer>>,
    opened: Mutex<Opened>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Engine").finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Engine {
    /// Start the audio thread, and make the devices on it — so a sound-card
    /// stream, which may not cross threads, never has to.
    pub fn new(make: impl FnOnce() -> Box<dyn Devices> + Send + 'static) -> Self {
        let monitor = Arc::new(Mutex::new(Mixer::new(48_000)));
        let cable = Arc::new(Mutex::new(Mixer::new(48_000)));
        let mic = Arc::new(Mutex::new(DriftBuffer::new(960, 480)));
        let (commands, inbox) = mpsc::channel::<Command>();
        {
            let monitor = Arc::clone(&monitor);
            let cable = Arc::clone(&cable);
            std::thread::Builder::new()
                .name("soundboard-audio".into())
                .spawn(move || {
                    let mut devices = make();
                    for command in inbox {
                        match command {
                            Command::Configure(config, reply) => {
                                let _ = reply.send(devices.open(&config, &monitor, &cable, &mic));
                            }
                            Command::Outputs(reply) => {
                                let _ = reply.send((devices.outputs(), devices.inputs()));
                            }
                        }
                    }
                })
                .expect("the audio thread starts");
        }
        Self {
            commands: Mutex::new(commands),
            monitor,
            cable,
            opened: Mutex::new(Opened::default()),
        }
    }

    /// Use these devices. Returns what was opened, and what could not be.
    pub fn configure(&self, config: Config) -> Opened {
        let (reply, answer) = mpsc::channel();
        let _ = lock(&self.commands).send(Command::Configure(config, reply));
        let opened = answer.recv().unwrap_or_else(|_| Opened {
            problems: vec!["the audio thread stopped".into()],
            ..Opened::default()
        });
        *lock(&self.opened) = opened.clone();
        opened
    }

    pub fn opened(&self) -> Opened {
        lock(&self.opened).clone()
    }

    /// Every output and input device's name.
    pub fn devices(&self) -> (Vec<String>, Vec<String>) {
        let (reply, answer) = mpsc::channel();
        let _ = lock(&self.commands).send(Command::Outputs(reply));
        answer.recv().unwrap_or_default()
    }

    /// Play `clip` as `key`. Returns whether it is playing afterwards — a
    /// toggle of a playing clip stops it.
    pub fn play(&self, key: &str, clip: &Clip, gain: f32, mode: Mode, output: Output) -> Result<bool, String> {
        let opened = self.opened();
        if matches!(output, Output::Cable) && opened.cable.is_none() {
            return Err(NO_CABLE.into());
        }
        if matches!(output, Output::Monitor) && opened.monitor.is_none() {
            return Err("no headphones or speakers are open; check the monitor device".into());
        }
        if opened.monitor.is_none() && opened.cable.is_none() {
            return Err("no output device is open".into());
        }
        if mode == Mode::Toggle && self.is_playing(key) {
            self.stop(key);
            return Ok(false);
        }
        let targets = [
            (opened.monitor.is_some() && output != Output::Cable, &self.monitor),
            (opened.cable.is_some() && output != Output::Monitor, &self.cable),
        ];
        for (wanted, mixer) in targets {
            if wanted {
                let mut mixer = lock(mixer);
                if mode == Mode::Restart {
                    mixer.stop(key);
                }
                mixer.play(key, clip, gain);
            }
        }
        Ok(true)
    }

    pub fn stop(&self, key: &str) -> bool {
        let a = lock(&self.monitor).stop(key);
        let b = lock(&self.cable).stop(key);
        a || b
    }

    pub fn stop_all(&self) {
        lock(&self.monitor).stop_all();
        lock(&self.cable).stop_all();
    }

    pub fn is_playing(&self, key: &str) -> bool {
        lock(&self.monitor).is_playing(key) || lock(&self.cable).is_playing(key)
    }

    /// Every key playing on either output.
    pub fn playing(&self) -> Vec<String> {
        let mut keys = lock(&self.monitor).playing();
        keys.extend(lock(&self.cable).playing());
        keys.sort();
        keys.dedup();
        keys
    }

    pub fn set_master(&self, gain: f32) {
        lock(&self.monitor).master = gain;
        lock(&self.cable).master = gain;
    }

    pub fn set_clip_gain(&self, key: &str, gain: f32) {
        lock(&self.monitor).set_gain(key, gain);
        lock(&self.cable).set_gain(key, gain);
    }

    pub fn set_mic_gain(&self, gain: f32) {
        lock(&self.cable).passthrough_gain = gain;
    }

    /// For tests: render `frames` of each output.
    #[cfg(test)]
    pub fn render(&self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut monitor = vec![0.0; frames * 2];
        let mut cable = vec![0.0; frames * 2];
        lock(&self.monitor).render(&mut monitor, 2);
        lock(&self.cable).render(&mut cable, 2);
        (monitor, cable)
    }
}

/// What to say when a clip is meant for the cable and there is none.
pub const NO_CABLE: &str = "no virtual audio cable was found. Install VB-CABLE (vb-audio.com/Cable), \
     then choose \"CABLE Output\" as your microphone in Discord or OBS — see the plugin's README";

/// Devices that open nothing and play nowhere. The mixers still work, and a
/// test drives them with [`Engine::render`].
#[derive(Debug, Default)]
pub struct NullDevices {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    /// Every configuration it was asked to open, in order.
    pub log: Arc<Mutex<Vec<Config>>>,
}

impl Devices for NullDevices {
    fn outputs(&self) -> Vec<String> {
        self.outputs.clone()
    }

    fn inputs(&self) -> Vec<String> {
        self.inputs.clone()
    }

    fn open(
        &mut self,
        config: &Config,
        monitor: &Arc<Mutex<Mixer>>,
        cable: &Arc<Mutex<Mixer>>,
        mic: &Arc<Mutex<DriftBuffer>>,
    ) -> Opened {
        lock(&self.log).push(config.clone());
        let mut opened = Opened::default();
        opened.monitor = match &config.monitor {
            Some(name) => pick(&self.outputs, name),
            None => self.outputs.first().cloned(),
        };
        opened.cable = match &config.cable {
            Some(name) => pick(&self.outputs, name),
            None => find_cable(&self.outputs),
        };
        if config.cable.is_some() && opened.cable.is_none() {
            opened.problems.push("the chosen cable is not connected".into());
        }
        let mut cable = lock(cable);
        cable.passthrough = None;
        if config.passthrough && opened.cable.is_some() {
            opened.mic = match &config.mic {
                Some(name) => pick(&self.inputs, name),
                None => self.inputs.first().cloned(),
            };
            if opened.mic.is_some() {
                cable.passthrough = Some(Arc::clone(mic));
            }
        }
        lock(monitor).rate = 48_000;
        cable.rate = 48_000;
        opened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(outputs: &[&str]) -> Engine {
        let outputs: Vec<String> = outputs.iter().map(|name| name.to_string()).collect();
        let engine = Engine::new(move || {
            Box::new(NullDevices {
                outputs,
                inputs: vec!["Microphone (Yeti)".into()],
                ..NullDevices::default()
            })
        });
        engine.configure(Config::default());
        engine
    }

    fn beep(frames: usize) -> Clip {
        Clip::new(48_000, vec![0.25; frames * 2])
    }

    #[test]
    fn a_virtual_cable_is_found_by_the_names_cables_use() {
        let outputs = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        assert_eq!(
            find_cable(&outputs(&["Speakers", "CABLE Input (VB-Audio Virtual Cable)"])),
            Some("CABLE Input (VB-Audio Virtual Cable)".into())
        );
        assert_eq!(
            find_cable(&outputs(&["VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)"])),
            Some("VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)".into())
        );
        assert_eq!(find_cable(&outputs(&["Speakers", "Headphones"])), None);
    }

    #[test]
    fn a_clip_plays_to_both_outputs_and_each_hears_it() {
        let engine = engine(&["Headphones", "CABLE Input (VB-Audio Virtual Cable)"]);
        assert!(engine.play("airhorn", &beep(4), 1.0, Mode::Restart, Output::Both).unwrap());
        let (monitor, cable) = engine.render(2);
        assert_eq!(monitor, vec![0.25; 4]);
        assert_eq!(cable, vec![0.25; 4]);
        assert_eq!(engine.playing(), ["airhorn"]);
    }

    #[test]
    fn a_clip_for_the_cable_without_a_cable_says_how_to_get_one() {
        let engine = engine(&["Headphones"]);
        let error = engine.play("x", &beep(4), 1.0, Mode::Restart, Output::Cable).unwrap_err();
        assert!(error.contains("VB-CABLE"), "{error}");
        // "Both" still plays where it can: the monitor.
        assert!(engine.play("x", &beep(4), 1.0, Mode::Restart, Output::Both).unwrap());
    }

    #[test]
    fn restart_overlap_and_toggle_do_what_they_say() {
        let engine = engine(&["Headphones"]);
        engine.play("a", &beep(8), 1.0, Mode::Restart, Output::Both).unwrap();
        engine.render(4);
        engine.play("a", &beep(8), 1.0, Mode::Restart, Output::Both).unwrap();
        let (monitor, _) = engine.render(1);
        assert_eq!(monitor, vec![0.25; 2], "restarted: one voice, not two");

        engine.play("a", &beep(8), 1.0, Mode::Overlap, Output::Both).unwrap();
        let (monitor, _) = engine.render(1);
        assert_eq!(monitor, vec![0.5; 2], "overlapped: two voices");

        assert!(!engine.play("a", &beep(8), 1.0, Mode::Toggle, Output::Both).unwrap(), "toggled off");
        assert!(engine.playing().is_empty());
        assert!(engine.play("a", &beep(8), 1.0, Mode::Toggle, Output::Both).unwrap(), "toggled on");
    }

    #[test]
    fn the_microphone_is_mixed_into_the_cable_only() {
        let devices = NullDevices {
            outputs: vec!["Headphones".into(), "CABLE Input (VB-Audio Virtual Cable)".into()],
            inputs: vec!["Microphone (Yeti)".into()],
            ..NullDevices::default()
        };
        let engine = Engine::new(move || Box::new(devices));
        let opened = engine.configure(Config {
            passthrough: true,
            ..Config::default()
        });
        assert_eq!(opened.mic.as_deref(), Some("Microphone (Yeti)"));
        assert!(lock(&engine.cable).passthrough.is_some());
        assert!(lock(&engine.monitor).passthrough.is_none(), "never into your own ears");
    }

    #[test]
    fn master_and_clip_gain_scale_what_is_heard() {
        let engine = engine(&["Headphones"]);
        engine.play("a", &beep(8), 1.0, Mode::Restart, Output::Both).unwrap();
        engine.set_master(0.5);
        engine.set_clip_gain("a", 0.5);
        let (monitor, _) = engine.render(1);
        assert!((monitor[0] - 0.0625).abs() < 1e-6, "{monitor:?}");
    }
}

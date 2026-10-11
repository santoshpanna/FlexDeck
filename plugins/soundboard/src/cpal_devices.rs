//! The real sound cards, through cpal (WASAPI on Windows).
//!
//! Lives on the engine's audio thread and nowhere else: it owns the streams,
//! and dropping a stream is what stops it.

use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream};

use crate::engine::{find_cable, pick, Config, Devices, Opened};
use crate::mix::{DriftBuffer, Mixer, StreamResampler};

pub struct CpalDevices {
    host: cpal::Host,
    streams: Vec<Stream>,
}

impl CpalDevices {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
            streams: Vec::new(),
        }
    }

    fn output_devices(&self) -> Vec<Device> {
        self.host
            .output_devices()
            .map(|devices| devices.collect())
            .unwrap_or_default()
    }

    fn input_devices(&self) -> Vec<Device> {
        self.host
            .input_devices()
            .map(|devices| devices.collect())
            .unwrap_or_default()
    }
}

fn names(devices: &[Device]) -> Vec<String> {
    devices.iter().filter_map(|device| device.name().ok()).collect()
}

fn named(devices: Vec<Device>, name: &str) -> Option<Device> {
    devices
        .into_iter()
        .find(|device| device.name().map(|candidate| candidate == name).unwrap_or(false))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Open `device` for playback from `mixer`, at the device's own rate.
fn open_output(device: &Device, mixer: &Arc<Mutex<Mixer>>) -> Result<Stream, String> {
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let channels = usize::from(supported.channels());
    let rate = supported.sample_rate().0;
    {
        let mut mixer = lock(mixer);
        // Voices were resampled for the old rate; a new device starts clean.
        if mixer.rate != rate {
            mixer.stop_all();
        }
        mixer.rate = rate;
    }
    let config = supported.config();
    let report = |error: cpal::StreamError| eprintln!("soundboard: output stream: {error}");
    let mixer = Arc::clone(mixer);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _| lock(&mixer).render(data, channels),
            report,
            None,
        ),
        SampleFormat::I16 => {
            let mut scratch: Vec<f32> = Vec::new();
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    scratch.resize(data.len(), 0.0);
                    lock(&mixer).render(&mut scratch, channels);
                    for (out, sample) in data.iter_mut().zip(&scratch) {
                        *out = (sample * f32::from(i16::MAX)) as i16;
                    }
                },
                report,
                None,
            )
        }
        other => return Err(format!("it plays {other:?} samples, which this cannot produce")),
    }
    .map_err(|error| error.to_string())?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

/// Open `device` for capture into `buffer`, resampled to `rate`.
fn open_input(device: &Device, buffer: &Arc<Mutex<DriftBuffer>>, rate: u32) -> Result<Stream, String> {
    let supported = device
        .default_input_config()
        .map_err(|error| error.to_string())?;
    let channels = usize::from(supported.channels()).max(1);
    let mut resampler = StreamResampler::new(supported.sample_rate().0, rate);
    *lock(buffer) = DriftBuffer::new((rate / 50) as usize, (rate / 100) as usize);
    let buffer = Arc::clone(buffer);
    let config = supported.config();
    let report = |error: cpal::StreamError| eprintln!("soundboard: microphone stream: {error}");
    let mut frames: Vec<(f32, f32)> = Vec::new();
    let mut out: Vec<(f32, f32)> = Vec::new();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                frames.clear();
                // A microphone is mono in practice; its first channel goes to both sides.
                frames.extend(data.chunks(channels).map(|frame| (frame[0], frame[0])));
                out.clear();
                resampler.push(&frames, &mut out);
                lock(&buffer).push(out.drain(..));
            },
            report,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                frames.clear();
                frames.extend(data.chunks(channels).map(|frame| {
                    let sample = f32::from(frame[0]) / f32::from(i16::MAX);
                    (sample, sample)
                }));
                out.clear();
                resampler.push(&frames, &mut out);
                lock(&buffer).push(out.drain(..));
            },
            report,
            None,
        ),
        other => return Err(format!("it records {other:?} samples, which this cannot read")),
    }
    .map_err(|error| error.to_string())?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

impl Devices for CpalDevices {
    fn outputs(&self) -> Vec<String> {
        names(&self.output_devices())
    }

    fn inputs(&self) -> Vec<String> {
        names(&self.input_devices())
    }

    fn open(
        &mut self,
        config: &Config,
        monitor: &Arc<Mutex<Mixer>>,
        cable: &Arc<Mutex<Mixer>>,
        mic: &Arc<Mutex<DriftBuffer>>,
    ) -> Opened {
        // Dropping a stream stops it; everything is reopened from scratch.
        self.streams.clear();
        lock(cable).passthrough = None;
        let mut opened = Opened::default();
        let outputs = self.outputs();

        let monitor_device = match &config.monitor {
            Some(wanted) => pick(&outputs, wanted).and_then(|name| named(self.output_devices(), &name)),
            None => self.host.default_output_device(),
        };
        let cable_name = match &config.cable {
            Some(wanted) => pick(&outputs, wanted),
            None => find_cable(&outputs),
        };

        match monitor_device {
            Some(device) => {
                let name = device.name().unwrap_or_default();
                // The cable is not also your headphones.
                if Some(&name) == cable_name.as_ref() {
                    opened.problems.push(format!("{name} is the cable, so it is not also used to listen"));
                } else {
                    match open_output(&device, monitor) {
                        Ok(stream) => {
                            self.streams.push(stream);
                            opened.monitor = Some(name);
                        }
                        Err(error) => opened.problems.push(format!("{name} could not be opened: {error}")),
                    }
                }
            }
            None => opened.problems.push(match &config.monitor {
                Some(wanted) => format!("no output device matches '{wanted}'"),
                None => "there is no default output device".into(),
            }),
        }

        if let Some(name) = cable_name {
            match named(self.output_devices(), &name).map(|device| open_output(&device, cable)) {
                Some(Ok(stream)) => {
                    self.streams.push(stream);
                    opened.cable = Some(name);
                }
                Some(Err(error)) => opened.problems.push(format!("{name} could not be opened: {error}")),
                None => opened.problems.push(format!("{name} went away")),
            }
        } else if let Some(wanted) = &config.cable {
            opened.problems.push(format!("no output device matches '{wanted}'"));
        }

        if config.passthrough {
            if opened.cable.is_none() {
                opened.problems.push("mic passthrough needs a virtual cable".into());
            } else {
                let inputs = self.input_devices();
                let device = match &config.mic {
                    Some(wanted) => pick(&names(&inputs), wanted).and_then(|name| named(inputs, &name)),
                    None => self.host.default_input_device(),
                };
                match device {
                    Some(device) => {
                        let name = device.name().unwrap_or_default();
                        let rate = lock(cable).rate;
                        match open_input(&device, mic, rate) {
                            Ok(stream) => {
                                self.streams.push(stream);
                                lock(cable).passthrough = Some(Arc::clone(mic));
                                opened.mic = Some(name);
                            }
                            Err(error) => opened.problems.push(format!("{name} could not be opened: {error}")),
                        }
                    }
                    None => opened.problems.push("no microphone was found".into()),
                }
            }
        }
        opened
    }
}

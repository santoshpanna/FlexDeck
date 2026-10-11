//! The arithmetic of a soundboard: clips, voices, mixing, resampling, and
//! keeping a microphone in step with a second sound card.
//!
//! Nothing here touches a device, so all of it is tested exactly. The audio
//! callbacks in `engine.rs` only call [`Mixer::render`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// A decoded sound: stereo, interleaved, at its own sample rate. Resampled
/// copies for each device rate are made once and kept.
#[derive(Debug)]
pub struct Clip {
    pub rate: u32,
    /// `[l, r, l, r, …]`.
    pub samples: Arc<Vec<f32>>,
    resampled: Mutex<HashMap<u32, Arc<Vec<f32>>>>,
}

impl Clip {
    pub fn new(rate: u32, samples: Vec<f32>) -> Self {
        Self {
            rate,
            samples: Arc::new(samples),
            resampled: Mutex::new(HashMap::new()),
        }
    }

    /// Bytes held, for the cache's budget.
    pub fn size(&self) -> usize {
        let resampled: usize = self
            .resampled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|samples| samples.len())
            .sum();
        (self.samples.len() + resampled) * std::mem::size_of::<f32>()
    }

    /// The samples at `rate`, made once.
    pub fn at_rate(&self, rate: u32) -> Arc<Vec<f32>> {
        if rate == self.rate {
            return Arc::clone(&self.samples);
        }
        let mut resampled = self.resampled.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(
            resampled
                .entry(rate)
                .or_insert_with(|| Arc::new(resample_stereo(&self.samples, self.rate, rate))),
        )
    }
}

/// Linear-interpolation resampling of interleaved stereo.
///
/// Linear is audibly fine for sound effects and voice at the rates sound cards
/// use (44.1 and 48 kHz), and needs no filter state; a soundboard is not a
/// mastering suite.
pub fn resample_stereo(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    let frames = samples.len() / 2;
    if from == to || frames == 0 || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let out_frames = ((frames as u64 * u64::from(to)) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    let mut out = Vec::with_capacity(out_frames * 2);
    for frame in 0..out_frames {
        let position = frame as f64 * step;
        let index = position.floor() as usize;
        let fraction = (position - index as f64) as f32;
        let next = (index + 1).min(frames - 1);
        for channel in 0..2 {
            let a = samples[index.min(frames - 1) * 2 + channel];
            let b = samples[next * 2 + channel];
            out.push(a + (b - a) * fraction);
        }
    }
    out
}

/// Keep a sum inside [-1, 1] without the crack of a hard clip: untouched below
/// 0.8, then bent smoothly toward 1.
pub fn soft_clip(sample: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let magnitude = sample.abs();
    if magnitude <= KNEE {
        return sample;
    }
    let over = (magnitude - KNEE) / (1.0 - KNEE);
    let bent = KNEE + (1.0 - KNEE) * (over / (1.0 + over));
    bent.copysign(sample)
}

/// One sound playing on one output.
#[derive(Debug)]
struct Voice {
    key: String,
    samples: Arc<Vec<f32>>,
    position: usize,
    gain: f32,
}

/// Everything playing on one device, at that device's rate.
#[derive(Debug)]
pub struct Mixer {
    pub rate: u32,
    voices: Vec<Voice>,
    pub master: f32,
    /// Microphone audio to mix in (the cable output only), already at `rate`.
    pub passthrough: Option<Arc<Mutex<DriftBuffer>>>,
    pub passthrough_gain: f32,
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            voices: Vec::new(),
            master: 1.0,
            passthrough: None,
            passthrough_gain: 1.0,
        }
    }

    pub fn play(&mut self, key: &str, clip: &Clip, gain: f32) {
        self.voices.push(Voice {
            key: key.to_string(),
            samples: clip.at_rate(self.rate),
            position: 0,
            gain,
        });
    }

    /// Stop every voice playing `key`. Returns whether any was.
    pub fn stop(&mut self, key: &str) -> bool {
        let before = self.voices.len();
        self.voices.retain(|voice| voice.key != key);
        before != self.voices.len()
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    /// Set the gain of every voice playing `key` (a clip's own slider).
    pub fn set_gain(&mut self, key: &str, gain: f32) {
        for voice in self.voices.iter_mut().filter(|voice| voice.key == key) {
            voice.gain = gain;
        }
    }

    pub fn is_playing(&self, key: &str) -> bool {
        self.voices.iter().any(|voice| voice.key == key)
    }

    /// The keys playing now, each once.
    pub fn playing(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.voices.iter().map(|voice| voice.key.clone()).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Fill `out` (interleaved, `channels` per frame) with the next stretch of
    /// every voice, the microphone, master gain, and a soft clip. Finished
    /// voices are dropped.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        if channels == 0 {
            return;
        }
        let frames = out.len() / channels;
        for voice in &mut self.voices {
            let available = (voice.samples.len() - voice.position) / 2;
            let count = available.min(frames);
            for frame in 0..count {
                let left = voice.samples[voice.position + frame * 2] * voice.gain;
                let right = voice.samples[voice.position + frame * 2 + 1] * voice.gain;
                mix_frame(&mut out[frame * channels..(frame + 1) * channels], left, right);
            }
            voice.position += count * 2;
        }
        self.voices.retain(|voice| voice.position < voice.samples.len());

        if let Some(passthrough) = &self.passthrough {
            let mut buffer = passthrough.lock().unwrap_or_else(PoisonError::into_inner);
            for frame in 0..frames {
                let (left, right) = buffer.pull();
                mix_frame(
                    &mut out[frame * channels..(frame + 1) * channels],
                    left * self.passthrough_gain,
                    right * self.passthrough_gain,
                );
            }
        }

        for sample in out.iter_mut() {
            *sample = soft_clip(*sample * self.master);
        }
    }
}

/// Add a stereo frame to a device frame of any width: mono gets the average,
/// stereo gets both, and wider devices get front left and right.
fn mix_frame(frame: &mut [f32], left: f32, right: f32) {
    match frame.len() {
        0 => {}
        1 => frame[0] += (left + right) * 0.5,
        _ => {
            frame[0] += left;
            frame[1] += right;
        }
    }
}

/// Resampling a live stream, a callback's worth at a time, keeping its place
/// between calls so chunk edges do not click.
#[derive(Debug)]
pub struct StreamResampler {
    step: f64,
    /// Where the next output frame falls, measured from `previous`.
    position: f64,
    previous: (f32, f32),
}

impl StreamResampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            step: f64::from(from.max(1)) / f64::from(to.max(1)),
            position: 0.0,
            previous: (0.0, 0.0),
        }
    }

    /// Resample `input` stereo frames, appending to `out`.
    pub fn push(&mut self, input: &[(f32, f32)], out: &mut Vec<(f32, f32)>) {
        for &next in input {
            // Every output frame between previous and next.
            while self.position < 1.0 {
                let t = self.position as f32;
                out.push((
                    self.previous.0 + (next.0 - self.previous.0) * t,
                    self.previous.1 + (next.1 - self.previous.1) * t,
                ));
                self.position += self.step;
            }
            self.position -= 1.0;
            self.previous = next;
        }
    }
}

/// Microphone audio on its way to another sound card.
///
/// Two sound cards never run at exactly the same speed, so a buffer between
/// them slowly fills or drains. This one aims to hold about `target` frames:
/// past `target + band` it drops a frame, under `target - band` it repeats one.
/// At most one correction per `every` frames — one in hundreds is inaudible,
/// where a click every few seconds from running dry or overflowing is not —
/// and because it is paced, a microphone that stops simply drains to silence
/// instead of one frame being repeated into a tone.
#[derive(Debug)]
pub struct DriftBuffer {
    frames: std::collections::VecDeque<(f32, f32)>,
    target: usize,
    band: usize,
    every: usize,
    since_correction: usize,
    last: (f32, f32),
    /// Waiting to reach `target` before playing, after starting or running dry.
    priming: bool,
}

/// How many frames apart two corrections may be, by default.
const CORRECT_EVERY: usize = 300;

impl DriftBuffer {
    /// `target` and `band` in frames: at 48 kHz, 960 and 480 are 20 ms ± 10 ms.
    pub fn new(target: usize, band: usize) -> Self {
        Self::paced(target, band, CORRECT_EVERY)
    }

    /// As [`DriftBuffer::new`], correcting at most once per `every` frames.
    pub fn paced(target: usize, band: usize, every: usize) -> Self {
        Self {
            frames: std::collections::VecDeque::with_capacity(target * 4),
            target,
            band,
            every: every.max(1),
            since_correction: 0,
            last: (0.0, 0.0),
            priming: true,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Add microphone frames (already at the output's rate).
    pub fn push(&mut self, frames: impl IntoIterator<Item = (f32, f32)>) {
        self.frames.extend(frames);
        // Never let a stalled output grow it without bound.
        let ceiling = self.target * 4;
        while self.frames.len() > ceiling {
            self.frames.pop_front();
        }
        if self.priming && self.frames.len() >= self.target {
            self.priming = false;
        }
    }

    /// The next frame for the output.
    pub fn pull(&mut self) -> (f32, f32) {
        if self.priming {
            return (0.0, 0.0);
        }
        self.since_correction += 1;
        let may_correct = self.since_correction >= self.every;
        if may_correct && self.frames.len() > self.target + self.band {
            // Running ahead: skip one.
            self.since_correction = 0;
            self.frames.pop_front();
        } else if may_correct
            && self.frames.len() < self.target.saturating_sub(self.band)
            && !self.frames.is_empty()
        {
            // Running behind: hold one back for a frame.
            self.since_correction = 0;
            return self.last;
        }
        match self.frames.pop_front() {
            Some(frame) => {
                self.last = frame;
                frame
            }
            None => {
                // Dry: silence, and wait to refill rather than stutter.
                self.priming = true;
                (0.0, 0.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> Vec<f32> {
        (0..frames).flat_map(|i| [i as f32 / 1000.0, -(i as f32) / 1000.0]).collect()
    }

    #[test]
    fn resampling_keeps_the_duration_and_the_shape() {
        let source = ramp(441);
        let out = resample_stereo(&source, 44_100, 48_000);
        assert_eq!(out.len() / 2, 480, "10 ms stays 10 ms");
        // Interpolated, so a straight ramp stays straight.
        let mid = out[240 * 2];
        assert!((mid - 0.2205).abs() < 0.001, "{mid}");
        assert_eq!(resample_stereo(&source, 48_000, 48_000), source);
    }

    #[test]
    fn a_resampled_copy_is_made_once_per_rate() {
        let clip = Clip::new(44_100, ramp(441));
        let first = clip.at_rate(48_000);
        let second = clip.at_rate(48_000);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&clip.at_rate(44_100), &clip.samples));
        assert!(clip.size() > ramp(441).len() * 4, "the copy counts against the budget");
    }

    #[test]
    fn voices_sum_with_their_gain_and_finished_ones_go() {
        let loud = Clip::new(48_000, vec![0.5; 8]); // 4 frames
        let quiet = Clip::new(48_000, vec![0.25; 4]); // 2 frames
        let mut mixer = Mixer::new(48_000);
        mixer.play("a", &loud, 1.0);
        mixer.play("b", &quiet, 0.5);
        let mut out = vec![0.0; 6]; // 3 stereo frames
        mixer.render(&mut out, 2);
        assert_eq!(&out[..4], &[0.625; 4], "both, with b at half");
        assert_eq!(&out[4..], &[0.5; 2], "b ran out");
        assert_eq!(mixer.playing(), ["a"]);
        mixer.render(&mut out, 2);
        assert_eq!(&out[..2], &[0.5; 2]);
        assert!(mixer.playing().is_empty(), "a finished too");
    }

    #[test]
    fn stop_and_master_and_mono_devices() {
        let clip = Clip::new(48_000, vec![0.4, 0.2, 0.4, 0.2]);
        let mut mixer = Mixer::new(48_000);
        mixer.play("x", &clip, 1.0);
        mixer.master = 0.5;
        // One frame of a two-frame clip, so it is still playing to be stopped.
        let mut mono = vec![0.0; 1];
        mixer.render(&mut mono, 1);
        assert!((mono[0] - 0.15).abs() < 1e-6, "average of 0.4 and 0.2, halved");
        assert!(mixer.stop("x"));
        assert!(!mixer.stop("x"));
    }

    #[test]
    fn a_loud_sum_is_bent_back_under_full_scale_without_flipping() {
        assert_eq!(soft_clip(0.5), 0.5, "quiet is untouched");
        for loud in [0.9_f32, 1.5, 4.0, 100.0] {
            let bent = soft_clip(loud);
            assert!(bent < 1.0 && bent > 0.8, "{loud} -> {bent}");
            assert_eq!(soft_clip(-loud), -bent);
        }
        assert!(soft_clip(4.0) > soft_clip(1.5), "still monotonic");
    }

    #[test]
    fn the_drift_buffer_waits_to_fill_then_holds_its_level() {
        let mut buffer = DriftBuffer::paced(10, 4, 1);
        buffer.push((0..5).map(|i| (i as f32, 0.0)));
        assert_eq!(buffer.pull(), (0.0, 0.0), "priming: silence until 10 are in");
        buffer.push((5..12).map(|i| (i as f32, 0.0)));
        assert_eq!(buffer.pull().0, 0.0, "the first real frame");

        // Running ahead: frames are skipped until it is back in the band.
        buffer.push((12..40).map(|i| (i as f32, 0.0)));
        let before = buffer.len();
        buffer.pull();
        assert_eq!(buffer.len(), before - 2, "one played, one dropped");

        // Running behind: a frame is held rather than draining to a click.
        let mut slow = DriftBuffer::paced(10, 4, 1);
        slow.push((0..10).map(|i| (i as f32, 0.0)));
        for _ in 0..5 {
            slow.pull();
        }
        let held = slow.pull();
        assert_eq!(slow.pull(), held, "repeated while under the band");
    }

    #[test]
    fn a_dry_buffer_goes_quiet_and_primes_again() {
        let mut buffer = DriftBuffer::paced(2, 0, 1000);
        buffer.push([(1.0, 1.0), (2.0, 2.0)]);
        assert_eq!(buffer.pull(), (1.0, 1.0));
        assert_eq!(buffer.pull(), (2.0, 2.0), "paced: no repeat this soon");
        assert_eq!(buffer.pull(), (0.0, 0.0), "dry");
        buffer.push([(3.0, 3.0)]);
        assert_eq!(buffer.pull(), (0.0, 0.0), "still priming");
        buffer.push([(4.0, 4.0)]);
        assert_eq!(buffer.pull(), (3.0, 3.0));
    }

    #[test]
    fn a_microphone_that_stops_drains_to_silence_rather_than_a_tone() {
        let mut buffer = DriftBuffer::paced(100, 10, 50);
        buffer.push((0..100).map(|i| (i as f32 + 1.0, 0.0)));
        let mut heard = Vec::new();
        for _ in 0..400 {
            heard.push(buffer.pull().0);
        }
        let repeats = heard.windows(2).filter(|pair| pair[0] == pair[1] && pair[0] != 0.0).count();
        assert!(repeats <= 3, "{repeats} repeated frames");
        assert_eq!(heard.last(), Some(&0.0), "silent once drained");
    }

    #[test]
    fn a_live_stream_resampled_in_chunks_matches_its_length_and_stays_smooth() {
        let mut resampler = StreamResampler::new(44_100, 48_000);
        let mut out = Vec::new();
        let ramp: Vec<(f32, f32)> = (0..4410).map(|i| (i as f32, 0.0)).collect();
        for chunk in ramp.chunks(441) {
            resampler.push(chunk, &mut out);
        }
        let expected = 4410.0 * 48_000.0 / 44_100.0;
        assert!((out.len() as f64 - expected).abs() <= 2.0, "{} vs {expected}", out.len());
        let jumps = out.windows(2).map(|pair| pair[1].0 - pair[0].0).fold(0.0_f32, f32::max);
        assert!(jumps <= 1.0, "no jump bigger than one input step across chunk edges: {jumps}");
    }

    #[test]
    fn the_microphone_is_mixed_in_at_its_own_gain() {
        let buffer = Arc::new(Mutex::new(DriftBuffer::new(1, 0)));
        buffer.lock().unwrap().push([(0.2, 0.2), (0.2, 0.2)]);
        let mut mixer = Mixer::new(48_000);
        mixer.passthrough = Some(Arc::clone(&buffer));
        mixer.passthrough_gain = 2.0;
        let mut out = vec![0.0; 2];
        mixer.render(&mut out, 2);
        assert!((out[0] - 0.4).abs() < 1e-6, "{out:?}");
    }
}

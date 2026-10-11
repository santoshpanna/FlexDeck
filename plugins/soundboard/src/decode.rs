//! Reading a sound file into a [`Clip`]: MP3, WAV, FLAC, Ogg Vorbis, and
//! AAC/M4A, through symphonia.

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::mix::Clip;

/// The longest sound kept: a soundboard plays clips, not albums, and a
/// ten-minute file decoded to floats is a quarter of a gigabyte.
pub const MAX_SECONDS: u64 = 300;

/// Decode `bytes` (a whole file) into a stereo clip. `name` is only a hint at
/// the format, from its extension.
pub fn decode(bytes: Vec<u8>, name: &str) -> Result<Clip, String> {
    let mut hint = Hint::new();
    if let Some(extension) = name.rsplit('.').next().filter(|ext| ext.len() <= 5) {
        hint.with_extension(extension);
    }
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|error| format!("{name} is not a sound this can play ({error})"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| format!("{name} has no sound in it"))?;
    let track_id = track.id;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| format!("{name} does not say its sample rate"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| format!("{name} uses a codec this cannot play ({error})"))?;

    let limit = (MAX_SECONDS * u64::from(rate) * 2) as usize;
    let mut samples: Vec<f32> = Vec::new();
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(error) => return Err(format!("{name} could not be read to the end ({error})")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged frame is skipped, as every player does.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(format!("{name} could not be decoded ({error})")),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let target = buffer.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, spec));
        if target.capacity() < decoded.capacity() * channels {
            *target = SampleBuffer::new(decoded.capacity() as u64, spec);
        }
        target.copy_interleaved_ref(decoded);
        for frame in target.samples().chunks(channels) {
            let left = frame[0];
            let right = if channels > 1 { frame[1] } else { frame[0] };
            samples.push(left);
            samples.push(right);
        }
        if samples.len() >= limit {
            samples.truncate(limit);
            break;
        }
    }
    if samples.is_empty() {
        return Err(format!("{name} is silent or empty"));
    }
    Ok(Clip::new(rate, samples))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny mono 16-bit WAV, written by hand so the test needs no files.
    pub(crate) fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn a_mono_wav_becomes_a_stereo_clip_at_its_own_rate() {
        let clip = decode(wav(22_050, &[0, 16_384, -16_384, 32_767]), "beep.wav").expect("decodes");
        assert_eq!(clip.rate, 22_050);
        assert_eq!(clip.samples.len(), 8, "four frames, two channels each");
        assert!((clip.samples[2] - 0.5).abs() < 0.001, "{:?}", clip.samples);
        assert_eq!(clip.samples[2], clip.samples[3], "mono is copied to both sides");
    }

    #[test]
    fn something_that_is_not_sound_is_refused_with_its_name() {
        let error = decode(b"definitely not audio".to_vec(), "notes.txt").unwrap_err();
        assert!(error.contains("notes.txt"), "{error}");
    }
}

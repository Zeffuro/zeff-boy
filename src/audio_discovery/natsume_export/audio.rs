use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

use super::NatsumeExportRequest;
use crate::audio_discovery::{
    audio_file::{self, AudioInfo},
    formats::{AudioFormat, BankSelect},
    natsume::preview::NativeRenderSession,
    render::{
        MAX_DURATION_SECONDS, MAX_FADE_SECONDS, PlaybackGain, RenderOptions, validate_sample_rate,
    },
};

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Options {
    sample_rate: u32,
    duration_seconds: u16,
    fade_seconds: u8,
}

impl Options {
    pub(super) fn from_render(options: RenderOptions) -> Result<Self> {
        validate_sample_rate(options.sample_rate)?;
        ensure!(
            (1..=MAX_DURATION_SECONDS).contains(&options.max_seconds),
            "recording duration must be between 1 and {MAX_DURATION_SECONDS} seconds"
        );
        ensure!(
            options.fade_seconds <= MAX_FADE_SECONDS
                && u16::from(options.fade_seconds) <= options.max_seconds,
            "fade must be at most {MAX_FADE_SECONDS} seconds and fit within the recording duration"
        );
        ensure!(
            options.loops == 1,
            "Natsume audio uses a duration, not loop passes"
        );
        ensure!(
            options.playback_gain == PlaybackGain::Raw
                && options.skip_channel10
                && options.bank_select == BankSelect::Gs,
            "MP2k gain and MIDI controls do not apply to original-driver audio"
        );
        let options = Self {
            sample_rate: options.sample_rate,
            duration_seconds: options.max_seconds,
            fade_seconds: options.fade_seconds,
        };
        options.info().validate()?;
        Ok(options)
    }

    fn info(self) -> AudioInfo {
        AudioInfo {
            frames: u64::from(self.duration_seconds) * u64::from(self.sample_rate),
            channels: 2,
            sample_rate: self.sample_rate,
            loop_range: None,
            pitch: None,
        }
    }
}

pub(super) fn write_new(
    mut request: NatsumeExportRequest,
    format: AudioFormat,
    options: Options,
    path: &Path,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<()> {
    request.metadata["schema"] = json!("zeff-natsume-audio-export/1");
    request.metadata["kind"] = json!("original_driver_audio");
    request.metadata["audio_format"] = json!(format.extension());
    request.metadata["recording_options"] = serde_json::to_value(options)?;
    request.metadata["frames"] = json!(options.info().frames);
    request.metadata["limitations"] = json!([
        "Runs the authenticated original sound driver in an isolated GBA emulator. Hardware-bit-exact output is not claimed.",
        "Records the requested duration without automatic song-end or loop detection. The optional fade occupies the end of that duration.",
        "Instrument-bank and MIDI conversion remain unavailable. Ogg Vorbis compression is lossy."
    ]);
    let renderer = NativeRenderSession::new(
        &request.bytes,
        &request.song,
        options.sample_rate,
        u32::from(options.duration_seconds),
        cancel,
    )?;
    write_renderer(
        renderer,
        Recording {
            format,
            options,
            metadata: request.metadata,
        },
        path,
        cancel,
        progress,
    )
}

struct Recording {
    format: AudioFormat,
    options: Options,
    metadata: Value,
}

fn write_renderer(
    mut renderer: NativeRenderSession,
    recording: Recording,
    path: &Path,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<()> {
    let Recording {
        format,
        options,
        metadata,
    } = recording;
    let info = options.info();
    info.validate()?;
    ensure!(
        renderer.sample_rate() == info.sample_rate
            && renderer.duration_frames() as u64 == info.frames
            && renderer.position_frames() == 0,
        "native audio renderer does not match its recording settings"
    );
    let metadata = serde_json::to_vec(&metadata)?;
    crate::platform::write_new_file_atomically_streamed(
        path,
        |output| {
            let mut spool =
                tempfile::tempfile().context("could not create native audio storage")?;
            {
                let mut writer = BufWriter::new(&mut spool);
                let mut pcm = [0; 2048];
                let mut encoded = [0; 4096];
                while renderer.position_frames() < renderer.duration_frames() {
                    check_cancel(cancel)?;
                    let start = renderer.position_frames();
                    let count = renderer.read(&mut pcm, cancel)?;
                    ensure!(
                        count > 0 && count.is_multiple_of(2),
                        "native audio render stopped early"
                    );
                    for (index, sample) in pcm[..count].iter().enumerate() {
                        let sample = faded(*sample, start + index / 2, info.frames, options);
                        encoded[index * 2..index * 2 + 2].copy_from_slice(&sample.to_le_bytes());
                    }
                    writer.write_all(&encoded[..count * 2])?;
                    progress.store(
                        (renderer.position_frames() as u64 * 95 / info.frames) as u32,
                        Ordering::Relaxed,
                    );
                }
                writer.flush()?;
            }
            check_cancel(cancel)?;
            progress.store(97, Ordering::Relaxed);
            audio_file::encode_to(format, &mut spool, info, &metadata, cancel, output)
        },
        || check_cancel(cancel),
    )?;
    progress.store(100, Ordering::Relaxed);
    Ok(())
}

fn faded(sample: i16, frame: usize, total: u64, options: Options) -> i16 {
    let fade = u64::from(options.fade_seconds) * u64::from(options.sample_rate);
    let remaining = total - 1 - frame as u64;
    if fade < 2 || remaining >= fade {
        sample
    } else {
        (i64::from(sample) * remaining as i64 / (fade - 1) as i64) as i16
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "native audio export cancelled"
    );
    Ok(())
}

#[cfg(test)]
mod tests;

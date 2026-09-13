use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{
    audio_file::{self, AudioInfo},
    formats::{AudioFormat, BankSelect},
    render::{
        MAX_DURATION_SECONDS, MAX_FADE_SECONDS, PlaybackGain, RenderOptions, validate_sample_rate,
    },
};

mod gb;
mod gb_banked;
pub(crate) mod gba;
mod nes;
mod sega;
pub(crate) mod song;
#[cfg(test)]
mod tests;
pub(crate) mod tracker;
mod ws;

pub(crate) trait PcmSession: Send {
    fn has_source_duration_limit(&self) -> bool {
        false
    }
    fn duration_frames(&self) -> usize;
    fn position_frames(&self) -> usize;
    fn sample_rate(&self) -> u32;
    fn track_count(&self) -> usize;
    fn warnings(&self) -> &[String];
    fn reset(&mut self) -> Result<()>;
    fn set_track_mask(&mut self, mask: u16) -> Result<()>;
    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize>;
}

pub(crate) fn validate_options(options: RenderOptions) -> Result<()> {
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
        "this player uses a recording duration; loop passes do not apply"
    );
    ensure!(
        options.playback_gain == PlaybackGain::Raw
            && options.skip_channel10
            && options.bank_select == BankSelect::Gs,
        "MP2k gain and MIDI controls do not apply to this player"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_new(
    mut session: Box<dyn PcmSession>,
    format: AudioFormat,
    options: RenderOptions,
    mut metadata: Value,
    path: &Path,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<()> {
    validate_options(options)?;
    let info = AudioInfo {
        frames: session.duration_frames() as u64,
        channels: 2,
        sample_rate: session.sample_rate(),
        loop_range: None,
        pitch: None,
    };
    info.validate()?;
    ensure!(
        (info.frames == u64::from(options.max_seconds) * u64::from(options.sample_rate)
            || (session.has_source_duration_limit()
                && info.frames <= u64::from(options.max_seconds) * u64::from(options.sample_rate)))
            && session.position_frames() == 0
            && info.sample_rate == options.sample_rate,
        "audio session does not match its recording settings"
    );
    metadata["recording_options"] = serde_json::to_value(options)?;
    metadata["frames"] = info.frames.into();
    metadata["playback_warnings"] = serde_json::to_value(session.warnings())?;
    let metadata = serde_json::to_vec(&metadata)?;
    crate::platform::write_new_file_atomically_streamed(
        path,
        |output| {
            let mut spool = tempfile::tempfile().context("could not create audio storage")?;
            {
                let mut writer = BufWriter::new(&mut spool);
                let mut pcm = [0; 2048];
                let mut encoded = [0; 4096];
                while session.position_frames() < session.duration_frames() {
                    check_cancel(cancel)?;
                    let start = session.position_frames();
                    let count = session.read(&mut pcm, cancel)?;
                    ensure!(
                        count > 0 && count.is_multiple_of(2),
                        "audio render stopped early"
                    );
                    ensure!(
                        session.position_frames() == start + count / 2,
                        "audio session position did not advance by the returned frame count"
                    );
                    for (index, sample) in pcm[..count].iter().enumerate() {
                        encoded[index * 2..index * 2 + 2].copy_from_slice(&sample.to_le_bytes());
                    }
                    writer.write_all(&encoded[..count * 2])?;
                    progress.store(
                        (session.position_frames() as u64 * 95 / info.frames) as u32,
                        Ordering::Relaxed,
                    );
                }
                writer.flush()?;
            }
            check_cancel(cancel)?;
            audio_file::encode_to(format, &mut spool, info, &metadata, cancel, output)
        },
        || check_cancel(cancel),
    )?;
    progress.store(100, Ordering::Relaxed);
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "audio playback cancelled");
    Ok(())
}

fn fade(sample: i16, frame: usize, total: usize, rate: u32, seconds: u8) -> i16 {
    let fade_frames = usize::from(seconds) * rate as usize;
    let remaining = total.saturating_sub(frame + 1);
    if fade_frames < 2 || remaining >= fade_frames {
        sample
    } else {
        (i64::from(sample) * remaining as i64 / (fade_frames - 1) as i64) as i16
    }
}

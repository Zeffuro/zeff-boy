use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use serde_json::Value;
use zeff_audio_discovery::huge::{catalog::HugeSong, discovery};

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::{huge_validation, render::RenderOptions};

pub(super) struct HugeSession {
    pcm: Vec<i16>,
    proof: Value,
    options: RenderOptions,
    duration: usize,
    position: usize,
    mask: u16,
    warnings: Vec<String>,
}

impl HugeSession {
    pub(super) fn new(
        bytes: &[u8],
        song: &HugeSong,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        ensure!(
            zeff_firmware::sha256_hex(bytes) == song.source_sha256,
            "hUGE source identity changed"
        );
        let found = discovery::discover(bytes, Default::default(), cancel)
            .map_err(|stop| anyhow::anyhow!("hUGE rebinding stopped: {stop:?}"))?;
        let matching: Vec<_> = found
            .bound
            .iter()
            .filter(|bound| bound.song.descriptor == song.bound.song.descriptor)
            .collect();
        ensure!(
            matching.len() == 1 && matching[0] == &song.bound,
            "hUGE catalog selection changed or is ambiguous"
        );
        let frames = huge_validation::required_frames(song.bound.song.loop_ticks)?;
        ensure!(
            frames == song.validation_frames,
            "hUGE validation contract changed"
        );
        let validated = huge_validation::validate_at_rate(
            bytes,
            u16::try_from(song.bound.song.descriptor.offset)?,
            frames,
            options.sample_rate,
            cancel,
        )?;
        let duration = (validated.pcm.len() / 2)
            .min(usize::from(options.max_seconds) * options.sample_rate as usize);
        ensure!(
            duration > 0,
            "hUGE validation returned no complete audio frames"
        );
        let pcm = validated
            .pcm
            .into_iter()
            .take(duration * 2)
            .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
            .collect();
        check_cancel(cancel)?;
        Ok(Self {
            pcm, proof: validated.report, options, duration, position: 0, mask: 1,
            warnings: vec![
                "Original and isolated hUGEDriver execution passed source, memory, control-state and PCM verification before playback.".into(),
                format!("Plays {:.3} seconds within the verified capture window, capped by the requested {}-second maximum. Audio is not repeated; this is not a seamless-loop or hardware-equivalence claim.", duration as f64 / f64::from(options.sample_rate), options.max_seconds),
            ],
        })
    }
}

impl PcmSession for HugeSession {
    fn runtime_validation(&self) -> Option<&Value> {
        Some(&self.proof)
    }
    fn has_source_duration_limit(&self) -> bool {
        true
    }
    fn duration_frames(&self) -> usize {
        self.duration
    }
    fn position_frames(&self) -> usize {
        self.position
    }
    fn sample_rate(&self) -> u32 {
        self.options.sample_rate
    }
    fn track_count(&self) -> usize {
        1
    }
    fn warnings(&self) -> &[String] {
        &self.warnings
    }
    fn reset(&mut self) -> Result<()> {
        self.position = 0;
        Ok(())
    }
    fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        ensure!(mask <= 1, "hUGE playback exposes one mixed audio track");
        self.mask = mask;
        Ok(())
    }
    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        check_cancel(cancel)?;
        ensure!(
            output.len().is_multiple_of(2),
            "stereo output requires pairs of samples"
        );
        let frames = (output.len() / 2).min(self.duration - self.position);
        for (index, sample) in output[..frames * 2].iter_mut().enumerate() {
            *sample = fade(
                if self.mask == 0 {
                    0
                } else {
                    self.pcm[self.position * 2 + index]
                },
                self.position + index / 2,
                self.duration,
                self.options.sample_rate,
                self.options.fade_seconds,
            );
        }
        self.position += frames;
        Ok(frames * 2)
    }
}

#[cfg(test)]
mod tests;

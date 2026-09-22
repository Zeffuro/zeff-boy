use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::GameBoyAudioTrace;
use zeff_gb_core::hardware::apu::GameBoyTraceReplayer;

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

pub(crate) struct GbTraceSession {
    replay: GameBoyTraceReplayer,
    options: RenderOptions,
    source_frames: u64,
    duration: usize,
    position: usize,
    pending: Vec<f32>,
    pending_cursor: usize,
    floats: Vec<f32>,
    mask: u16,
    interrupted: bool,
    warnings: Vec<String>,
}

impl GbTraceSession {
    pub(crate) fn new(
        trace: GameBoyAudioTrace,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        let replay = GameBoyTraceReplayer::new(trace, options.sample_rate)?;
        check_cancel(cancel)?;
        let source_frames = replay.duration_frames();
        let duration = source_frames
            .min(u64::from(options.max_seconds) * u64::from(options.sample_rate))
            as usize;
        ensure!(
            duration > 0,
            "Game Boy trace contains no emitted audio frames"
        );
        let mut warnings = vec![
            "Replays the captured Game Boy audio interval. This interval does not identify a song or loop.".into(),
            "Playback follows the audio emitted by the source; its duration may be shorter than the capture timeline.".into(),
        ];
        if options.fade_seconds > 0 {
            warnings
                .push("The requested fade is applied at the end of the rendered interval.".into());
        }
        Ok(Self {
            replay,
            options,
            source_frames,
            duration,
            position: 0,
            pending: Vec::new(),
            pending_cursor: 0,
            floats: Vec::with_capacity(2048),
            mask: 1,
            interrupted: false,
            warnings,
        })
    }

    fn generate(&mut self, samples: usize, cancel: &AtomicBool) -> Result<()> {
        self.floats.clear();
        while self.floats.len() < samples {
            check_cancel(cancel)?;
            if self.pending_cursor == self.pending.len() {
                self.pending = self.replay.read_next_drain(cancel)?.ok_or_else(|| {
                    anyhow::anyhow!("Game Boy replay ended before its emitted duration")
                })?;
                self.pending_cursor = 0;
            }
            let count = (samples - self.floats.len()).min(self.pending.len() - self.pending_cursor);
            self.floats
                .extend_from_slice(&self.pending[self.pending_cursor..self.pending_cursor + count]);
            self.pending_cursor += count;
        }
        if (self.position + samples / 2) as u64 == self.source_frames {
            ensure!(
                self.pending_cursor == self.pending.len(),
                "Game Boy replay exceeded its emitted duration"
            );
            while let Some(tail) = self.replay.read_next_drain(cancel)? {
                ensure!(
                    tail.is_empty(),
                    "Game Boy replay exceeded its emitted duration"
                );
            }
        }
        Ok(())
    }
}

impl PcmSession for GbTraceSession {
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
        self.replay.reset()?;
        self.pending.clear();
        self.pending_cursor = 0;
        self.floats.clear();
        self.position = 0;
        self.interrupted = false;
        Ok(())
    }

    fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        ensure!(mask & !1 == 0, "track mask selects an unavailable track");
        self.mask = mask;
        Ok(())
    }

    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "audio buffer must hold complete stereo frames"
        );
        check_cancel(cancel)?;
        ensure!(
            !self.interrupted,
            "reset Game Boy replay after an interrupted read"
        );
        let frames = (output.len() / 2)
            .min(self.duration - self.position)
            .min(1024);
        if frames == 0 {
            return Ok(0);
        }
        self.interrupted = true;
        self.generate(frames * 2, cancel)?;
        for (index, value) in self.floats.iter().enumerate() {
            let sample = if self.mask == 0 {
                0
            } else {
                (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
            };
            output[index] = fade(
                sample,
                self.position + index / 2,
                self.duration,
                self.options.sample_rate,
                self.options.fade_seconds,
            );
        }
        self.position += frames;
        self.interrupted = false;
        Ok(frames * 2)
    }
}

#[cfg(test)]
mod tests;

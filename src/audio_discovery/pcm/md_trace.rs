use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::vgm::capture::validate_md_trace;
use zeff_emu_common::audio_trace::MdAudioTrace;
use zeff_md_core::{MD_AUDIO_REPLAY_MAX_FRAMES, MdAudioReplay};

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

pub(crate) struct MdTraceSession {
    trace: MdAudioTrace,
    replay: MdAudioReplay,
    options: RenderOptions,
    duration: usize,
    source_complete: bool,
    position: usize,
    next_event: usize,
    mask: u16,
    interrupted: bool,
    floats: Vec<f32>,
    warnings: Vec<String>,
}

impl MdTraceSession {
    pub(crate) fn new(
        trace: MdAudioTrace,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        check_cancel(cancel)?;
        validate_options(options)?;
        validate_md_trace(&trace, cancel)?;
        let source_frames: usize = (u128::from(trace.end_cycle) * u128::from(options.sample_rate)
            / u128::from(trace.chip.master_hz))
        .try_into()
        .context("MD source duration overflows")?;
        let max_frames = usize::from(options.max_seconds)
            .checked_mul(options.sample_rate as usize)
            .context("MD render duration overflows")?;
        let duration = source_frames.min(max_frames);
        ensure!(duration > 0, "MD trace is shorter than one output frame");
        let mut warnings = vec![
            "Replays the captured native Mega Drive audio interval using its original bus-service timing. This composite interval may contain effects and silence; it does not identify a song or loop.".into(),
        ];
        if options.fade_seconds > 0 {
            warnings
                .push("The requested fade is applied at the end of the rendered interval.".into());
        }
        Ok(Self {
            replay: MdAudioReplay::new(&trace.chip, options.sample_rate)?,
            trace,
            options,
            duration,
            source_complete: source_frames <= max_frames,
            position: 0,
            next_event: 0,
            mask: 1,
            interrupted: false,
            floats: Vec::with_capacity(MD_AUDIO_REPLAY_MAX_FRAMES * 2),
            warnings,
        })
    }

    fn generate(&mut self, frames: usize, check: &mut impl FnMut() -> Result<()>) -> Result<()> {
        let end_frame = self.position + frames;
        let target = if self.source_complete && end_frame == self.duration {
            self.trace.end_cycle
        } else {
            ((end_frame as u128) * u128::from(self.trace.chip.master_hz))
                .div_ceil(u128::from(self.options.sample_rate))
                .min(u128::from(self.trace.end_cycle)) as u64
        };
        self.floats.clear();
        // Bound sparse intervals by chip time too.
        let max_ticks = (self.trace.chip.master_hz / 1000).max(1);
        loop {
            check()?;
            if let Some(event) = self.trace.events.get(self.next_event)
                && event.cycle <= self.replay.now()
            {
                self.replay
                    .write(event.cycle, event.write, &mut self.floats)?;
                self.next_event += 1;
                continue;
            }
            if self.replay.now() == target {
                break;
            }
            let next_event = self
                .trace
                .events
                .get(self.next_event)
                .map_or(target, |event| event.cycle.min(target));
            let step = (next_event - self.replay.now()).min(max_ticks);
            ensure!(step > 0, "MD trace has a stalled event schedule");
            self.replay
                .advance_to(self.replay.now() + step, &mut self.floats)?;
        }
        ensure!(
            self.floats.len() == frames * 2 && self.floats.iter().all(|value| value.is_finite()),
            "native MD audio returned an invalid stereo interval"
        );
        Ok(())
    }

    fn read_checked(
        &mut self,
        output: &mut [i16],
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<usize> {
        ensure!(
            output.len().is_multiple_of(2),
            "audio buffer must hold complete stereo frames"
        );
        check()?;
        ensure!(
            !self.interrupted,
            "reset MD replay after an interrupted read"
        );
        let frames = (output.len() / 2)
            .min(self.duration - self.position)
            .min(MD_AUDIO_REPLAY_MAX_FRAMES);
        if frames == 0 {
            return Ok(0);
        }
        self.interrupted = true;
        self.generate(frames, &mut check)?;
        check()?;
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

impl PcmSession for MdTraceSession {
    fn has_source_duration_limit(&self) -> bool {
        self.source_complete
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
        self.replay = MdAudioReplay::new(&self.trace.chip, self.options.sample_rate)?;
        self.position = 0;
        self.next_event = 0;
        self.interrupted = false;
        self.floats.clear();
        Ok(())
    }

    fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        ensure!(mask & !1 == 0, "track mask selects an unavailable track");
        self.mask = mask;
        Ok(())
    }

    fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        self.read_checked(output, || check_cancel(cancel))
    }
}

#[cfg(test)]
mod tests;

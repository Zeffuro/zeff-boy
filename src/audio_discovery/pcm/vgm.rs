use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_audio_discovery::vgm::{
    TICKS_PER_SECOND, VgmLog,
    playback::{self, PreparedSnVgm, SnPlayback, SnPsgModel, SnWrite},
};

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

enum Chip {
    Sega(zeff_sega8_core::hardware::psg::Psg),
    Ti(zeff_coleco_core::psg::Psg),
}

impl Chip {
    fn new(config: SnPlayback, rate: u32) -> Self {
        match config.model {
            SnPsgModel::Sega => Self::Sega(
                zeff_sega8_core::hardware::psg::Psg::new_with_sample_rate_and_clock_hz(
                    rate,
                    config.clock_hz,
                ),
            ),
            SnPsgModel::TiSn76489 => {
                Self::Ti(zeff_coleco_core::psg::Psg::new_with_sample_rate(rate))
            }
        }
    }

    fn write(&mut self, write: SnWrite) {
        match (self, write) {
            (Self::Sega(chip), SnWrite::Psg(value)) => chip.write_data(value),
            (Self::Sega(chip), SnWrite::Stereo(value)) => chip.write_stereo_control(value),
            (Self::Ti(chip), SnWrite::Psg(value)) => chip.write(value),
            (Self::Ti(_), SnWrite::Stereo(_)) => unreachable!("validated mono chip"),
        }
    }

    fn advance(&mut self, cycles: u32, output: &mut Vec<f32>) {
        match self {
            Self::Sega(chip) => {
                chip.step_cycles(cycles);
                chip.drain_audio_samples_into(output);
            }
            Self::Ti(chip) => {
                chip.step_cycles(cycles);
                chip.drain_audio_samples_into(output);
            }
        }
    }
}

pub(crate) struct VgmSession {
    prepared: PreparedSnVgm,
    chip: Chip,
    options: RenderOptions,
    duration: usize,
    position: usize,
    cycle: u64,
    next_write: usize,
    mask: u16,
    floats: Vec<f32>,
    warnings: Vec<String>,
}

impl VgmSession {
    pub(crate) fn new(
        bytes: &[u8],
        log: &VgmLog,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        validate_options(options)?;
        let prepared = playback::prepare(bytes, log, cancel)?;
        let source_frames =
            prepared.duration_ticks * u64::from(options.sample_rate) / u64::from(TICKS_PER_SECOND);
        let duration = source_frames
            .min(u64::from(options.max_seconds) * u64::from(options.sample_rate))
            as usize;
        ensure!(
            duration > 0,
            "VGM interval is shorter than one output frame"
        );
        let mut warnings = vec![
            "Plays the recorded register log once, stopping at its end or the duration limit. Loop markers are not repeated; captures may include effects and silence.".into(),
            "Uses Zeff Boy's PSG model. VGM timing is quantized to 44,100 ticks per second and does not preserve original oscillator phase or all chip behavior; source PCM equivalence is not claimed.".into(),
        ];
        if options.fade_seconds > 0 {
            warnings
                .push("The requested fade is applied at the end of the rendered interval.".into());
        }
        Ok(Self {
            chip: Chip::new(prepared.config, options.sample_rate),
            prepared,
            options,
            duration,
            position: 0,
            cycle: 0,
            next_write: 0,
            mask: 1,
            floats: Vec::new(),
            warnings,
        })
    }

    fn generate(&mut self, frames: usize, cancel: &AtomicBool) -> Result<()> {
        let clock = u64::from(self.prepared.config.clock_hz);
        let target =
            ((self.position + frames) as u64 * clock).div_ceil(u64::from(self.options.sample_rate));
        self.floats.clear();
        while self.cycle < target {
            check_cancel(cancel)?;
            if let Some(event) = self.prepared.writes.get(self.next_write) {
                let event_cycle = (event.tick * clock).div_ceil(u64::from(TICKS_PER_SECOND));
                if event_cycle <= self.cycle {
                    self.chip.write(event.write);
                    self.next_write += 1;
                    continue;
                }
                let end = target.min(event_cycle);
                self.chip
                    .advance((end - self.cycle) as u32, &mut self.floats);
                self.cycle = end;
            } else {
                self.chip
                    .advance((target - self.cycle) as u32, &mut self.floats);
                self.cycle = target;
            }
        }
        ensure!(
            self.floats.len() == frames * 2 && self.floats.iter().all(|value| value.is_finite()),
            "VGM chip returned an invalid stereo interval"
        );
        Ok(())
    }
}

impl PcmSession for VgmSession {
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
        self.chip = Chip::new(self.prepared.config, self.options.sample_rate);
        self.position = 0;
        self.cycle = 0;
        self.next_write = 0;
        self.floats.clear();
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
        let frames = (output.len() / 2)
            .min(self.duration - self.position)
            .min(1024);
        if frames == 0 {
            return Ok(0);
        }
        self.generate(frames, cancel)?;
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
        Ok(frames * 2)
    }
}

#[cfg(test)]
mod tests;

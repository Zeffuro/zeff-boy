use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{
    AudioTrace, AudioTraceStart, AudioTraceTiming, AudioTraceWrite, Sn76489ResetState,
    Sn76489Tone2NoiseClock, Sn76489TraceChip, Sn76489ZeroPeriod,
};
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::{PcmSession, check_cancel, fade, validate_options};
use crate::audio_discovery::render::RenderOptions;

#[derive(Clone, Copy)]
enum Model {
    Sega,
    Ti,
}

impl Model {
    fn contract(self, clock_hz: u32, stereo: bool) -> Sn76489TraceChip {
        let sega = matches!(self, Self::Sega);
        Sn76489TraceChip {
            clock_hz,
            feedback_mask: if sega { 9 } else { 3 },
            shift_register_width: if sega { 16 } else { 15 },
            zero_period: if sega {
                Sn76489ZeroPeriod::ConstantHigh
            } else {
                Sn76489ZeroPeriod::Period1024
            },
            period_one_constant_high: sega,
            tone_counter_clock_divider: 16,
            noise_tone2_clock: if sega {
                Sn76489Tone2NoiseClock::HalfPeriod
            } else {
                Sn76489Tone2NoiseClock::RisingEdge
            },
            noise_output_high_when_lfsr_bit_zero: sega,
            stereo,
            reset: Sn76489ResetState {
                tone_periods: [0; 3],
                volumes: [15; 4],
                noise_control: 0,
                stereo_control: 255,
                latched_register: 0,
                noise_lfsr: if sega { 0x8000 } else { 0x4000 },
                tone_output_high: [sega; 3],
                tone_clocks_remaining: [16; 3],
                noise_clocks_remaining: if sega { 512 } else { 16 },
            },
        }
    }
}

fn validate(trace: &AudioTrace, cancel: &AtomicBool) -> Result<Model> {
    check_cancel(cancel)?;
    trace.validate_complete()?;
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.cycle_hz_denominator == 1
            && trace.cycle_hz == trace.chip.clock_hz,
        "SN76489 replay requires reset capture with matching native event and chip clocks"
    );
    let model = match trace.timing {
        AudioTraceTiming::InstructionBoundary => {
            ensure!(
                [Sega8VideoStandard::Ntsc, Sega8VideoStandard::Pal]
                    .map(Sega8VideoStandard::clock_hz_approx)
                    .contains(&trace.cycle_hz),
                "unsupported native Sega PSG clock"
            );
            Model::Sega
        }
        AudioTraceTiming::IoWriteCompletion => {
            ensure!(
                trace.cycle_hz == zeff_coleco_core::psg::COLECO_PSG_INPUT_CLOCK_HZ
                    && !trace.chip.stereo,
                "unsupported native TI PSG clock or stereo contract"
            );
            Model::Ti
        }
        _ => anyhow::bail!("unsupported native SN76489 write timing"),
    };
    ensure!(
        trace.chip == model.contract(trace.cycle_hz, trace.chip.stereo),
        "unsupported SN76489 model or reset state"
    );
    for event in &trace.events {
        check_cancel(cancel)?;
        let supported = match (model, event.write) {
            (Model::Sega, AudioTraceWrite::Sn76489 { port, .. }) => (0x40..=0x7f).contains(&port),
            (Model::Ti, AudioTraceWrite::Sn76489 { port, .. }) => port >= 0xe0,
            (Model::Sega, AudioTraceWrite::GameGearStereo { port: 6, .. }) => trace.chip.stereo,
            _ => false,
        };
        ensure!(supported, "unsupported SN76489 native write port or device");
    }
    Ok(model)
}

enum Chip {
    Sega(zeff_sega8_core::hardware::psg::Psg),
    Ti(zeff_coleco_core::psg::Psg),
}

impl Chip {
    fn new(model: Model, clock: u32, rate: u32) -> Self {
        match model {
            Model::Sega => Self::Sega(
                zeff_sega8_core::hardware::psg::Psg::new_with_sample_rate_and_clock_hz(rate, clock),
            ),
            Model::Ti => Self::Ti(zeff_coleco_core::psg::Psg::new_with_sample_rate(rate)),
        }
    }

    fn write(&mut self, write: AudioTraceWrite) {
        match (self, write) {
            (Self::Sega(chip), AudioTraceWrite::Sn76489 { value, .. }) => chip.write_data(value),
            (Self::Sega(chip), AudioTraceWrite::GameGearStereo { value, .. }) => {
                chip.write_stereo_control(value);
            }
            (Self::Ti(chip), AudioTraceWrite::Sn76489 { value, .. }) => chip.write(value),
            (Self::Ti(_), AudioTraceWrite::GameGearStereo { .. }) => {
                unreachable!("validated mono chip")
            }
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

pub(crate) struct SnTraceSession {
    trace: AudioTrace,
    model: Model,
    chip: Chip,
    options: RenderOptions,
    duration: usize,
    position: usize,
    cycle: u64,
    next_write: usize,
    mask: u16,
    interrupted: bool,
    floats: Vec<f32>,
    warnings: Vec<String>,
}

impl SnTraceSession {
    pub(crate) fn new(
        trace: AudioTrace,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        validate_options(options)?;
        let model = validate(&trace, cancel)?;
        let source_frames = u128::from(trace.end_cycle) * u128::from(options.sample_rate)
            / u128::from(trace.cycle_hz);
        let duration = source_frames
            .min(u128::from(options.max_seconds) * u128::from(options.sample_rate))
            as usize;
        ensure!(
            duration > 0,
            "SN76489 trace is shorter than one output frame"
        );
        let mut warnings = vec![
            "Replays the native reset-to-end register interval using its original cycle timing. This interval may contain effects and silence; it does not identify a song or loop.".into(),
        ];
        if options.fade_seconds > 0 {
            warnings
                .push("The requested fade is applied at the end of the rendered interval.".into());
        }
        Ok(Self {
            chip: Chip::new(model, trace.cycle_hz, options.sample_rate),
            trace,
            model,
            options,
            duration,
            position: 0,
            cycle: 0,
            next_write: 0,
            mask: 1,
            interrupted: false,
            floats: Vec::with_capacity(2048),
            warnings,
        })
    }

    fn generate(&mut self, frames: usize, cancel: &AtomicBool) -> Result<()> {
        let target = ((self.position + frames) as u64 * u64::from(self.trace.cycle_hz))
            .div_ceil(u64::from(self.options.sample_rate));
        self.floats.clear();
        while self.cycle < target {
            check_cancel(cancel)?;
            let end = if let Some(event) = self.trace.events.get(self.next_write) {
                if event.cycle <= self.cycle {
                    self.chip.write(event.write);
                    self.next_write += 1;
                    continue;
                }
                target.min(event.cycle)
            } else {
                target
            };
            self.chip
                .advance((end - self.cycle) as u32, &mut self.floats);
            self.cycle = end;
        }
        ensure!(
            self.floats.len() == frames * 2 && self.floats.iter().all(|value| value.is_finite()),
            "native SN76489 chip returned an invalid stereo interval"
        );
        Ok(())
    }
}

impl PcmSession for SnTraceSession {
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
        self.chip = Chip::new(self.model, self.trace.cycle_hz, self.options.sample_rate);
        self.position = 0;
        self.cycle = 0;
        self.next_write = 0;
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
        ensure!(
            output.len().is_multiple_of(2),
            "audio buffer must hold complete stereo frames"
        );
        check_cancel(cancel)?;
        ensure!(
            !self.interrupted,
            "reset native SN76489 replay after an interrupted read"
        );
        let frames = (output.len() / 2)
            .min(self.duration - self.position)
            .min(1024);
        if frames == 0 {
            return Ok(0);
        }
        self.interrupted = true;
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
        self.interrupted = false;
        Ok(frames * 2)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod timing_tests;

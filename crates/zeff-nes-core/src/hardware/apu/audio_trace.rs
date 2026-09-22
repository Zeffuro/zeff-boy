use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{NesTraceChip, NesTraceRegion, NesTraceReset};
use zeff_emu_common::time::ClockRate;

use super::Apu;
use crate::hardware::timing::NesTiming;

pub(crate) const AUDIO_TRACE_CPU_ORIGIN: u64 = 7;

pub(crate) fn native_trace_chip(timing: NesTiming) -> NesTraceChip {
    let (numerator, denominator) = timing.cpu_clock_hz_ratio();
    let clock = ClockRate::from_ratio(numerator, denominator);
    NesTraceChip {
        clock_hz_numerator: clock.numerator_hz(),
        clock_hz_denominator: clock.denominator() as u32,
        region: match timing {
            NesTiming::Ntsc => NesTraceRegion::Ntsc,
            NesTiming::Pal => NesTraceRegion::Pal,
            NesTiming::Dendy => NesTraceRegion::Dendy,
        },
        reset: NesTraceReset::ZeffPowerOnV1,
        initial_cpu_cycle: AUDIO_TRACE_CPU_ORIGIN,
        initial_cpu_cycle_odd: true,
        initial_apu_frame_cycle: 9,
        initial_half_rate_timer_clock: false,
    }
}

impl Apu {
    pub fn audio_trace_frame_count(
        chip: &NesTraceChip,
        sample_rate: u32,
        cycles: u64,
    ) -> Result<u64> {
        let apu = Self::new_for_audio_trace(chip, f64::from(sample_rate))?;
        let clock = apu.cpu_clock_hz;
        ensure!(
            clock >= 1_048_576.0 && clock + f64::from(sample_rate) < 2_097_152.0,
            "unsupported NES native sample-clock range"
        );
        let numerator = (clock.to_bits() & ((1 << 52) - 1)) | (1 << 52);
        // Every accumulator operation is exact on the clock's 2^-32 grid below 2^21.
        let frames =
            u128::from(cycles) * u128::from(sample_rate) * (1 << 32) / u128::from(numerator);
        Ok(u64::try_from(frames)?)
    }

    pub fn new_for_audio_trace(chip: &NesTraceChip, sample_rate: f64) -> Result<Self> {
        ensure!(
            (1.0..=192_000.0).contains(&sample_rate),
            "invalid NES replay sample rate"
        );
        let timing = match chip.region {
            NesTraceRegion::Ntsc => NesTiming::Ntsc,
            NesTraceRegion::Pal => NesTiming::Pal,
            NesTraceRegion::Dendy => NesTiming::Dendy,
        };
        ensure!(
            *chip == native_trace_chip(timing),
            "unsupported NES native audio reset or clock contract"
        );
        Ok(Self::new_with_timing(sample_rate, timing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_frame_count_matches_native_sampling_at_rational_boundaries() {
        for timing in [NesTiming::Ntsc, NesTiming::Pal, NesTiming::Dendy] {
            let chip = native_trace_chip(timing);
            for rate in [44_100, 48_000, 63_072, 96_000] {
                let mut apu = Apu::new_for_audio_trace(&chip, f64::from(rate)).unwrap();
                apu.set_debug_collection_enabled(false);
                let mut cycle = 0;
                let mut frames = 0;
                for end in [1, 13_125, 131_250, 1_312_500] {
                    while cycle < end {
                        apu.tick();
                        cycle += 1;
                    }
                    frames += apu.sample_buffer.len() as u64;
                    apu.sample_buffer.clear();
                    assert_eq!(
                        Apu::audio_trace_frame_count(&chip, rate, end).unwrap(),
                        frames
                    );
                }
            }
        }
        let chip = native_trace_chip(NesTiming::Ntsc);
        let physical =
            13_125u64 * 48_000 * u64::from(chip.clock_hz_denominator) / chip.clock_hz_numerator;
        assert_ne!(
            Apu::audio_trace_frame_count(&chip, 48_000, 13_125).unwrap(),
            physical
        );
    }
}

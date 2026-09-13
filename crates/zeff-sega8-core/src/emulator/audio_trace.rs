use super::Emulator;
use crate::hardware::cartridge::Sega8System;
use zeff_emu_common::audio_trace::{
    AudioTrace, AudioTraceTiming, Sn76489ResetState, Sn76489Tone2NoiseClock, Sn76489TraceChip,
    Sn76489ZeroPeriod,
};

impl Emulator {
    pub fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        let clock_hz = self.video_standard.clock_hz_approx();
        let chip = Sn76489TraceChip {
            clock_hz: self.bus.apu().clock_hz(),
            feedback_mask: 0x0009,
            shift_register_width: 16,
            zero_period: Sn76489ZeroPeriod::ConstantHigh,
            period_one_constant_high: true,
            tone_counter_clock_divider: 16,
            noise_tone2_clock: Sn76489Tone2NoiseClock::HalfPeriod,
            noise_output_high_when_lfsr_bit_zero: true,
            stereo: self.system() == Sega8System::GameGear,
            reset: Sn76489ResetState {
                tone_periods: [0; 3],
                volumes: [15; 4],
                noise_control: 0,
                stereo_control: 0xFF,
                latched_register: 0,
                noise_lfsr: 0x8000,
                tone_output_high: [true; 3],
                tone_clocks_remaining: [16; 3],
                noise_clocks_remaining: 512,
            },
        };
        let recorder = self.bus.audio_trace.prepare(
            max_events,
            clock_hz,
            chip,
            AudioTraceTiming::InstructionBoundary,
        )?;
        self.reset();
        self.bus.audio_trace = recorder;
        Ok(())
    }

    pub fn finish_audio_trace(&mut self) -> Option<AudioTrace> {
        self.bus.audio_trace.finish(self.cpu.cycles())
    }
}

#[cfg(test)]
mod tests;

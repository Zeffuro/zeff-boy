use super::Emulator;
use crate::constants::{
    BIOS_END, CARTRIDGE_START, CPU_CLOCK_HZ, WORK_RAM_END, WORK_RAM_SIZE, WORK_RAM_START,
};
use zeff_emu_common::audio_trace::{
    AudioTrace, AudioTraceEvent, AudioTraceSource, AudioTraceTiming, AudioTraceWrite,
    Sn76489ResetState, Sn76489Tone2NoiseClock, Sn76489TraceChip, Sn76489ZeroPeriod,
};

impl Emulator {
    pub fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        let clock_hz = CPU_CLOCK_HZ as u32;
        let chip = Sn76489TraceChip {
            clock_hz,
            feedback_mask: 0x0003,
            shift_register_width: 15,
            zero_period: Sn76489ZeroPeriod::Period1024,
            period_one_constant_high: false,
            tone_counter_clock_divider: 16,
            noise_tone2_clock: Sn76489Tone2NoiseClock::RisingEdge,
            noise_output_high_when_lfsr_bit_zero: false,
            stereo: false,
            reset: Sn76489ResetState {
                tone_periods: [0; 3],
                volumes: [15; 4],
                noise_control: 0,
                stereo_control: 0xFF,
                latched_register: 0,
                noise_lfsr: 0x4000,
                tone_output_high: [false; 3],
                tone_clocks_remaining: [16; 3],
                noise_clocks_remaining: 16,
            },
        };
        let recorder = self.bus.audio_trace.prepare(
            max_events,
            clock_hz,
            chip,
            AudioTraceTiming::IoWriteCompletion,
        )?;
        self.reset();
        self.bus.audio_trace = recorder;
        Ok(())
    }

    pub fn finish_audio_trace(&mut self) -> Option<AudioTrace> {
        self.bus.audio_trace.finish(self.effective_cycles)
    }

    pub(super) fn record_audio_write(&mut self, pc: u16, port: u8, value: u8) {
        if !self.bus.audio_trace.is_enabled() {
            return;
        }
        let instruction_source = match pc {
            0..=BIOS_END => AudioTraceSource::BootRom {
                offset: u64::from(pc),
            },
            WORK_RAM_START..=WORK_RAM_END => AudioTraceSource::WorkRam {
                offset: (usize::from(pc) & (WORK_RAM_SIZE - 1)) as u32,
            },
            CARTRIDGE_START..=u16::MAX => self.bus.rom_offset_for_cpu_address(pc).map_or(
                AudioTraceSource::Unmapped,
                |offset| AudioTraceSource::CartridgeRom {
                    offset: offset as u64,
                    bit_reversed: false,
                },
            ),
            _ => AudioTraceSource::Unmapped,
        };
        self.bus.audio_trace.record(AudioTraceEvent {
            cycle: self.effective_cycles,
            pc: u32::from(pc),
            instruction_source,
            write: AudioTraceWrite::Sn76489 { port, value },
        });
    }
}

#[cfg(test)]
mod tests;

use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{AudioTraceInvalidation, AudioTraceTiming, NesAudioTrace};
use zeff_emu_common::time::ClockRate;

use super::Emulator;
use crate::hardware::apu::{AUDIO_TRACE_CPU_ORIGIN, native_trace_chip};

impl Emulator {
    pub fn new_with_audio_trace(
        rom_data: &[u8],
        sample_rate: f64,
        max_events: usize,
    ) -> Result<Self> {
        ensure!(
            (1.0..=192_000.0).contains(&sample_rate),
            "invalid NES capture sample rate"
        );
        let mut emulator = Self::new(rom_data, sample_rate)?;
        ensure!(
            emulator.has_standard_console_hardware()
                && emulator.bus.cartridge.supports_base_audio_trace(),
            "NES audio capture requires a standard NROM, MMC1, UxROM, CNROM, MMC3 or AxROM cartridge; expansion audio and other boards are unsupported"
        );
        let chip = native_trace_chip(emulator.bus.timing);
        let clock = ClockRate::from_ratio(
            chip.clock_hz_numerator,
            u64::from(chip.clock_hz_denominator),
        );
        emulator.bus.audio_trace = emulator.bus.audio_trace.prepare_with_clock(
            max_events,
            clock,
            chip,
            AudioTraceTiming::CpuBusCycleBoundary,
        )?;
        Ok(emulator)
    }

    pub fn finish_audio_trace(&mut self) -> Option<NesAudioTrace> {
        let end = self.cpu.cycles.checked_sub(AUDIO_TRACE_CPU_ORIGIN);
        if end.is_none() {
            self.bus
                .audio_trace
                .invalidate(AudioTraceInvalidation::ClockOverflow);
        }
        self.bus.audio_trace.finish(end.unwrap_or(0))
    }

    pub(crate) fn invalidate_audio_trace(&mut self, reason: AudioTraceInvalidation) {
        self.bus.audio_trace.invalidate(reason);
    }
}

#[cfg(test)]
mod tests;

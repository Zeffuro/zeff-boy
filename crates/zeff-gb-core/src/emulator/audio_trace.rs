use super::Emulator;
use crate::hardware::types::constants::GB_T_CYCLES_PER_SECOND;
use crate::hardware::types::hardware_mode::HardwareMode;
use zeff_emu_common::audio_trace::{AudioTraceInvalidation, AudioTraceTiming, GameBoyAudioTrace};

impl Emulator {
    pub fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        anyhow::ensure!(
            !matches!(self.hardware_mode, HardwareMode::SGB1 | HardwareMode::SGB2),
            "Game Boy audio capture supports DMG and CGB hardware"
        );
        anyhow::ensure!(
            self.bus.audio_trace_apu_enabled(),
            "Game Boy audio capture requires the APU enabled"
        );
        let mut reset = self.make_reset_emulator()?;
        let recorder = self.bus.audio_trace.prepare(
            max_events,
            GB_T_CYCLES_PER_SECOND as u32,
            reset.bus.audio_trace_chip(),
            AudioTraceTiming::CpuBusCycleBoundary,
        )?;
        reset.bus.audio_trace = recorder;
        *self = reset;
        Ok(())
    }

    pub fn finish_audio_trace(&mut self) -> Option<GameBoyAudioTrace> {
        self.bus.audio_trace.finish(self.bus.audio_trace_cycle)
    }

    pub(crate) fn invalidate_audio_trace(&mut self) {
        self.bus
            .audio_trace
            .invalidate(AudioTraceInvalidation::ExternalMutation);
    }
}

#[cfg(test)]
mod tests;

use super::Emulator;
use crate::hardware::constants::CPU_CLOCK_HZ;
use zeff_emu_common::audio_trace::{
    AudioTraceInvalidation, AudioTraceTiming, WonderSwanAudioTrace, WonderSwanResetState,
    WonderSwanTraceChip,
};

impl Emulator {
    pub fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        let recorder = self.bus.audio_trace.prepare(
            max_events,
            CPU_CLOCK_HZ,
            WonderSwanTraceChip {
                clock_hz: CPU_CLOCK_HZ,
                color: self.bus.is_color_model(),
                reset: WonderSwanResetState::default(),
            },
            AudioTraceTiming::BusServiceBoundary,
        )?;
        self.reset();
        self.bus.audio_trace = recorder;
        Ok(())
    }

    pub fn finish_audio_trace(&mut self) -> Option<WonderSwanAudioTrace> {
        self.bus.audio_trace.finish(self.bus.cycles)
    }

    pub(crate) fn invalidate_audio_trace(&mut self) {
        self.bus
            .audio_trace
            .invalidate(AudioTraceInvalidation::ExternalMutation);
    }
}

#[cfg(test)]
mod tests;

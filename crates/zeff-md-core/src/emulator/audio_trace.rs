use super::*;
use crate::hardware::audio::trace::{main_origin, trace_chip};
use sha2::{Digest, Sha256};
use zeff_emu_common::audio_trace::{AudioTraceInvalidation, AudioTraceTiming, MdAudioTraceCapture};

impl Emulator {
    pub fn reset_and_begin_audio_trace(&mut self, max_events: usize) -> anyhow::Result<()> {
        self.reset_and_begin_audio_trace_captured(max_events, &mut NoSoundCapture)
    }

    pub fn reset_and_begin_audio_trace_sound_traced(
        &mut self,
        max_events: usize,
        sound: &mut crate::sound_trace::SoundTraceAttachment<'_>,
    ) -> anyhow::Result<()> {
        self.reset_and_begin_audio_trace_captured(max_events, sound)
    }

    fn reset_and_begin_audio_trace_captured<C: SoundCapture>(
        &mut self,
        max_events: usize,
        sound: &mut C,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.bus.cartridge.rom_overlay.is_empty(),
            "audio tracing is unavailable with active ROM cheats"
        );
        anyhow::ensure!(
            self.bus.owned.is_none(),
            "audio trace ownership is unavailable for precise timing"
        );
        let chip = trace_chip(self.region());
        let recorder = self.bus.audio.trace.prepare_with_clock(
            max_events,
            self.bus.rate(),
            chip,
            AudioTraceTiming::BusServiceBoundary,
        )?;
        sound.reset();
        self.reset();
        self.bus.audio.trace = recorder;
        Ok(())
    }

    pub fn finish_audio_trace(&mut self) -> Option<MdAudioTraceCapture> {
        self.bus.audio.trace_origin = None;
        let trace = self.bus.audio.trace.finish(self.bus.now.get())?;
        Some(MdAudioTraceCapture {
            effective_rom_sha256: Sha256::digest(self.bus.cartridge.bytes.as_ref()).into(),
            effective_rom_len: self.bus.cartridge.bytes.len() as u32,
            trace,
        })
    }

    pub(super) fn begin_main_trace(&mut self) {
        if self.bus.audio.trace.is_enabled() {
            self.bus.audio.trace_origin =
                Some(main_origin(self.cpu.registers.pc, &self.bus.cartridge));
        }
    }

    pub(crate) fn invalidate_audio_trace(&mut self, reason: AudioTraceInvalidation) {
        self.bus.audio.invalidate_trace(reason);
    }

    pub(super) fn invalidate_trace_fault(&mut self, fault: &MachineFault) {
        let reason = if matches!(
            fault,
            MachineFault::Main(
                TimingFault::ClockOverflow
                    | TimingFault::Bus {
                        source: BusError::ClockOverflow,
                        ..
                    }
            ) | MachineFault::Sound(SoundFault::ClockOverflow)
        ) {
            AudioTraceInvalidation::ClockOverflow
        } else {
            AudioTraceInvalidation::ExecutionFault
        };
        self.invalidate_audio_trace(reason);
    }
}

#[cfg(test)]
mod tests;

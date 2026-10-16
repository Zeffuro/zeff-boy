use super::*;
use crate::constants::{FM_CHANNEL_COUNT, PSG_CHANNEL_COUNT};
use crate::hardware::audio::{Opn2DebugSnapshot, PsgDebugSnapshot};

impl Emulator {
    pub fn psg_snapshot(&self) -> PsgDebugSnapshot {
        self.bus.audio.psg.debug_snapshot()
    }

    pub fn opn2_snapshot(&self) -> Opn2DebugSnapshot {
        self.bus.audio.opn2.debug_snapshot()
    }

    pub fn set_fm_channel_mutes(&mut self, mutes: [bool; FM_CHANNEL_COUNT]) {
        self.bus.audio.set_fm_channel_mutes(mutes);
    }

    pub fn sample_rate(&self) -> u32 {
        self.bus.audio.sample_rate()
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.audio.set_sample_rate(rate);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.audio.set_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.bus.audio.set_channel_mutes(mutes);
    }

    /// Appends stereo frames; an undrained queue retains the latest 4096 frames.
    pub fn drain_audio_samples_into(&mut self, samples: &mut Vec<f32>) {
        self.bus.audio.drain_into(samples);
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut samples = Vec::new();
        self.drain_audio_samples_into(&mut samples);
        samples
    }
}

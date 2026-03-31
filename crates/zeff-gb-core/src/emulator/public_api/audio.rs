use super::super::Emulator;

impl Emulator {
    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu_drain_samples()
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu_drain_samples_into(buf);
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.set_apu_sample_rate(rate);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_sample_generation_enabled(enabled);
    }

    pub fn set_apu_debug_capture_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_debug_capture_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 4]) {
        self.bus.set_apu_channel_mutes(mutes);
    }

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.bus.apu_channel_snapshot()
    }

    pub fn apu_regs_snapshot(&self) -> [u8; 0x17] {
        self.bus.apu_regs_snapshot()
    }

    pub fn apu_wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.bus.apu_wave_ram_snapshot()
    }

    pub fn apu_nr52_raw(&self) -> u8 {
        self.bus.apu_nr52_raw()
    }

    pub fn apu_channel_debug_samples_ordered(&self, ch: usize) -> [f32; 512] {
        self.bus.apu_channel_debug_samples_ordered(ch)
    }

    pub fn apu_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.bus.apu_master_debug_samples_ordered()
    }

    pub fn apu_channel_mutes(&self) -> [bool; 4] {
        self.bus.apu_channel_mutes()
    }

    pub fn set_apu_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_enabled(enabled);
    }
}

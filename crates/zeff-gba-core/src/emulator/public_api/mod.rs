use crate::emulator::Emulator;

mod debug;
mod queries;

impl Emulator {
    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu.drain_samples_into(buf);
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.apu.set_sample_rate(rate);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_sample_generation_enabled(enabled);
    }

    pub fn set_apu_debug_capture_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_debug_capture_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 6]) {
        self.bus.apu.set_channel_mutes(mutes);
    }

    pub fn apu_channel_mutes(&self) -> [bool; 6] {
        self.bus.apu.channel_mutes()
    }

    pub fn apu_psg_regs_snapshot(&self) -> [u8; 0x17] {
        self.bus.apu.psg_regs_snapshot()
    }

    pub fn apu_psg_wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.bus.apu.psg_wave_ram_snapshot()
    }

    pub fn apu_psg_nr52_raw(&self) -> u8 {
        self.bus.apu.psg_nr52_raw()
    }

    pub fn apu_psg_channel_debug_samples_ordered(&self, channel: usize) -> [f32; 512] {
        self.bus.apu.psg_channel_debug_samples_ordered(channel)
    }

    pub fn apu_psg_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.bus.apu.psg_master_debug_samples_ordered()
    }

    pub fn apu_direct_debug_samples_ordered(&self, fifo: usize) -> [f32; 512] {
        self.bus.apu.direct_debug_samples_ordered(fifo)
    }

    pub fn apu_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.bus.apu.master_debug_samples_ordered()
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.bus.set_ppu_debug_flags(bg, window, sprites);
    }

    pub fn set_ppu_debug_bg_layers(&mut self, layers: [bool; 4]) {
        self.bus.set_ppu_debug_bg_layers(layers);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.bus
            .keypad
            .set_host_input(buttons_pressed, dpad_pressed);
    }

    pub fn set_tilt_input(&mut self, x: f32, y: f32) -> bool {
        self.bus.cartridge.set_tilt_input(x, y)
    }
}

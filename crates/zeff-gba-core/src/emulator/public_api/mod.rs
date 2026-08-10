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

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 6]) {
        self.bus.apu.set_channel_mutes(mutes);
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
}

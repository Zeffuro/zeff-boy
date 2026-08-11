use crate::emulator::Emulator;
use crate::hardware::apu::PSG_CHANNEL_COUNT;
use crate::hardware::input::ControllerPort;
use crate::hardware::region::Sega8Region;
use crate::hardware::timing::Sega8VideoStandard;

mod debug;
mod queries;

impl Emulator {
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = if sample_rate == 0 {
            super::DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        self.bus.set_apu_sample_rate(self.sample_rate);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn video_standard(&self) -> Sega8VideoStandard {
        self.video_standard
    }

    pub fn set_video_standard(&mut self, video_standard: Sega8VideoStandard) {
        self.video_standard = video_standard;
        self.bus.set_video_standard(video_standard);
    }

    pub fn console_region(&self) -> Sega8Region {
        self.console_region
    }

    pub fn set_console_region(&mut self, console_region: Sega8Region) {
        self.console_region = console_region;
        self.bus.set_console_region(console_region);
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.bus.drain_audio_samples_into(buf);
    }

    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        let mut buf = Vec::new();
        self.drain_audio_samples_into(&mut buf);
        buf
    }

    pub fn clear_rom_patches(&mut self) {
        self.bus.clear_rom_patches();
    }

    pub fn add_rom_patch(&mut self, patch: zeff_emu_common::cheats::CheatPatch) {
        self.bus.add_rom_patch(patch);
    }

    pub fn rom_patches(&self) -> &[zeff_emu_common::cheats::CheatPatch] {
        self.bus.rom_patches()
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.set_apu_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.bus.set_apu_channel_mutes(mutes);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        let raw = host_input_to_sms_raw(buttons_pressed, dpad_pressed);
        let game_gear_start_pressed = buttons_pressed & super::HOST_BUTTON_START != 0;
        self.bus
            .input_mut()
            .set_controller_raw(ControllerPort::One, raw);
        self.bus
            .input_mut()
            .set_game_gear_start_pressed(game_gear_start_pressed);
    }

    pub fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        let raw = host_input_to_sms_raw(buttons_pressed, dpad_pressed);
        self.bus
            .input_mut()
            .set_controller_raw(ControllerPort::Two, raw);
    }

    pub fn sync_game_gear_link_peer(&mut self, peer: &mut Self) {
        self.bus.sync_game_gear_link_peer(&mut peer.bus);
    }
}

fn host_input_to_sms_raw(buttons_pressed: u8, dpad_pressed: u8) -> u8 {
    let mut raw = 0xFF;
    if dpad_pressed & super::HOST_DPAD_UP != 0 {
        raw &= !super::SMS_PAD_UP;
    }
    if dpad_pressed & super::HOST_DPAD_DOWN != 0 {
        raw &= !super::SMS_PAD_DOWN;
    }
    if dpad_pressed & super::HOST_DPAD_LEFT != 0 {
        raw &= !super::SMS_PAD_LEFT;
    }
    if dpad_pressed & super::HOST_DPAD_RIGHT != 0 {
        raw &= !super::SMS_PAD_RIGHT;
    }
    if buttons_pressed & super::HOST_BUTTON_1 != 0 {
        raw &= !super::SMS_PAD_BUTTON_1;
    }
    if buttons_pressed & super::HOST_BUTTON_2 != 0 {
        raw &= !super::SMS_PAD_BUTTON_2;
    }
    raw
}

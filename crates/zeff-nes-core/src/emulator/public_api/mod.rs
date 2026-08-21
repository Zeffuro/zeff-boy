use crate::emulator::Emulator;

mod debug;
mod queries;

impl Emulator {
    pub fn drain_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu.drain_samples()
    }

    pub fn drain_audio_samples_into(&mut self, buf: &mut Vec<f32>) {
        self.drain_audio_into_stereo(buf);
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.bus.apu.set_output_sample_rate(rate as f64);
    }

    pub fn drain_audio_into_stereo(&mut self, buf: &mut Vec<f32>) {
        self.bus.apu.drain_samples_into_stereo(buf);
    }

    pub fn set_apu_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_sample_generation_enabled(enabled);
    }

    pub fn set_apu_channel_mutes(&mut self, mutes: [bool; 5]) {
        self.bus.apu.set_channel_mutes(mutes);
    }

    pub fn set_apu_debug_collection_enabled(&mut self, enabled: bool) {
        self.bus.apu.set_debug_collection_enabled(enabled);
    }

    pub fn set_palette_mode(&mut self, mode: crate::hardware::ppu::NesPaletteMode) {
        self.bus.set_palette_mode(mode);
    }

    pub fn set_custom_palette(&mut self, palette: Option<crate::hardware::ppu::NesPalette>) {
        self.bus.set_custom_palette(palette);
    }

    pub fn palette_mode(&self) -> crate::hardware::ppu::NesPaletteMode {
        self.bus.palette_mode()
    }

    pub fn palette_color_rgba(&self, index: u8) -> [u8; 4] {
        self.bus.palette_color_rgba(index)
    }

    pub fn palette_lut(&self) -> [[u8; 4]; 64] {
        self.bus.palette_lut()
    }

    pub fn apu_channel_snapshot(&self) -> crate::hardware::apu::ApuChannelSnapshot {
        self.bus.apu.channel_snapshot()
    }

    pub fn set_input_p1_raw(&mut self, buttons: u8) {
        self.bus.set_vs_system_credit_input(buttons & 0x04 != 0);
        self.bus.controller1.set_buttons(buttons);
    }

    pub fn set_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_input_p1_raw(map_host_to_nes_byte(buttons_pressed, dpad_pressed));
    }

    pub fn set_input_p2(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.set_input_p2_raw(map_host_to_nes_byte(buttons_pressed, dpad_pressed));
    }

    pub fn set_input_p2_raw(&mut self, buttons: u8) {
        self.bus.controller2.set_buttons(buttons);
    }

    pub fn set_zapper_state(
        &mut self,
        enabled: bool,
        trigger: bool,
        hit: bool,
        screen_pos: Option<(u16, u16)>,
    ) {
        use crate::hardware::cartridge::NesMapper;
        use crate::hardware::controller::ControllerType;
        self.bus.set_zapper_light_sensor(screen_pos, hit);
        if !enabled {
            self.bus.controller1.set_type(ControllerType::Standard);
            self.bus.controller2.set_type(ControllerType::Standard);
            return;
        }

        match self.bus.cartridge.header().mapper_kind() {
            NesMapper::VsSystem | NesMapper::Vrc1VsSystem | NesMapper::LegacyVsVrc1 => {
                self.bus
                    .controller1
                    .set_type(ControllerType::VsZapper { trigger, hit });
                self.bus.controller2.set_type(ControllerType::Standard);
            }
            _ => {
                self.bus
                    .controller2
                    .set_type(ControllerType::Zapper { trigger, hit });
            }
        }
    }

    pub fn clear_game_genie(&mut self) {
        self.bus.game_genie.clear();
    }

    pub fn add_game_genie_patch(&mut self, patch: crate::cheats::NesGameGeniePatch) {
        self.bus.game_genie.patches.push(patch);
    }

    pub fn set_fds_disk_side(&mut self, side: u8) -> anyhow::Result<()> {
        self.bus.cartridge.set_fds_disk_side(side)
    }

    pub fn fds_disk_side(&self) -> Option<u8> {
        self.bus.cartridge.fds_disk_side()
    }

    pub fn media_slot_snapshot(&self) -> Option<zeff_emu_common::media::MediaSlotSnapshot> {
        self.bus.cartridge.media_slot_snapshot()
    }

    pub fn apply_media_event(
        &mut self,
        event: &zeff_emu_common::media::MediaEvent,
    ) -> anyhow::Result<()> {
        self.bus.cartridge.apply_media_event(event)
    }
}

fn map_host_to_nes_byte(buttons_pressed: u8, dpad_pressed: u8) -> u8 {
    (buttons_pressed & 0x0F)
        | ((dpad_pressed & 0x04) << 2)
        | ((dpad_pressed & 0x08) << 2)
        | ((dpad_pressed & 0x02) << 5)
        | ((dpad_pressed & 0x01) << 7)
}

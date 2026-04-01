mod audio;
mod debug;
mod queries;

use super::Emulator;

impl Emulator {
    pub fn set_input(&mut self, buttons: u8, dpad: u8) {
        if self.bus.apply_joypad_pressed_masks(buttons, dpad) {
            self.bus.if_reg |= 0x10;
        }
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        self.bus.write_byte(addr, value);
    }

    pub fn ppu_bg_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.bus.ppu_bg_palette_ram_snapshot()
    }

    pub fn ppu_obj_palette_ram_snapshot(&self) -> [u8; 0x40] {
        self.bus.ppu_obj_palette_ram_snapshot()
    }

    pub fn set_ppu_debug_flags(&mut self, bg: bool, window: bool, sprites: bool) {
        self.bus.set_ppu_debug_flags(bg, window, sprites);
    }

    pub fn set_dmg_palette_preset(&mut self, preset: crate::hardware::ppu::DmgPalettePreset) {
        self.bus.set_ppu_dmg_palette_preset(preset);
    }

    pub fn set_sgb_border_enabled(&mut self, enabled: bool) {
        self.bus.set_ppu_sgb_border_enabled(enabled);
    }

    pub fn sgb_border_active(&self) -> bool {
        self.bus.ppu_sgb_border_active()
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.ppu_framebuffer_dimensions()
    }

    pub fn clear_rom_patches(&mut self) {
        self.bus.game_genie_patches.clear();
    }

    pub fn add_rom_patch(&mut self, patch: crate::cheats::CheatPatch) {
        self.bus.game_genie_patches.push(patch);
    }

    pub fn rom_patches(&self) -> &[crate::cheats::CheatPatch] {
        &self.bus.game_genie_patches
    }
}

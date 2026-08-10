use super::{ActiveCore, CoreState};

impl CoreState {
    pub fn cheat_reset(&mut self) {
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.clear_rom_patches(),
            ActiveCore::Gba(_) => {}
            ActiveCore::Nes(emu) => emu.clear_game_genie(),
            ActiveCore::Sega8(_) => {}
            ActiveCore::Ws(_) => {}
        }
    }

    pub fn cheat_set(&mut self, code: &str) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                if let Ok((patches, _)) = zeff_gb_core::cheats::parse_cheat(code) {
                    for p in patches {
                        emu.add_rom_patch(p);
                    }
                }
            }
            ActiveCore::Gba(_) => {}
            ActiveCore::Nes(emu) => {
                if let Some(patch) = zeff_nes_core::cheats::decode_nes_game_genie(code) {
                    emu.add_game_genie_patch(patch);
                }
            }
            ActiveCore::Sega8(_) => {}
            ActiveCore::Ws(_) => {}
        }
    }
}

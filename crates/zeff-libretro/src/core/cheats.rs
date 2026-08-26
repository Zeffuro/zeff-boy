use super::{ActiveCore, CoreState};
use zeff_emu_common::cheats::{
    CheatPatch, apply_ram_cheats_16, apply_wide_ram_cheats,
    parse_gba_codebreaker_cheats_with_state, parse_raw16_cheat, parse_raw16_cheats,
    parse_wide_raw_cheats,
};

impl CoreState {
    pub fn cheat_reset(&mut self) {
        self.ram_cheats.clear();
        self.gba_codebreaker_state.reset();
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.clear_rom_patches(),
            ActiveCore::Gba(_) => {}
            ActiveCore::Nes(emu) => emu.clear_game_genie(),
            ActiveCore::Pce(_) => {}
            ActiveCore::Sega8(emu) => emu.clear_rom_patches(),
            ActiveCore::Ws(_) => {}
        }
    }

    pub fn cheat_set(&mut self, code: &str) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                if let Ok((patches, _)) = zeff_gb_core::cheats::parse_cheat(code) {
                    for patch in patches.into_iter().map(resolve_libretro_patch_parameter) {
                        if patch.is_rom_patch() {
                            emu.add_rom_patch(patch);
                        } else if matches!(
                            patch,
                            CheatPatch::RamWrite { .. } | CheatPatch::RamWriteIfEquals { .. }
                        ) {
                            self.ram_cheats.push(patch);
                        }
                    }
                }
            }
            ActiveCore::Gba(_) => {
                if let Some(patches) = parse_wide_raw_cheats(code).or_else(|| {
                    parse_gba_codebreaker_cheats_with_state(code, &mut self.gba_codebreaker_state)
                }) {
                    self.ram_cheats.extend(patches);
                }
            }
            ActiveCore::Nes(emu) => {
                if let Some(patch) = zeff_nes_core::cheats::decode_nes_game_genie(code) {
                    emu.add_game_genie_patch(patch);
                } else {
                    self.ram_cheats
                        .extend(parse_raw16_cheats(code).unwrap_or_default());
                }
            }
            ActiveCore::Pce(_) => {
                self.ram_cheats
                    .extend(parse_pce_cheats(code).unwrap_or_default());
            }
            ActiveCore::Sega8(emu) => {
                if let Ok((patches, _)) = zeff_sega8_core::cheats::parse_cheat(code) {
                    for patch in patches.into_iter().map(resolve_libretro_patch_parameter) {
                        if patch.is_rom_patch() {
                            emu.add_rom_patch(patch);
                        } else if matches!(
                            patch,
                            CheatPatch::RamWrite { .. } | CheatPatch::RamWriteIfEquals { .. }
                        ) {
                            self.ram_cheats.push(patch);
                        }
                    }
                }
            }
            ActiveCore::Ws(_) => {
                self.ram_cheats
                    .extend(parse_wide_raw_cheats(code).unwrap_or_default());
            }
        }
    }

    pub fn apply_ram_cheats(&mut self) {
        let cheats = self.ram_cheats.as_slice();
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                apply_ram_cheats_16(emu.as_mut(), cheats);
            }
            ActiveCore::Gba(emu) => {
                apply_wide_ram_cheats(emu.as_mut(), cheats);
            }
            ActiveCore::Nes(emu) => {
                apply_ram_cheats_16(emu.as_mut(), cheats);
            }
            ActiveCore::Pce(host) => {
                host.apply_cheats(cheats);
            }
            ActiveCore::Sega8(emu) => {
                apply_ram_cheats_16(emu.as_mut(), cheats);
            }
            ActiveCore::Ws(emu) => {
                apply_wide_ram_cheats(emu.as_mut(), cheats);
            }
        }
    }
}

fn parse_pce_cheats(input: &str) -> Option<Vec<CheatPatch>> {
    let mut patches = Vec::new();
    for part in input.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        patches.push(parse_raw16_cheat(part).or_else(|| parse_pce_physical_ram_cheat(part))?);
    }
    (!patches.is_empty()).then_some(patches)
}

fn parse_pce_physical_ram_cheat(input: &str) -> Option<CheatPatch> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (address_text, value_text) = cleaned
        .split_once(':')
        .or_else(|| cleaned.split_once('='))?;
    let address_text = address_text
        .strip_prefix("0x")
        .or_else(|| address_text.strip_prefix("0X"))
        .or_else(|| address_text.strip_prefix('$'))
        .unwrap_or(address_text);
    if address_text.len() != 6 || value_text.len() != 2 {
        return None;
    }
    let address = u32::from_str_radix(address_text, 16).ok()?;
    let value = u8::from_str_radix(value_text, 16).ok()?;
    if !(0x1F_0000..=0x1F_7FFF).contains(&address) {
        return None;
    }
    Some(CheatPatch::WideRamWrite {
        address,
        value: zeff_emu_common::cheats::CheatValue::Constant(value),
    })
}

fn resolve_libretro_patch_parameter(patch: CheatPatch) -> CheatPatch {
    if patch.has_user_parameter() {
        patch.resolve_user_parameter(patch.default_user_value().unwrap_or(0))
    } else {
        patch
    }
}

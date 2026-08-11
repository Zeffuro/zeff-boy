use super::{ActiveCore, CoreState};
use zeff_emu_common::cheats::{CheatPatch, CheatValue};

impl CoreState {
    pub fn cheat_reset(&mut self) {
        self.ram_cheats.clear();
        match &mut self.core {
            ActiveCore::Gb(emu) => emu.clear_rom_patches(),
            ActiveCore::Gba(_) => {}
            ActiveCore::Nes(emu) => emu.clear_game_genie(),
            ActiveCore::Sega8(emu) => emu.clear_rom_patches(),
            ActiveCore::Ws(_) => {}
        }
    }

    pub fn cheat_set(&mut self, code: &str) {
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                if let Ok((patches, _)) = zeff_gb_core::cheats::parse_cheat(code) {
                    for patch in patches {
                        match patch {
                            CheatPatch::RomWrite { .. } | CheatPatch::RomWriteIfEquals { .. } => {
                                emu.add_rom_patch(patch);
                            }
                            CheatPatch::RamWrite { .. } | CheatPatch::RamWriteIfEquals { .. } => {
                                self.ram_cheats.push(patch);
                            }
                            CheatPatch::WideRamWrite { .. }
                            | CheatPatch::WideRamWriteIfEquals { .. } => {}
                        }
                    }
                }
            }
            ActiveCore::Gba(_) => {
                self.ram_cheats.extend(parse_wide_raw_cheats(code));
            }
            ActiveCore::Nes(emu) => {
                if let Some(patch) = zeff_nes_core::cheats::decode_nes_game_genie(code) {
                    emu.add_game_genie_patch(patch);
                } else {
                    self.ram_cheats.extend(parse_raw16_cheats(code));
                }
            }
            ActiveCore::Sega8(emu) => {
                if let Ok((patches, _)) = zeff_sega8_core::cheats::parse_cheat(code) {
                    for patch in patches {
                        match patch {
                            CheatPatch::RomWrite { .. } | CheatPatch::RomWriteIfEquals { .. } => {
                                emu.add_rom_patch(patch);
                            }
                            CheatPatch::RamWrite { .. } | CheatPatch::RamWriteIfEquals { .. } => {
                                self.ram_cheats.push(patch);
                            }
                            CheatPatch::WideRamWrite { .. }
                            | CheatPatch::WideRamWriteIfEquals { .. } => {}
                        }
                    }
                }
            }
            ActiveCore::Ws(_) => {
                self.ram_cheats.extend(parse_wide_raw_cheats(code));
            }
        }
    }

    pub fn apply_ram_cheats(&mut self) {
        let cheats = self.ram_cheats.as_slice();
        match &mut self.core {
            ActiveCore::Gb(emu) => {
                for patch in cheats {
                    match *patch {
                        CheatPatch::RamWrite { address, value } => {
                            let current = emu.peek_byte_raw(address);
                            emu.write_byte(address, value.resolve_with_current(current));
                        }
                        CheatPatch::RamWriteIfEquals {
                            address,
                            value,
                            compare,
                        } => {
                            let current = emu.peek_byte_raw(address);
                            if compare.matches(current) {
                                emu.write_byte(address, value.resolve_with_current(current));
                            }
                        }
                        _ => {}
                    }
                }
            }
            ActiveCore::Gba(emu) => {
                for patch in cheats {
                    match *patch {
                        CheatPatch::WideRamWrite { address, value } => {
                            let current = emu.cpu_peek8(address);
                            emu.cpu_write8(address, value.resolve_with_current(current));
                        }
                        CheatPatch::WideRamWriteIfEquals {
                            address,
                            value,
                            compare,
                        } => {
                            let current = emu.cpu_peek8(address);
                            if compare.matches(current) {
                                emu.cpu_write8(address, value.resolve_with_current(current));
                            }
                        }
                        _ => {}
                    }
                }
            }
            ActiveCore::Nes(emu) => {
                for patch in cheats {
                    match *patch {
                        CheatPatch::RamWrite { address, value } => {
                            let CheatValue::Constant(value) = value else {
                                continue;
                            };
                            emu.cpu_write(address, value);
                        }
                        CheatPatch::RamWriteIfEquals {
                            address,
                            value,
                            compare,
                        } => {
                            let current = emu.cpu_peek(address);
                            if compare.matches(current) {
                                let CheatValue::Constant(value) = value else {
                                    continue;
                                };
                                emu.cpu_write(address, value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            ActiveCore::Sega8(emu) => {
                for patch in cheats {
                    match *patch {
                        CheatPatch::RamWrite { address, value } => {
                            let current = emu.cpu_peek8(address);
                            emu.cpu_write8(address, value.resolve_with_current(current));
                        }
                        CheatPatch::RamWriteIfEquals {
                            address,
                            value,
                            compare,
                        } => {
                            let current = emu.cpu_peek8(address);
                            if compare.matches(current) {
                                emu.cpu_write8(address, value.resolve_with_current(current));
                            }
                        }
                        _ => {}
                    }
                }
            }
            ActiveCore::Ws(emu) => {
                for patch in cheats {
                    match *patch {
                        CheatPatch::WideRamWrite { address, value } => {
                            let current = emu.cpu_peek8(address);
                            emu.cpu_write8(address, value.resolve_with_current(current));
                        }
                        CheatPatch::WideRamWriteIfEquals {
                            address,
                            value,
                            compare,
                        } => {
                            let current = emu.cpu_peek8(address);
                            if compare.matches(current) {
                                emu.cpu_write8(address, value.resolve_with_current(current));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn parse_raw16_cheats(input: &str) -> Vec<CheatPatch> {
    parse_raw_cheat_parts(input, parse_raw16_cheat).unwrap_or_default()
}

fn parse_wide_raw_cheats(input: &str) -> Vec<CheatPatch> {
    parse_raw_cheat_parts(input, parse_wide_raw_cheat).unwrap_or_default()
}

fn parse_raw_cheat_parts(
    input: &str,
    parser: fn(&str) -> Option<CheatPatch>,
) -> Option<Vec<CheatPatch>> {
    let mut patches = Vec::new();
    for part in input.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        patches.push(parser(part)?);
    }
    (!patches.is_empty()).then_some(patches)
}

fn parse_raw16_cheat(input: &str) -> Option<CheatPatch> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (address_text, value_text) = cleaned
        .split_once(':')
        .or_else(|| cleaned.split_once('='))?;
    let address = parse_hex_u16_exact(address_text)?;
    let value = parse_hex_u8_exact(value_text)?;
    Some(CheatPatch::RamWrite {
        address,
        value: CheatValue::constant(value),
    })
}

fn parse_wide_raw_cheat(input: &str) -> Option<CheatPatch> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (address_text, value_text) = cleaned
        .split_once(':')
        .or_else(|| cleaned.split_once('='))?;
    let address = parse_hex_u32_exact(address_text)?;
    let value = parse_hex_u8_exact(value_text)?;
    Some(CheatPatch::WideRamWrite {
        address,
        value: CheatValue::constant(value),
    })
}

fn parse_hex_u16_exact(input: &str) -> Option<u16> {
    let token = normalize_hex_token(input)?;
    if token.len() != 4 {
        return None;
    }
    u16::from_str_radix(token, 16).ok()
}

fn parse_hex_u32_exact(input: &str) -> Option<u32> {
    let token = normalize_hex_token(input)?;
    if token.len() != 8 {
        return None;
    }
    u32::from_str_radix(token, 16).ok()
}

fn parse_hex_u8_exact(input: &str) -> Option<u8> {
    let token = normalize_hex_token(input)?;
    if token.len() != 2 {
        return None;
    }
    u8::from_str_radix(token, 16).ok()
}

fn normalize_hex_token(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let token = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .or_else(|| trimmed.strip_prefix('$'))
        .unwrap_or(trimmed);
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(token)
}

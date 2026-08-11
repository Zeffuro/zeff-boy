pub use zeff_emu_common::cheats::{CheatPatch, CheatType, CheatValue};

pub fn parse_cheat(input: &str) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty cheat code");
    }

    if let Some(result) = try_parse_single(trimmed) {
        return Ok(result);
    }

    let parts: Vec<&str> = trimmed.split('+').collect();
    if parts.len() > 1 {
        let mut all_patches = Vec::new();
        let mut detected_type = None;

        for part in &parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match try_parse_single(part) {
                Some((patches, ty)) => {
                    detected_type = Some(ty);
                    all_patches.extend(patches);
                }
                None => {
                    return Err(
                        "Unrecognized Sega 8-bit multi-code. Use raw (AAAA:VV), Action Replay (00AA-AAVV), or Game Genie (XXX-XXX-XXX)",
                    );
                }
            }
        }

        if let Some(ty) = detected_type
            && !all_patches.is_empty()
        {
            return Ok((all_patches, ty));
        }
    }

    Err(
        "Unrecognized Sega 8-bit cheat. Use raw (AAAA:VV), Action Replay (00AA-AAVV), or Game Genie (XXX-XXX-XXX)",
    )
}

fn try_parse_single(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    try_parse_raw(input)
        .or_else(|| try_parse_action_replay(input))
        .or_else(|| try_parse_game_genie(input))
}

fn try_parse_raw(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned = remove_whitespace(input);
    let (address_text, value_text) = cleaned
        .split_once(':')
        .or_else(|| cleaned.split_once('='))?;
    let address = parse_hex_u16_exact(address_text)?;
    let value = parse_hex_u8_exact(value_text)?;
    Some((
        vec![CheatPatch::RamWrite {
            address,
            value: CheatValue::constant(value),
        }],
        CheatType::Raw,
    ))
}

fn try_parse_action_replay(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned = remove_whitespace_and_dashes(input);
    if cleaned.len() != 8 || !cleaned.starts_with("00") {
        return None;
    }

    let address = parse_hex_u16_exact(&cleaned[2..6])?;
    let value = CheatValue::from_gameshark_value(&cleaned[6..8])?;
    Some((
        vec![CheatPatch::RamWrite { address, value }],
        CheatType::ActionReplay,
    ))
}

fn try_parse_game_genie(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned = remove_whitespace_and_dashes(input);
    if cleaned.len() != 6 && cleaned.len() != 9 {
        return None;
    }

    let n = parse_hex_nybbles(&cleaned)?;
    let value = CheatValue::constant((n[0] << 4) | n[1]);
    let address = ((u16::from(n[4]) | (u16::from(n[5] ^ 0xF) << 4)) << 8)
        | u16::from(n[2])
        | (u16::from(n[3]) << 4);

    let patch = if cleaned.len() == 9 {
        let op3 = (u16::from(n[6]) << 8) | (u16::from(n[7]) << 4) | u16::from(n[8]);
        CheatPatch::RomWriteIfEquals {
            address,
            value,
            compare: CheatValue::constant(decode_game_genie_compare(op3)),
        }
    } else {
        CheatPatch::RomWrite { address, value }
    };

    Some((vec![patch], CheatType::GameGenie))
}

fn decode_game_genie_compare(op3: u16) -> u8 {
    let op = u32::from(op3);
    let packed = ((op & 0x0F00) << 20) | (op & 0x000F);
    let rotated = packed.rotate_right(2);
    let folded = rotated | (rotated >> 24);
    (folded as u8) ^ 0xBA
}

fn parse_hex_nybbles(input: &str) -> Option<Vec<u8>> {
    input
        .chars()
        .map(|c| c.to_digit(16).map(|v| v as u8))
        .collect()
}

fn parse_hex_u16_exact(input: &str) -> Option<u16> {
    let token = normalize_hex_token(input)?;
    if token.len() != 4 {
        return None;
    }
    u16::from_str_radix(token, 16).ok()
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

fn remove_whitespace(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace()).collect()
}

fn remove_whitespace_and_dashes(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_ram_cheat() {
        let (patches, ty) = parse_cheat("C123:42").expect("raw cheat should parse");
        assert_eq!(ty, CheatType::Raw);
        assert!(matches!(
            patches.as_slice(),
            [CheatPatch::RamWrite {
                address: 0xC123,
                value: CheatValue::Constant(0x42)
            }]
        ));
    }

    #[test]
    fn parses_raw_ram_multi_code() {
        let (patches, ty) = parse_cheat("$C000:01+0xD000=02").expect("multi-code should parse");
        assert_eq!(ty, CheatType::Raw);
        assert_eq!(patches.len(), 2);
        assert!(matches!(
            patches[0],
            CheatPatch::RamWrite {
                address: 0xC000,
                value: CheatValue::Constant(0x01),
            }
        ));
        assert!(matches!(
            patches[1],
            CheatPatch::RamWrite {
                address: 0xD000,
                value: CheatValue::Constant(0x02),
            }
        ));
    }

    #[test]
    fn raw_ram_cheats_require_full_width() {
        assert!(parse_cheat("C00:01").is_err());
        assert!(parse_cheat("C000:1").is_err());
    }

    #[test]
    fn parses_sms_action_replay_ram_cheat() {
        let (patches, ty) = parse_cheat("00D2-AA98").expect("Action Replay cheat should parse");
        assert_eq!(ty, CheatType::ActionReplay);
        assert!(matches!(
            patches.as_slice(),
            [CheatPatch::RamWrite {
                address: 0xD2AA,
                value: CheatValue::Constant(0x98)
            }]
        ));
    }

    #[test]
    fn parses_sms_action_replay_user_parameter() {
        let (patches, ty) =
            parse_cheat("00D2-3EXX").expect("Action Replay parameter cheat should parse");
        assert_eq!(ty, CheatType::ActionReplay);
        assert!(matches!(
            patches.as_slice(),
            [CheatPatch::RamWrite {
                address: 0xD23E,
                value: CheatValue::UserParameterized {
                    mask: 0xFF,
                    base: 0
                }
            }]
        ));
    }

    #[test]
    fn parses_sms_game_genie_rom_cheat() {
        let (patches, ty) = parse_cheat("006-46F-F7A").expect("Game Genie cheat should parse");
        assert_eq!(ty, CheatType::GameGenie);
        assert!(matches!(
            patches.as_slice(),
            [CheatPatch::RomWriteIfEquals {
                address: 0x0646,
                value: CheatValue::Constant(0x00),
                compare: CheatValue::Constant(0x04)
            }]
        ));
    }

    #[test]
    fn parses_sms_game_genie_without_compare() {
        let (patches, ty) = parse_cheat("006-46F").expect("Game Genie cheat should parse");
        assert_eq!(ty, CheatType::GameGenie);
        assert!(matches!(
            patches.as_slice(),
            [CheatPatch::RomWrite {
                address: 0x0646,
                value: CheatValue::Constant(0x00)
            }]
        ));
    }
}

use super::types::{CheatPatch, CheatType, CheatValue};

fn gg_char_to_nybble(c: char) -> Option<u8> {
    match c.to_ascii_uppercase() {
        'D' => Some(0x0),
        'E' => Some(0x1),
        'F' => Some(0x2),
        'G' => Some(0x3),
        'H' => Some(0x4),
        'I' => Some(0x5),
        'J' => Some(0x6),
        'K' => Some(0x7),
        'L' => Some(0x8),
        'M' => Some(0x9),
        'N' => Some(0xA),
        'O' => Some(0xB),
        'P' => Some(0xC),
        'Q' => Some(0xD),
        'R' => Some(0xE),
        'S' => Some(0xF),
        _ => None,
    }
}

fn gg_hex_char_to_nybble(c: char) -> Option<u8> {
    c.to_digit(16).map(|v| v as u8)
}

fn parse_gg_nybbles(cleaned: &str) -> Option<Vec<u8>> {
    cleaned
        .chars()
        .map(gg_char_to_nybble)
        .collect::<Option<Vec<_>>>()
        .or_else(|| {
            cleaned
                .chars()
                .map(gg_hex_char_to_nybble)
                .collect::<Option<Vec<_>>>()
        })
}

pub(super) fn try_parse_game_genie(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    if cleaned.len() != 6 && cleaned.len() != 9 {
        return None;
    }

    let n = parse_gg_nybbles(&cleaned)?;

    let value = CheatValue::constant((n[0] << 4) | n[1]);

    let address = ((u16::from(n[4]) | (u16::from(n[5] ^ 0xF) << 4)) << 8)
        | u16::from(n[2])
        | (u16::from(n[3]) << 4);

    let patch = if cleaned.len() == 9 {
        let op3 = (u16::from(n[6]) << 8) | (u16::from(n[7]) << 4) | u16::from(n[8]);
        let compare = CheatValue::constant(decode_gg_compare(op3));

        CheatPatch::RomWriteIfEquals {
            address,
            value,
            compare,
        }
    } else {
        CheatPatch::RomWrite { address, value }
    };

    Some((vec![patch], CheatType::GameGenie))
}

#[inline]
fn decode_gg_compare(op3: u16) -> u8 {
    let op = op3 as u32;
    let packed = ((op & 0x0F00) << 20) | (op & 0x000F);
    let rotated = packed.rotate_right(2);
    let folded = rotated | (rotated >> 24);
    (folded as u8) ^ 0xBA
}

pub(super) fn try_parse_raw(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (addr_str, val_str) = cleaned.split_once(':')?;

    let addr = u16::from_str_radix(addr_str, 16).ok()?;
    let value = CheatValue::constant(u8::from_str_radix(val_str, 16).ok()?);

    Some((
        vec![CheatPatch::RamWrite {
            address: addr,
            value,
        }],
        CheatType::Raw,
    ))
}

pub(super) fn try_parse_gameshark(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let parts: Vec<&str> = input.split('+').collect();
    let mut patches = Vec::new();

    for part in parts {
        let (p, _) = try_parse_gameshark_single(part)?;
        patches.extend(p);
    }

    Some((patches, CheatType::GameShark))
}

pub(super) fn try_parse_gameshark_single(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    if cleaned.len() != 8 {
        return None;
    }

    let code_type_byte = u8::from_str_radix(&cleaned[0..2], 16).ok()?;
    let value = CheatValue::from_gameshark_value(&cleaned[2..4])?;
    let addr_low = u8::from_str_radix(&cleaned[4..6], 16).ok()?;
    let addr_high = u8::from_str_radix(&cleaned[6..8], 16).ok()?;
    let address = (u16::from(addr_high) << 8) | u16::from(addr_low);

    let patch = match code_type_byte {
        0x01 | 0x80 | 0x90 | 0x91 | 0x96 | 0x95 => CheatPatch::RamWrite { address, value },
        _ => {
            log::warn!(
                "Unsupported GameShark opcode {:02X}, treating as RAM write",
                code_type_byte
            );
            CheatPatch::RamWrite { address, value }
        }
    };

    Some((vec![patch], CheatType::GameShark))
}

pub(super) fn try_parse_xploder(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let parts: Vec<&str> = input.split('+').collect();
    let mut patches = Vec::new();

    for part in parts {
        let cleaned: String = part.chars().filter(|c| !c.is_whitespace()).collect();
        let hex = cleaned.strip_prefix('$')?;

        if hex.len() != 8 {
            return None;
        }

        let code_type = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let value = CheatValue::constant(u8::from_str_radix(&hex[2..4], 16).ok()?);
        let addr_hi = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let addr_lo = u8::from_str_radix(&hex[6..8], 16).ok()?;
        let address = (u16::from(addr_hi) << 8) | u16::from(addr_lo);

        let patch = match code_type {
            0x0D => CheatPatch::RamWrite { address, value },
            _ => {
                log::warn!(
                    "Unknown XPloder opcode {:02X}, treating as RAM write",
                    code_type
                );
                CheatPatch::RamWrite { address, value }
            }
        };

        patches.push(patch);
    }

    Some((patches, CheatType::XPloder))
}


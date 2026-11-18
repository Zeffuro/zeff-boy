use zeff_emu_common::cheats::{CheatPatch, CheatType, CheatValue};

const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPRSTVWXYZ0123456789";
const ADDRESS_BITS: [u8; 24] = [
    35, 36, 37, 38, 39, 30, 31, 32, 19, 10, 11, 12, 13, 14, 5, 6, 21, 22, 23, 24, 15, 16, 17, 18,
];
const DATA_BITS: [u8; 16] = [7, 8, 9, 0, 1, 2, 3, 4, 26, 27, 28, 29, 20, 33, 34, 25];

fn permute(packed: u64, positions: &[u8]) -> u32 {
    positions
        .iter()
        .enumerate()
        .fold(0, |result, (bit, source)| {
            result | (((packed >> source) & 1) as u32) << bit
        })
}

pub fn decode_game_genie(input: &str) -> Option<(u32, u16)> {
    let bytes = input.as_bytes();
    if bytes.len() != 9 || bytes[4] != b'-' {
        return None;
    }
    let mut packed = 0;
    for (index, byte) in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (index != 4).then_some(byte))
        .enumerate()
    {
        let value = ALPHABET
            .iter()
            .position(|&entry| entry == byte.to_ascii_uppercase())?;
        packed |= (value as u64) << (index * 5);
    }
    Some((
        permute(packed, &ADDRESS_BITS),
        permute(packed, &DATA_BITS) as u16,
    ))
}

fn token(input: &str) -> &str {
    input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .or_else(|| input.strip_prefix('$'))
        .unwrap_or(input)
}

fn byte_patch(address: u32, value: u8) -> Option<CheatPatch> {
    let value = CheatValue::Constant(value);
    match address {
        0..=0x7fffff => Some(CheatPatch::WideRomWrite { address, value }),
        0xff0000..=0xffffff => Some(CheatPatch::WideRamWrite { address, value }),
        _ => None,
    }
}

fn raw_part(input: &str) -> Option<Vec<CheatPatch>> {
    let (address, value) = input.split_once(':').or_else(|| input.split_once('='))?;
    let address = token(address);
    let value = token(value);
    if !matches!(address.len(), 6 | 8)
        || !matches!(value.len(), 2 | 4)
        || !address
            .bytes()
            .chain(value.bytes())
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let address = u32::from_str_radix(address, 16).ok()?;
    let mut result = vec![byte_patch(
        address,
        u8::from_str_radix(&value[..2], 16).ok()?,
    )?];
    if value.len() == 4 {
        if matches!(address, 0x7fffff | 0xffffff) {
            return None;
        }
        result.push(byte_patch(
            address + 1,
            u8::from_str_radix(&value[2..], 16).ok()?,
        )?);
    }
    Some(result)
}

fn source_part(input: &str) -> Option<Vec<CheatPatch>> {
    let (address, value) = input.split_once(':')?;
    let address = token(address);
    let value = token(value);
    if address.len() != 8
        || !matches!(value.len(), 2 | 4)
        || !address
            .bytes()
            .chain(value.bytes())
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let address = u32::from_str_radix(address, 16).ok()?;
    let width = (value.len() / 2) as u32;
    if address.checked_add(width)? > crate::constants::MAX_BANKED_CARTRIDGE_SIZE as u32
        || width == 2 && address & 1 != 0
    {
        return None;
    }
    (0..width)
        .map(|index| {
            let start = index as usize * 2;
            Some(CheatPatch::WideRomWrite {
                address: address + index,
                value: CheatValue::Constant(u8::from_str_radix(&value[start..start + 2], 16).ok()?),
            })
        })
        .collect()
}

pub fn parse_cheats(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let mut patches = Vec::new();
    let mut format = CheatType::Raw;
    for part in input.split('+') {
        let part: String = part.chars().filter(|c| !c.is_whitespace()).collect();
        if part.is_empty() {
            continue;
        }
        if part
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ROM:"))
        {
            patches.extend(source_part(&part[4..])?);
        } else if part.contains('-') {
            let (address, value) = decode_game_genie(&part)?;
            if address > 0x3fffff {
                return None;
            }
            let address = address & !1;
            patches.push(byte_patch(address, (value >> 8) as u8)?);
            patches.push(byte_patch(address + 1, value as u8)?);
            format = CheatType::GameGenie;
        } else {
            patches.extend(raw_part(&part)?);
        }
    }
    (!patches.is_empty()).then_some((patches, format))
}

#[cfg(test)]
mod tests;

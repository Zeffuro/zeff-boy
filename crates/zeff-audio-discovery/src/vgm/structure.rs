use super::*;

pub(crate) fn command(
    opcode: u8,
    data: &[u8],
    pos: usize,
    version: u32,
) -> Option<(usize, u64, bool)> {
    let minimum = match opcode {
        0x70..=0x7f => 0x110,
        0x67 | 0x80..=0x8f | 0xe0 => 0x150,
        0x30 | 0x3f | 0x55..=0x5f | 0xa0..=0xb2 | 0xc0..=0xc3 | 0xd0..=0xd1 => 0x151,
        0x68 | 0x90..=0x95 => 0x160,
        0xb3..=0xbb | 0xc4 | 0xd2..=0xd4 => 0x161,
        0x31 | 0xbc..=0xbf | 0xc5..=0xc8 | 0xd5..=0xd6 | 0xe1 => 0x171,
        _ => 0x100,
    };
    if version < minimum {
        return None;
    }
    if opcode == 0x67 {
        if data.get(pos + 1) != Some(&0x66) {
            return None;
        }
        let kind = *data.get(pos + 2)?;
        let size_field = u32le(data, pos + 3)?;
        if (kind < 0x80 || version < 0x151) && size_field & 0x8000_0000 != 0 {
            return None;
        }
        let size = (size_field & 0x7fff_ffff) as usize;
        let len = 7usize.checked_add(size)?;
        let payload = data.get(pos + 7..pos.checked_add(len)?)?;
        match kind {
            0x40..=0x7e if version >= 0x160 => {
                payload.get(..5)?;
                if matches!(payload[0], 0 | 1) {
                    payload.get(..10)?;
                }
            }
            0x7f if version >= 0x160 => {
                payload.get(..6)?;
            }
            0x80..=0xbf if version >= 0x151 => {
                let rom_size = u32le(payload, 0)?;
                let start = u32le(payload, 4)?;
                if start.checked_add(size.checked_sub(8)? as u32)? > rom_size {
                    return None;
                }
            }
            0xc0..=0xdf if version >= 0x151 => {
                payload.get(..2)?;
            }
            0xe0..=0xff if version >= 0x151 => {
                payload.get(..4)?;
            }
            _ => {}
        }
        return Some((len, 0, false));
    }
    let fixed = match opcode {
        0x00 => (1, 0, true),
        0x30 | 0x31 | 0x3f => (2, 0, false),
        0x32..=0x3e => (2, 0, true),
        0x40..=0x4e => (if version <= 0x160 { 2 } else { 3 }, 0, true),
        0x4f | 0x50 => (2, 0, false),
        0x51..=0x5f | 0xa0..=0xbf => (3, 0, false),
        0x61 => (3, u16le(data, pos + 1)? as u64, false),
        0x62 => (1, 735, false),
        0x63 => (1, 882, false),
        0x66 => (1, 0, false),
        0x68 => {
            if data.get(pos + 1) != Some(&0x66) {
                return None;
            }
            (12, 0, false)
        }
        0x70..=0x7f => (1, u64::from((opcode & 15) + 1), false),
        0x80..=0x8f => (1, u64::from(opcode & 15), false),
        0x90 | 0x91 | 0x95 => (5, 0, false),
        0x92 => (6, 0, false),
        0x93 => (11, 0, false),
        0x94 => (2, 0, false),
        0xc0..=0xdf => (4, 0, (0xc9..=0xcf).contains(&opcode) || opcode >= 0xd7),
        0xe0..=0xff => (5, 0, opcode >= 0xe2),
        _ => return None,
    };
    data.get(pos..pos.checked_add(fixed.0)?)?;
    Some(fixed)
}

pub(crate) fn gd3(
    data: &[u8],
    eof: usize,
    command_end: usize,
    budget: &mut Budget<'_>,
    stopped: &mut Option<ScanStop>,
    encoding: VgmEncoding,
) -> Option<(Option<VgmSpan>, String)> {
    let relative = u32le(data, 0x14)?;
    if relative == 0 {
        return Some((None, String::new()));
    }
    let start = 0x14usize.checked_add(relative as usize)?;
    if start < command_end
        || start >= eof
        || data.get(start..start + 12)?.get(..4)? != b"Gd3 "
        || u32le(data, start + 4)? != 0x100
    {
        return None;
    }
    let len = u32le(data, start + 8)? as usize;
    if len > MAX_GD3_BYTES || !len.is_multiple_of(2) {
        return None;
    }
    let end = start.checked_add(12)?.checked_add(len)?;
    if end > eof {
        return None;
    }
    let mut strings = Vec::new();
    let mut field = Vec::new();
    for pair in data[start + 12..end].as_chunks::<2>().0 {
        charge(budget, stopped)?;
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        if value == 0 {
            strings.push(String::from_utf16(&field).ok()?);
            field.clear();
        } else {
            field.push(value);
        }
    }
    if !field.is_empty() || strings.len() != 11 {
        return None;
    }
    Some((
        Some(span(encoding, start, end - start)),
        strings.into_iter().next().unwrap_or_default(),
    ))
}

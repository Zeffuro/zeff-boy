use anyhow::ensure;

use super::super::{GaxNativeLayout, GaxNativeSong, INIT_SIGNATURES, half, rom_pointer, word};

pub(super) fn lower_rate(bytes: &[u8], song: &GaxNativeSong) -> anyhow::Result<Option<u16>> {
    if song.native.layout != GaxNativeLayout::V2Current || song.native.sample_rate != u16::MAX {
        return Ok(None);
    }
    let init = song.native.init.source.effective_offset as usize;
    if !bytes
        .get(init..)
        .is_some_and(|tail| tail.starts_with(INIT_SIGNATURES[2].bytes))
        || !matches(
            bytes,
            init + 0x3e,
            &[
                0x8938, 0x4900, 0x4288, 0xd104, 0x6b38, 0x6880, 0x6980, 0x8b00, 0x8138, 0x8978,
                0x4288, 0xd117, 0x6b38, 0x6880, 0x6981, 0x8b08, 0x2801, 0xd910, 0x7ec9, 0x0148,
                0x1a40, 0x0080, 0x1840, 0x00c0, 0xe009,
            ],
        )
        || literal(bytes, init + 0x40, 1) != Some(u32::from(u16::MAX))
        || !matches(
            bytes,
            init + 0x7c,
            &[0x1c08, 0xe01d, 0x1c08, 0xe02a, 0x8178],
        )
        || !matches(
            bytes,
            init + 0xa2,
            &[
                0x893b, 0x2100, 0x4a00, 0x4690, 0x897c, 0x6810, 0x4298, 0xd2e4, 0x3208, 0x3101,
                0x290c, 0xd9f8, 0x200c, 0x00c0, 0x4440, 0x6800, 0x8138, 0x1c23, 0x2100, 0x4642,
                0x6810, 0x4298, 0xd2d7, 0x3208, 0x3101, 0x290c, 0xd9f8, 0x200c, 0x00c0, 0x4440,
                0x6800, 0x8178,
            ],
        )
        || !matches(
            bytes,
            init + 0x1ec,
            &[
                0x68b0, 0x6980, 0x69c0, 0x2800, 0xd03e, 0x8a38, 0x2802, 0xd83b, 0x314c, 0x2001,
                0x7008,
            ],
        )
        || !matches(
            bytes,
            init + 0x27e,
            &[
                0x8978, 0x893c, 0x42a0, 0xd108, 0x4e00, 0x6830, 0x304c, 0x7800, 0x2800, 0xd00e,
                0x8a38, 0x2800, 0xd00b, 0x4800, 0x6801, 0x9803, 0x6188, 0x3008, 0x9003, 0x9804,
                0x3808, 0x9004,
            ],
        )
        || !matches(bytes, init + 0x2b0, &[0x4a00, 0x6811, 0x2000, 0x6188])
    {
        return Ok(None);
    }
    let Some(state) = literal(bytes, init + 0x22, 0) else {
        return Ok(None);
    };
    if !super::super::state_pointer(state)
        || literal(bytes, init + 0x286, 6) != Some(state)
        || literal(bytes, init + 0x298, 0) != Some(state)
        || literal(bytes, init + 0x2b0, 2) != Some(state)
        || !optimized_play(bytes, song, state)
    {
        return Ok(None);
    }
    let Some(table) =
        literal(bytes, init + 0xa6, 2).and_then(|address| rom_pointer(bytes, address, 13 * 8, 4))
    else {
        return Ok(None);
    };
    let mut rates = [0; 13];
    for (index, rate) in rates.iter_mut().enumerate() {
        let Some(value) =
            word(bytes, table + index * 8).and_then(|value| u16::try_from(value).ok())
        else {
            return Ok(None);
        };
        *rate = value;
    }
    if rates[0] == 0 || rates.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Ok(None);
    }
    let header = song.header.effective_offset as usize;
    let Some(data) = word(bytes, header + 8)
        .and_then(|address| rom_pointer(bytes, address, 28, 4))
        .and_then(|handler| word(bytes, handler + 24))
        .and_then(|address| rom_pointer(bytes, address, 32, 4))
    else {
        return Ok(None);
    };
    let Some(auxiliary) = word(bytes, data + 28) else {
        return Ok(None);
    };
    if auxiliary == 0 || rom_pointer(bytes, auxiliary, 1, 1).is_none() {
        return Ok(None);
    }
    let Some(primary) = half(bytes, data + 24) else {
        return Ok(None);
    };
    let secondary = if primary <= 1 {
        u32::from(primary)
    } else {
        u32::from(bytes[data + 27]) * 1000
    };
    let quantize = |request: u32| {
        rates
            .iter()
            .copied()
            .find(|&rate| u32::from(rate) >= request)
            .unwrap_or(rates[12])
    };
    let primary = quantize(u32::from(primary));
    if primary != quantize(secondary) {
        return Ok(None);
    }
    ensure!(
        primary > rates[0],
        "GAX optimized mixer has no lower secondary rate"
    );
    // Equal automatic rates omit an auxiliary buffer that this original optimized path requires.
    Ok(Some(rates[0]))
}

fn optimized_play(bytes: &[u8], song: &GaxNativeSong, state: u32) -> bool {
    let play = song.native.play.source.effective_offset as usize;
    if literal(bytes, play + 4, 0) != Some(state)
        || !matches(
            bytes,
            play + 0x15a,
            &[
                0x6918, 0x0080, 0x1c19, 0x3108, 0x1808, 0x6800, 0x6800, 0x6182, 0x1c18, 0x304c,
                0x7800, 0x2800, 0xd00d,
            ],
        )
    {
        return false;
    }
    let call = play + 0x18a;
    let Some(first) = half(bytes, call) else {
        return false;
    };
    let Some(second) = half(bytes, call + 2) else {
        return false;
    };
    if first & 0xf800 != 0xf000 || second & 0xf800 != 0xf800 {
        return false;
    }
    let high = (i32::from(first & 0x7ff) << 21) >> 9;
    let Some(target) =
        usize::try_from(call as i64 + 4 + i64::from(high) + i64::from(second & 0x7ff) * 2).ok()
    else {
        return false;
    };
    matches(
        bytes,
        target,
        &[0xb530, 0x1c04, 0x1c0d, 0x68e2, 0x69a0, 0x2801, 0xd103],
    ) && matches(bytes, target + 0x24, &[0x2000, 0x0600, 0x2800, 0xd10a])
}

fn matches(bytes: &[u8], at: usize, pattern: &[u16]) -> bool {
    pattern.iter().enumerate().all(|(index, &expected)| {
        half(bytes, at + index * 2).is_some_and(|actual| {
            if expected & 0xf8ff == 0x4800 {
                actual & 0xff00 == expected
            } else {
                actual == expected
            }
        })
    })
}

fn literal(bytes: &[u8], at: usize, register: u16) -> Option<u32> {
    let instruction = half(bytes, at)?;
    if instruction & 0xff00 != 0x4800 | register << 8 {
        return None;
    }
    let slot = ((at + 4) & !3) + usize::from(instruction & 255) * 4;
    word(bytes, slot)
}

#[cfg(test)]
#[path = "secondary/tests.rs"]
mod tests;

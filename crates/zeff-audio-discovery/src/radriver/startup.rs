use super::{ReadError, ReadResult, RomSpan, half, pointer, span, word};

pub(super) const CRT: [u32; 28] = [
    0xe3a0_00d2,
    0xe129_f000,
    0xe59f_d064,
    0xe3a0_001f,
    0xe129_f000,
    0xe59f_d054,
    0xe59f_118c,
    0xe28f_0084,
    0xe581_0000,
    0xe3a0_1403,
    0xe3a0_2000,
    0xe3a0_3d7d,
    0xe8a1_0004,
    0xe253_3004,
    0x1aff_fffc,
    0xe59f_116c,
    0xe59f_216c,
    0xe051_3002,
    0x0a00_0004,
    0xe59f_1164,
    0xe8b1_0001,
    0xe8a2_0001,
    0xe253_3004,
    0x1aff_fffb,
    0xe59f_1154,
    0xe1a0_e00f,
    0xe12f_ff11,
    0xeaff_ffe3,
];

pub(super) fn inspect(bytes: &[u8], bank_call: usize) -> ReadResult<Vec<RomSpan>> {
    if word(bytes, 0) != Some(0xea00_002e) {
        return Err(ReadError::Invalid);
    }
    for (index, &expected) in CRT.iter().enumerate() {
        let actual = word(bytes, 0xc0 + index * 4).ok_or(ReadError::Invalid)?;
        let matches = if expected & 0xffff_0000 == 0xe59f_0000 {
            actual & 0xffff_f000 == expected & 0xffff_f000
        } else if index == 7 {
            [0xe28f_0054, 0xe28f_0084].contains(&actual)
        } else if index == 11 {
            [0xe3a0_3902, 0xe3a0_3d7d].contains(&actual)
        } else {
            actual == expected
        };
        if !matches {
            return Err(ReadError::Invalid);
        }
    }
    let mut mapped = vec![span(bytes, 0, 4)?, span(bytes, 0xc0, 0x70)?];
    let mut load = |at| -> ReadResult<u32> {
        let opcode = word(bytes, at).ok_or(ReadError::Invalid)?;
        let literal = at + 8 + (opcode & 0xfff) as usize;
        mapped.push(span(bytes, literal, 4)?);
        word(bytes, literal).ok_or(ReadError::Invalid)
    };
    let irq_stack = load(0xc8)?;
    let stack = load(0xd4)?;
    let irq_pointer = load(0xd8)?;
    let data_end = load(0xfc)?;
    let data_start = load(0x100)?;
    let data_source = load(0x10c)?;
    let main_address = load(0x120)?;
    if irq_stack != 0x0300_7fa0
        || !(0x0300_7e00..=0x0300_7f20).contains(&stack)
        || !stack.is_multiple_of(4)
        || irq_pointer != 0x0300_7ffc
        || !(0x0300_0000..data_end).contains(&data_start)
        || data_end > 0x0300_7b00
        || !(data_start | data_end).is_multiple_of(4)
        || main_address & 1 == 0
    {
        return Err(ReadError::Invalid);
    }
    let main = (main_address & !1)
        .checked_sub(0x0800_0000)
        .ok_or(ReadError::Invalid)? as usize;
    let startup_end = main.checked_add(0x200).ok_or(ReadError::Invalid)?;
    span(bytes, main, 2)?;
    // Later menu reinitialization does not prove a reachable startup handoff.
    if !(main..startup_end).contains(&bank_call)
        || half(bytes, main).is_none_or(|op| op & 0xff00 != 0xb500)
    {
        return Err(ReadError::Invalid);
    }
    mapped.push(pointer(
        bytes,
        data_source,
        (data_end - data_start) as usize,
    )?);
    mapped.push(span(bytes, main, bank_call + 4 - main)?);
    Ok(mapped)
}

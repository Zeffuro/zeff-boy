use super::*;

pub(super) fn logic(sr: u16, size: u8, value: u32) -> u16 {
    let value = value & bulk_model::mask(size);
    let sign = 1u32 << (size * 8 - 1);
    sr & !15 | if value == 0 { 4 } else { 0 } | if value & sign != 0 { 8 } else { 0 }
}

pub(super) fn evaluate(family: u16, size: u8, sr: u16, source: u32) -> (u32, u16) {
    let mask = bulk_model::mask(size);
    let source = source & mask;
    let result = match family {
        0x4200 => 0,
        0x4600 => source ^ mask,
        0x4400 | 0x4000 => {
            0u32.wrapping_sub(source)
                .wrapping_sub(u32::from(family == 0x4000 && sr & 16 != 0))
                & mask
        }
        0x4a00 => source,
        _ => panic!("family"),
    };
    if matches!(family, 0x4400 | 0x4000) {
        let sign = 1u32 << (size * 8 - 1);
        let carry = source != 0 || family == 0x4000 && sr & 16 != 0;
        let zero = result == 0 && (family != 0x4000 || sr & 4 != 0);
        let overflow = source & result & sign != 0;
        let flags = (u16::from(carry) * 17)
            | (u16::from(overflow) * 2)
            | (u16::from(zero) * 4)
            | (u16::from(result & sign != 0) * 8);
        (result, sr & !31 | flags)
    } else {
        (result, logic(sr, size, result))
    }
}

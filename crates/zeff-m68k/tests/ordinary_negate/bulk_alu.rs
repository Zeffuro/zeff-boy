use super::*;

pub(super) fn logic(sr: u16, size: u8, value: u32) -> u16 {
    let value = value & bulk_model::mask(size);
    let sign = 1u32 << (size * 8 - 1);
    sr & !15 | (u16::from(value == 0) * 4) | (u16::from(value & sign != 0) * 8)
}

fn negate(family: u16, size: u8, sr: u16, value: u32) -> (u32, u16) {
    let modulus = 1u64 << (size * 8);
    let sign = modulus >> 1;
    let source = u64::from(value) & (modulus - 1);
    let incoming = u64::from(family == 0x4000 && sr & 16 != 0);
    let extended = source + incoming;
    let result = 0u64.wrapping_sub(extended) & (modulus - 1);
    let signed = if source & sign == 0 {
        source as i64
    } else {
        source as i64 - modulus as i64
    };
    let mathematical = -signed - incoming as i64;
    let overflow = mathematical < -(sign as i64) || mathematical >= sign as i64;
    let zero = result == 0 && (family == 0x4400 || sr & 4 != 0);
    let flags = (u16::from(extended != 0) * 17)
        | (u16::from(zero) * 4)
        | (u16::from(result & sign != 0) * 8)
        | (u16::from(overflow) * 2);
    (result as u32, sr & !31 | flags)
}

pub(super) fn evaluate(family: u16, size: u8, sr: u16, source: u32) -> (u32, u16) {
    assert!(matches!(size, 1 | 2 | 4));
    if matches!(family, 0x4000 | 0x4400) {
        return negate(family, size, sr, source);
    }
    let mask = bulk_model::mask(size);
    let source = source & mask;
    let result = match family {
        0x4200 => 0,
        0x4600 => source ^ mask,
        0x4a00 => source,
        _ => panic!("family"),
    };
    (result, logic(sr, size, result))
}

pub(super) fn stages(family: u16, size: u8, original_sr: u16, source: u32) -> (u32, u16, u16) {
    let (result, full) = evaluate(family, size, original_sr, source);
    let low = if matches!(family, 0x4000 | 0x4400) {
        evaluate(family, 2, original_sr, source).1
    } else {
        logic(original_sr, 2, result)
    };
    if size == 4 && matches!(family, 0x4000 | 0x4400) {
        let incoming = u64::from(family == 0x4000 && original_sr & 16 != 0);
        let borrow = u64::from(u64::from(source & 0xffff) + incoming != 0);
        let high_source = u64::from(source >> 16);
        let high = 0u64.wrapping_sub(high_source + borrow) & 0xffff;
        assert_eq!(
            result,
            (high as u32) << 16 | negate(family, 2, original_sr, source).0
        );
        let flags = (u16::from(high_source + borrow != 0) * 17)
            | (u16::from(low & 4 != 0 && high == 0) * 4)
            | (u16::from(high & 0x8000 != 0) * 8)
            | (u16::from(high_source & high & 0x8000 != 0) * 2);
        assert_eq!(full, original_sr & !31 | flags);
    }
    (result, low, full)
}

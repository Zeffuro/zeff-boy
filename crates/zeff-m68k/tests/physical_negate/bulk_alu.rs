use super::*;
use bulk_model::status;

fn arithmetic(original: Registers, family: u16, source: u32, size: u8) -> (u32, Registers) {
    let bits = if size == 2 { 16 } else { 32 };
    let modulus = 1u64 << bits;
    let sign = modulus >> 1;
    let source = u64::from(source) & (modulus - 1);
    let sr = original.status_register();
    let incoming = u64::from(family == 0x4000 && sr & 0x10 != 0);
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
    let flags = (u16::from(extended != 0) * 0x11)
        | (u16::from(zero) * 4)
        | (u16::from(result & sign != 0) * 8)
        | (u16::from(overflow) * 2);
    (result as u32, status(original, sr & !31 | flags))
}

pub(super) fn stages(
    original: Registers,
    family: u16,
    source: u32,
    size: u8,
) -> (u32, Registers, Registers) {
    assert!(matches!(family, 0x4000 | 0x4400));
    assert!(matches!(size, 2 | 4));
    let (result, full) = arithmetic(original, family, source, size);
    let (low, partial) = arithmetic(original, family, source, 2);
    if size == 4 {
        let incoming = u64::from(family == 0x4000 && original.status_register() & 0x10 != 0);
        let borrow = u64::from(u64::from(source & 0xffff) + incoming != 0);
        let high_source = u64::from(source >> 16);
        let high = 0u64.wrapping_sub(high_source + borrow) & 0xffff;
        assert_eq!(result, (high as u32) << 16 | low);
        let flags = (u16::from(high_source + borrow != 0) * 0x11)
            | (u16::from(partial.status_register() & 4 != 0 && high == 0) * 4)
            | (u16::from(high & 0x8000 != 0) * 8)
            | (u16::from(high_source & high & 0x8000 != 0) * 2);
        assert_eq!(
            full.status_register(),
            original.status_register() & !31 | flags
        );
    }
    (result, partial, full)
}

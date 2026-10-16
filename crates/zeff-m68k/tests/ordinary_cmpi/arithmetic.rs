use super::*;
pub(super) fn evaluate(s: model::Spec, operand: u32, immediate: u32) -> (u32, u16) {
    let width = u32::from(s.size) * 8;
    let limit = 1u64 << width;
    let sign = limit / 2;
    let a = u64::from(operand) & (limit - 1);
    let b = u64::from(immediate) & (limit - 1);
    let signed = |v: u64| {
        if v >= sign {
            v as i64 - limit as i64
        } else {
            v as i64
        }
    };
    let (result, carry, overflow) = match s.family {
        0x0000 => (a | b, false, false),
        0x0200 => (a & b, false, false),
        0x0a00 => (a ^ b, false, false),
        0x0600 => {
            let value = signed(a) + signed(b);
            (
                (a + b) & (limit - 1),
                a + b >= limit,
                value < -(sign as i64) || value >= sign as i64,
            )
        }
        0x0400 | 0x0c00 => {
            let value = signed(a) - signed(b);
            (
                (a + limit - b) & (limit - 1),
                a < b,
                value < -(sign as i64) || value >= sign as i64,
            )
        }
        _ => panic!("family"),
    };
    let flags = u16::from(carry)
        | u16::from(overflow) << 1
        | u16::from(result == 0) << 2
        | u16::from(result >= sign) << 3;
    let old = s.r.status_register();
    let x = if matches!(s.family, 0x0400 | 0x0600) {
        u16::from(carry) << 4
    } else {
        old & 0x10
    };
    (result as u32, (old & 0xffe0) | x | flags)
}

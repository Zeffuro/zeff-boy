use super::{Builder, Case, flags};
use zeff_m68k::Registers;

pub(super) fn derive(
    case: &Case,
    builder: &mut Builder<'_>,
) -> Result<(Registers, [u16; 2]), String> {
    let mut expected = case.initial.registers()?;
    let original = expected;
    let op = case.initial.prefetch[0];
    let family = case.cohort.split('.').next().unwrap();
    let dn = usize::from(op & 7);
    let initial = original.d[dn];
    let sr = original.status_register();
    let old_x = sr & 16 != 0;
    let pfc = if original.supervisor() { 6 } else { 2 };
    let fetched = builder.fetch(original.pc.wrapping_add(4), pfc)?;
    let (result, mask, new_sr, idle) = if family == "EXT" || family == "SWAP" {
        let (result, mask, sign) = match case.cohort.as_str() {
            "EXT.w" => (i32::from(initial as u8 as i8) as u32 & 65535, 65535, 32768),
            "EXT.l" => (
                i32::from(initial as u16 as i16) as u32,
                u32::MAX,
                0x80000000,
            ),
            "SWAP" => (initial.rotate_left(16), u32::MAX, 0x80000000),
            _ => return Err("invalid register ordinary form".into()),
        };
        (
            result,
            mask,
            flags(sr, result, sign, (old_x, false, false, result == 0)),
            0,
        )
    } else {
        let size = (op >> 6) & 3;
        if size == 3 {
            return Err("invalid register shift size".into());
        }
        let bits = 8 << size;
        let mask = if size == 2 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        };
        let sign = 1u32 << (bits - 1);
        let count = if op & 32 != 0 {
            original.d[usize::from((op >> 9) & 7)] & 63
        } else {
            let encoded = u32::from((op >> 9) & 7);
            if encoded == 0 { 8 } else { encoded }
        };
        let mut result = initial & mask;
        let left = op & 256 != 0;
        let rotate_x = family.starts_with("ROX");
        let rotate = family.starts_with("RO") && !rotate_x;
        let arithmetic = family.starts_with("AS");
        let mut x = old_x;
        let mut carry = rotate_x && old_x;
        let mut overflow = false;
        for _ in 0..count {
            let old_sign = result & sign != 0;
            let out = if left { old_sign } else { result & 1 != 0 };
            result = if left {
                result.wrapping_shl(1) & mask
            } else {
                result >> 1
            };
            if left {
                if rotate && out || rotate_x && x {
                    result |= 1;
                }
            } else if rotate && out || rotate_x && x || arithmetic && old_sign {
                result |= sign;
            }
            if arithmetic && left && old_sign != (result & sign != 0) {
                overflow = true;
            }
            carry = out;
            if !rotate {
                x = out;
            }
        }
        let idle = if size == 2 { 4 } else { 2 } + count * 2;
        (
            result,
            mask,
            flags(sr, result, sign, (x, carry, overflow, result == 0)),
            idle,
        )
    };
    if idle != 0 {
        builder.idle(idle)?;
    }
    expected.d[dn] = (initial & !mask) | result;
    expected.set_status_register(new_sr);
    expected.pc = original.pc.wrapping_add(2);
    Ok((expected, [case.initial.prefetch[1], fetched]))
}

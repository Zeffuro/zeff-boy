use crate::ea::Size;
use crate::{CARRY, EXTEND, NEGATIVE, OVERFLOW, Registers, ZERO};

pub(crate) fn logic(regs: &mut Registers, size: Size, value: u32) {
    let value = size.truncate(value);
    let flags = if value == 0 { ZERO } else { 0 }
        | if value & size.sign_bit() != 0 {
            NEGATIVE
        } else {
            0
        };
    regs.set_status_register(
        (regs.status_register() & !(NEGATIVE | ZERO | OVERFLOW | CARRY)) | flags,
    );
}

pub(crate) fn add(regs: &mut Registers, size: Size, left: u32, right: u32) -> u32 {
    let left = size.truncate(left);
    let right = size.truncate(right);
    let sum = u64::from(left) + u64::from(right);
    let result = size.truncate(sum as u32);
    logic(regs, size, result);
    let carry = sum > u64::from(size.mask());
    let overflow = (!(left ^ right) & (left ^ result) & size.sign_bit()) != 0;
    regs.set_status_register(
        (regs.status_register() & !EXTEND)
            | if carry { CARRY | EXTEND } else { 0 }
            | if overflow { OVERFLOW } else { 0 },
    );
    result
}

pub(crate) fn subtract(
    regs: &mut Registers,
    size: Size,
    left: u32,
    right: u32,
    affect_extend: bool,
) -> u32 {
    let left = size.truncate(left);
    let right = size.truncate(right);
    let result = size.truncate(left.wrapping_sub(right));
    logic(regs, size, result);
    let carry = right > left;
    let overflow = ((left ^ right) & (left ^ result) & size.sign_bit()) != 0;
    let old_x = regs.status_register() & EXTEND;
    regs.set_status_register(
        (regs.status_register() & !EXTEND)
            | if affect_extend {
                if carry { EXTEND } else { 0 }
            } else {
                old_x
            }
            | if carry { CARRY } else { 0 }
            | if overflow { OVERFLOW } else { 0 },
    );
    result
}

pub(crate) fn condition(sr: u16, code: u16) -> bool {
    let c = sr & CARRY != 0;
    let v = sr & OVERFLOW != 0;
    let z = sr & ZERO != 0;
    let n = sr & NEGATIVE != 0;
    match code {
        0 => true,
        1 => false,
        2 => !c && !z,
        3 => c || z,
        4 => !c,
        5 => c,
        6 => !z,
        7 => z,
        8 => !v,
        9 => v,
        10 => !n,
        11 => n,
        12 => n == v,
        13 => n != v,
        14 => !z && n == v,
        15 => z || n != v,
        _ => unreachable!(),
    }
}

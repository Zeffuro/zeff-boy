pub(super) fn arm_immediate_operand(raw: u32, old_carry: bool) -> (u32, bool) {
    let imm = raw & 0xFF;
    let rotate = ((raw >> 8) & 0xF) * 2;
    if rotate == 0 {
        (imm, old_carry)
    } else {
        let value = imm.rotate_right(rotate);
        (value, value & 0x8000_0000 != 0)
    }
}

pub(super) fn shift_operand(
    value: u32,
    shift_type: u32,
    amount: u32,
    by_register: bool,
    old_carry: bool,
) -> (u32, bool) {
    match shift_type {
        0 => {
            if amount == 0 {
                (value, old_carry)
            } else if amount < 32 {
                (value << amount, value & (1 << (32 - amount)) != 0)
            } else if amount == 32 {
                (0, value & 1 != 0)
            } else {
                (0, false)
            }
        }
        1 => {
            let amount = if !by_register && amount == 0 {
                32
            } else {
                amount
            };
            if amount == 0 {
                (value, old_carry)
            } else if amount < 32 {
                (value >> amount, value & (1 << (amount - 1)) != 0)
            } else if amount == 32 {
                (0, value & 0x8000_0000 != 0)
            } else {
                (0, false)
            }
        }
        2 => {
            let amount = if !by_register && amount == 0 {
                32
            } else {
                amount
            };
            if amount == 0 {
                (value, old_carry)
            } else if amount < 32 {
                (
                    ((value as i32) >> amount) as u32,
                    value & (1 << (amount - 1)) != 0,
                )
            } else {
                let result = if value & 0x8000_0000 != 0 {
                    u32::MAX
                } else {
                    0
                };
                (result, value & 0x8000_0000 != 0)
            }
        }
        3 => {
            if !by_register && amount == 0 {
                let carry = value & 1 != 0;
                ((value >> 1) | (u32::from(old_carry) << 31), carry)
            } else {
                let rot = amount & 31;
                if amount == 0 {
                    (value, old_carry)
                } else if rot == 0 {
                    (value, value & 0x8000_0000 != 0)
                } else {
                    let result = value.rotate_right(rot);
                    (result, result & 0x8000_0000 != 0)
                }
            }
        }
        _ => (value, old_carry),
    }
}

pub(super) fn rotate_right(value: u32, amount: u32) -> u32 {
    if amount == 0 {
        value
    } else {
        value.rotate_right(amount)
    }
}

pub(super) fn add_overflow(lhs: u32, rhs: u32, result: u32) -> bool {
    ((lhs ^ result) & (rhs ^ result) & 0x8000_0000) != 0
}

pub(super) fn sub_overflow(lhs: u32, rhs: u32, result: u32) -> bool {
    ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0
}

pub(super) fn sign_extend(value: u32, bits: u8) -> i32 {
    debug_assert!((1..=31).contains(&bits));
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

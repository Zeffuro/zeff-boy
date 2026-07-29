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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MultiplyCarryKind {
    Short,
    LongSigned,
    LongUnsigned,
}

pub(super) fn arm7tdmi_multiply_carry(
    kind: MultiplyCarryKind,
    rm: u32,
    rs: u32,
    accumulator: u64,
) -> bool {
    let signed = matches!(
        kind,
        MultiplyCarryKind::Short | MultiplyCarryKind::LongSigned
    );
    let long = matches!(
        kind,
        MultiplyCarryKind::LongSigned | MultiplyCarryKind::LongUnsigned
    );
    let mut multiplier = if signed {
        sign_extend_width(u64::from(rs), 32, 34)
    } else {
        u64::from(rs) & bit_mask(33)
    };
    let multiplicand = if signed {
        sign_extend_width(u64::from(rm), 32, 34)
    } else {
        u64::from(rm) & bit_mask(33)
    };

    let mut csa_output = CsaOutput {
        output: accumulator,
        carry: if multiplier & 1 != 0 {
            !multiplicand
        } else {
            0
        },
    };
    let mut acc_shift_register = accumulator >> 34;
    let mut partial_sum = u128::from(csa_output.output & 1);
    let mut partial_carry = u128::from(csa_output.carry & 1);
    csa_output.output >>= 1;
    csa_output.carry >>= 1;
    partial_sum = ror128(partial_sum, 1);
    partial_carry = ror128(partial_carry, 1);

    let mut iterations = 0;
    loop {
        csa_output = perform_booth_cycle(
            csa_output,
            multiplicand,
            multiplier,
            &mut acc_shift_register,
        );
        partial_sum |= u128::from(csa_output.output & 0xFF);
        partial_carry |= u128::from(csa_output.carry & 0xFF);
        csa_output.output >>= 8;
        csa_output.carry >>= 8;
        partial_sum = ror128(partial_sum, 8);
        partial_carry = ror128(partial_carry, 8);
        multiplier = asr_width(multiplier, 8, 33);
        iterations += 1;
        if multiply_should_terminate(multiplier, signed) {
            break;
        }
    }

    partial_sum |= u128::from(csa_output.output);
    partial_carry |= u128::from(csa_output.carry);

    let correction_ror = match iterations {
        1 => 23,
        2 => 15,
        3 => 7,
        _ => 31,
    };
    partial_sum = ror128(partial_sum, correction_ror);
    partial_carry = ror128(partial_carry, correction_ror);

    let alu_carry_in = rs & 1 != 0;
    let partial_sum_hi = (partial_sum >> 64) as u64;
    let partial_carry_hi = (partial_carry >> 64) as u64;

    if long {
        if iterations == 4 {
            let (_, lo_carry) =
                add_with_carry(partial_sum_hi as u32, partial_carry_hi as u32, alu_carry_in);
            let _ = add_with_carry(
                (partial_sum_hi >> 32) as u32,
                (partial_carry_hi >> 32) as u32,
                lo_carry,
            );
        } else {
            let (_, lo_carry) = add_with_carry(
                (partial_sum_hi >> 32) as u32,
                (partial_carry_hi >> 32) as u32,
                alu_carry_in,
            );
            let shift_amount = 2 + 8 * iterations;
            let partial_carry_lo = sign_extend_width(partial_carry as u64, shift_amount as u8, 64);
            let partial_sum_lo = (partial_sum as u64) | (acc_shift_register << shift_amount);
            let _ = add_with_carry(partial_sum_lo as u32, partial_carry_lo as u32, lo_carry);
        }

        partial_carry_hi >> 63 != 0
    } else if iterations == 4 {
        let _ = add_with_carry(partial_sum_hi as u32, partial_carry_hi as u32, alu_carry_in);
        partial_carry_hi & (1 << 31) != 0
    } else {
        let _ = add_with_carry(
            (partial_sum_hi >> 32) as u32,
            (partial_carry_hi >> 32) as u32,
            alu_carry_in,
        );
        partial_carry_hi >> 63 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CsaOutput {
    output: u64,
    carry: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoothRecodingOutput {
    recoded_output: u64,
    carry: bool,
}

fn perform_booth_cycle(
    previous: CsaOutput,
    multiplicand: u64,
    multiplier: u64,
    acc_shift_register: &mut u64,
) -> CsaOutput {
    let mut csa_output = previous;
    let mut final_csa_output = CsaOutput {
        output: 0,
        carry: 0,
    };

    for i in 0..4 {
        csa_output.output &= bit_mask(33);
        csa_output.carry &= bit_mask(33);
        let recoded = booth_recode(multiplicand, ((multiplier >> (2 * i)) & 0x7) as u8);
        let mut result = perform_csa(
            csa_output.output,
            recoded.recoded_output & bit_mask(33),
            csa_output.carry,
        );
        result.carry = (result.carry << 1) | u64::from(recoded.carry);

        final_csa_output.output |= (result.output & 3) << (2 * i);
        final_csa_output.carry |= (result.carry & 3) << (2 * i);

        result.output >>= 2;
        result.carry >>= 2;

        let magic = u64::from(bit(*acc_shift_register, 0))
            + u64::from(!bit(csa_output.carry, 32))
            + u64::from(!bit(recoded.recoded_output, 33));
        result.output |= magic << 31;
        result.carry |= u64::from(!bit(*acc_shift_register, 1)) << 32;
        *acc_shift_register >>= 2;
        csa_output = result;
    }

    final_csa_output.output |= csa_output.output << 8;
    final_csa_output.carry |= csa_output.carry << 8;
    final_csa_output
}

fn booth_recode(input: u64, chunk: u8) -> BoothRecodingOutput {
    let (recoded_output, carry) = match chunk {
        0 | 7 => (0, false),
        1 | 2 => (input, false),
        3 => (input.wrapping_mul(2), false),
        4 => (!input.wrapping_mul(2), true),
        5 | 6 => (!input, true),
        _ => unreachable!("booth chunk is three bits"),
    };

    BoothRecodingOutput {
        recoded_output: recoded_output & bit_mask(34),
        carry,
    }
}

fn perform_csa(a: u64, b: u64, c: u64) -> CsaOutput {
    CsaOutput {
        output: a ^ b ^ c,
        carry: (a & b) | (b & c) | (c & a),
    }
}

fn multiply_should_terminate(multiplier: u64, signed: bool) -> bool {
    if signed {
        multiplier == bit_mask(33) || multiplier == 0
    } else {
        multiplier == 0
    }
}

fn add_with_carry(a: u32, b: u32, carry: bool) -> (u32, bool) {
    let result = u64::from(a) + u64::from(b) + u64::from(carry);
    (result as u32, result > u64::from(u32::MAX))
}

fn ror128(value: u128, shift: u32) -> u128 {
    debug_assert!((1..128).contains(&shift));
    value.rotate_right(shift)
}

fn bit(value: u64, index: u8) -> bool {
    value & (1_u64 << index) != 0
}

fn bit_mask(bits: u8) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1_u64 << bits) - 1
    }
}

fn sign_extend_width(value: u64, from_bits: u8, to_bits: u8) -> u64 {
    debug_assert!(from_bits > 0 && from_bits <= to_bits && to_bits <= 64);
    let mut value = value & bit_mask(from_bits);
    if bit(value, from_bits - 1) {
        value |= bit_mask(to_bits) & !bit_mask(from_bits);
    }
    value
}

fn asr_width(value: u64, shift: u8, bits: u8) -> u64 {
    let value = sign_extend_width(value, bits, 64) as i64;
    ((value >> shift) as u64) & bit_mask(bits)
}

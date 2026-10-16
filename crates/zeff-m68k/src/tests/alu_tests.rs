use crate::alu;
use crate::ea::Size;
use crate::{CARRY, EXTEND, NEGATIVE, OVERFLOW, Registers, ZERO};

fn flags(result: u32, negative: bool, overflow: bool, carry: bool) -> u16 {
    (if result == 0 { ZERO } else { 0 })
        | if negative { NEGATIVE } else { 0 }
        | if overflow { OVERFLOW } else { 0 }
        | if carry { CARRY } else { 0 }
}

#[test]
fn byte_arithmetic_exhausts_inputs_and_matches_signed_unsigned_math() {
    for left in 0..=255_u32 {
        for right in 0..=255_u32 {
            let signed_left = if left < 128 {
                left as i32
            } else {
                left as i32 - 256
            };
            let signed_right = if right < 128 {
                right as i32
            } else {
                right as i32 - 256
            };
            let mut regs = Registers::new();
            regs.set_status_register(0x251f);
            let sum = left + right;
            let signed_sum = signed_left + signed_right;
            let result = alu::add(&mut regs, Size::Byte, left, right);
            assert_eq!(result, sum % 256);
            let carry = sum >= 256;
            assert_eq!(
                regs.status_register(),
                0x2500
                    | flags(
                        result,
                        result >= 128,
                        !(-128..128).contains(&signed_sum),
                        carry
                    )
                    | if carry { EXTEND } else { 0 }
            );
            for compare in [false, true] {
                regs.set_status_register(0x251f);
                let result = alu::subtract(&mut regs, Size::Byte, left, right, !compare);
                let expected = (i64::from(left) - i64::from(right)).rem_euclid(256) as u32;
                assert_eq!(result, expected);
                let signed_difference = signed_left - signed_right;
                let carry = right > left;
                assert_eq!(
                    regs.status_register(),
                    0x2500
                        | flags(
                            result,
                            result >= 128,
                            !(-128..128).contains(&signed_difference),
                            carry
                        )
                        | if compare || carry { EXTEND } else { 0 }
                );
            }
        }
    }
}

#[test]
fn wide_arithmetic_boundaries_preserve_upper_status() {
    for size in [Size::Word, Size::Long] {
        let sign = size.sign_bit();
        let mask = size.mask();
        let mut regs = Registers::new();
        regs.set_status_register(0x2700);
        assert_eq!(alu::add(&mut regs, size, sign - 1, 1), sign);
        assert_eq!(regs.status_register(), 0x2700 | NEGATIVE | OVERFLOW);
        assert_eq!(alu::add(&mut regs, size, mask, 1), 0);
        assert_eq!(regs.status_register(), 0x2700 | EXTEND | ZERO | CARRY);
        assert_eq!(alu::subtract(&mut regs, size, sign, 1, true), sign - 1);
        assert_eq!(regs.status_register(), 0x2700 | OVERFLOW);
        assert_eq!(alu::subtract(&mut regs, size, 0, 1, true), mask);
        assert_eq!(regs.status_register(), 0x2700 | EXTEND | NEGATIVE | CARRY);
    }
}

#[test]
fn logic_preserves_extend_and_clears_overflow_carry_for_every_byte() {
    for value in 0..=255 {
        for x in [0, EXTEND] {
            let mut regs = Registers::new();
            regs.set_status_register(0x250f | x);
            alu::logic(&mut regs, Size::Byte, value | 0xabcd_0000);
            assert_eq!(
                regs.status_register(),
                0x2500 | x | flags(value, value >= 128, false, false)
            );
        }
    }
}

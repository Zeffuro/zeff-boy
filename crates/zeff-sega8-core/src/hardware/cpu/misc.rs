use super::flags::szp_flags;
use super::*;

impl Cpu {
    pub(super) fn decimal_adjust_accumulator(&mut self) {
        let original = self.regs.a;
        let subtract = self.regs.f & Z80_FLAG_SUBTRACT != 0;
        let mut correction = 0;
        let mut carry = self.regs.f & Z80_FLAG_CARRY != 0;

        if self.regs.f & Z80_FLAG_HALF_CARRY != 0 || (!subtract && original & 0x0F > 9) {
            correction |= 0x06;
        }
        if carry || (!subtract && original > 0x99) {
            correction |= 0x60;
            carry = true;
        }

        let result = if subtract {
            original.wrapping_sub(correction)
        } else {
            original.wrapping_add(correction)
        };
        self.regs.a = result;

        self.regs.f = (self.regs.f & Z80_FLAG_SUBTRACT)
            | szp_flags(result)
            | if (original ^ result) & 0x10 != 0 {
                Z80_FLAG_HALF_CARRY
            } else {
                0
            }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    pub(super) fn complement_accumulator(&mut self) {
        self.regs.a = !self.regs.a;
        self.regs.f = (self.regs.f
            & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW | Z80_FLAG_CARRY))
            | (self.regs.a & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | Z80_FLAG_HALF_CARRY
            | Z80_FLAG_SUBTRACT;
    }

    pub(super) fn set_carry_flag(&mut self) {
        self.regs.f = (self.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW))
            | (self.regs.a & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | Z80_FLAG_CARRY;
    }

    pub(super) fn complement_carry_flag(&mut self) {
        let old_carry = self.regs.f & Z80_FLAG_CARRY != 0;
        self.regs.f = (self.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW))
            | (self.regs.a & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | if old_carry {
                Z80_FLAG_HALF_CARRY
            } else {
                Z80_FLAG_CARRY
            };
    }
}

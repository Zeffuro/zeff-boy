use super::flags::{sz53_flags, szp_flags};
use super::*;

impl Cpu {
    pub(super) fn execute_alu_group(&mut self, group: u8, value: u8) {
        match group {
            0 => self.add_a(value, false),
            1 => self.add_a(value, true),
            2 => self.sub_a(value, false),
            3 => self.sub_a(value, true),
            4 => self.and_a(value),
            5 => self.xor_a(value),
            6 => self.or_a(value),
            7 => self.cp_a(value),
            _ => unreachable!("ALU group index is always three bits"),
        }
    }

    fn add_a(&mut self, value: u8, with_carry: bool) {
        let a = self.regs.a;
        let carry_in = u8::from(with_carry && self.regs.f & Z80_FLAG_CARRY != 0);
        let result = a.wrapping_add(value).wrapping_add(carry_in);
        let half_carry = (a & 0x0F) + (value & 0x0F) + carry_in > 0x0F;
        let carry = u16::from(a) + u16::from(value) + u16::from(carry_in) > 0xFF;
        let overflow = (!(a ^ value) & (a ^ result) & 0x80) != 0;

        self.regs.a = result;
        self.regs.f = sz53_flags(result)
            | if half_carry { Z80_FLAG_HALF_CARRY } else { 0 }
            | if overflow {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    fn sub_a(&mut self, value: u8, with_carry: bool) {
        let a = self.regs.a;
        let carry_in = u8::from(with_carry && self.regs.f & Z80_FLAG_CARRY != 0);
        let result = a.wrapping_sub(value).wrapping_sub(carry_in);
        let half_borrow = (a & 0x0F) < ((value & 0x0F) + carry_in);
        let carry = u16::from(a) < u16::from(value) + u16::from(carry_in);
        let overflow = ((a ^ value) & (a ^ result) & 0x80) != 0;

        self.regs.a = result;
        self.regs.f = sz53_flags(result)
            | Z80_FLAG_SUBTRACT
            | if half_borrow { Z80_FLAG_HALF_CARRY } else { 0 }
            | if overflow {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    fn and_a(&mut self, value: u8) {
        self.regs.a &= value;
        self.regs.f = szp_flags(self.regs.a) | Z80_FLAG_HALF_CARRY;
    }

    fn xor_a(&mut self, value: u8) {
        self.regs.a ^= value;
        self.regs.f = szp_flags(self.regs.a);
    }

    fn or_a(&mut self, value: u8) {
        self.regs.a |= value;
        self.regs.f = szp_flags(self.regs.a);
    }

    fn cp_a(&mut self, value: u8) {
        let a = self.regs.a;
        let result = a.wrapping_sub(value);
        let half_borrow = (a & 0x0F) < (value & 0x0F);
        let carry = a < value;
        let overflow = ((a ^ value) & (a ^ result) & 0x80) != 0;

        self.regs.f = (result & Z80_FLAG_SIGN)
            | if result == 0 { Z80_FLAG_ZERO } else { 0 }
            | (value & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | Z80_FLAG_SUBTRACT
            | if half_borrow { Z80_FLAG_HALF_CARRY } else { 0 }
            | if overflow {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    pub(super) fn add_hl(&mut self, value: u16) {
        let hl = self.regs.hl();
        let result = hl.wrapping_add(value);
        let half_carry = (hl & 0x0FFF) + (value & 0x0FFF) > 0x0FFF;
        let carry = u32::from(hl) + u32::from(value) > 0xFFFF;
        let preserved = self.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW);
        let undocumented = ((result >> 8) as u8) & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3);

        self.regs.set_hl(result);
        self.regs.f = preserved
            | undocumented
            | if half_carry { Z80_FLAG_HALF_CARRY } else { 0 }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    pub(super) fn set_inc_flags(&mut self, value: u8, result: u8) {
        let carry = self.regs.f & Z80_FLAG_CARRY;
        self.regs.f = carry
            | sz53_flags(result)
            | if value & 0x0F == 0x0F {
                Z80_FLAG_HALF_CARRY
            } else {
                0
            }
            | if value == 0x7F {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            };
    }

    pub(super) fn set_dec_flags(&mut self, value: u8, result: u8) {
        let carry = self.regs.f & Z80_FLAG_CARRY;
        self.regs.f = carry
            | sz53_flags(result)
            | Z80_FLAG_SUBTRACT
            | if value & 0x0F == 0 {
                Z80_FLAG_HALF_CARRY
            } else {
                0
            }
            | if value == 0x80 {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            };
    }
}

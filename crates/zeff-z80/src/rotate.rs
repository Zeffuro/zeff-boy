use super::*;

impl Cpu {
    pub(super) fn rotate_a_left_circular(&mut self) {
        let carry = self.regs.a & 0x80 != 0;
        self.regs.a = self.regs.a.rotate_left(1);
        self.set_accumulator_rotate_flags(carry);
    }

    pub(super) fn rotate_a_right_circular(&mut self) {
        let carry = self.regs.a & 0x01 != 0;
        self.regs.a = self.regs.a.rotate_right(1);
        self.set_accumulator_rotate_flags(carry);
    }

    pub(super) fn rotate_a_left_through_carry(&mut self) {
        let carry_in = u8::from(self.regs.f & Z80_FLAG_CARRY != 0);
        let carry = self.regs.a & 0x80 != 0;
        self.regs.a = (self.regs.a << 1) | carry_in;
        self.set_accumulator_rotate_flags(carry);
    }

    pub(super) fn rotate_a_right_through_carry(&mut self) {
        let carry_in = if self.regs.f & Z80_FLAG_CARRY != 0 {
            0x80
        } else {
            0
        };
        let carry = self.regs.a & 0x01 != 0;
        self.regs.a = (self.regs.a >> 1) | carry_in;
        self.set_accumulator_rotate_flags(carry);
    }

    fn set_accumulator_rotate_flags(&mut self, carry: bool) {
        self.regs.f = (self.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW))
            | (self.regs.a & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }
}

use super::*;

impl Cpu {
    pub(super) fn alu8(&mut self, op: AluOp, lhs: u8, rhs: u8) -> u8 {
        match op {
            AluOp::Add => {
                let result = lhs.wrapping_add(rhs);
                self.set_add_flags8(lhs, rhs, result);
                result
            }
            AluOp::Adc => {
                let carry = u8::from(self.carry_set());
                let result = lhs.wrapping_add(rhs).wrapping_add(carry);
                self.set_adc_flags8(lhs, rhs, carry, result);
                result
            }
            AluOp::Or => {
                let result = lhs | rhs;
                self.set_logic_flags8(result);
                result
            }
            AluOp::And => {
                let result = lhs & rhs;
                self.set_logic_flags8(result);
                result
            }
            AluOp::Sub | AluOp::Cmp => {
                let result = lhs.wrapping_sub(rhs);
                self.set_sub_flags8(lhs, rhs, result);
                result
            }
            AluOp::Sbb => {
                let carry = u8::from(self.carry_set());
                let result = lhs.wrapping_sub(rhs).wrapping_sub(carry);
                self.set_sbb_flags8(lhs, rhs, carry, result);
                result
            }
            AluOp::Xor => {
                let result = lhs ^ rhs;
                self.set_logic_flags8(result);
                result
            }
        }
    }

    pub(super) fn alu16(&mut self, op: AluOp, lhs: u16, rhs: u16) -> u16 {
        match op {
            AluOp::Add => {
                let result = lhs.wrapping_add(rhs);
                self.set_add_flags16(lhs, rhs, result);
                result
            }
            AluOp::Adc => {
                let carry = u16::from(self.carry_set());
                let result = lhs.wrapping_add(rhs).wrapping_add(carry);
                self.set_adc_flags16(lhs, rhs, carry, result);
                result
            }
            AluOp::Or => {
                let result = lhs | rhs;
                self.set_logic_flags16(result);
                result
            }
            AluOp::And => {
                let result = lhs & rhs;
                self.set_logic_flags16(result);
                result
            }
            AluOp::Sub | AluOp::Cmp => {
                let result = lhs.wrapping_sub(rhs);
                self.set_sub_flags16(lhs, rhs, result);
                result
            }
            AluOp::Sbb => {
                let carry = u16::from(self.carry_set());
                let result = lhs.wrapping_sub(rhs).wrapping_sub(carry);
                self.set_sbb_flags16(lhs, rhs, carry, result);
                result
            }
            AluOp::Xor => {
                let result = lhs ^ rhs;
                self.set_logic_flags16(result);
                result
            }
        }
    }

    pub(super) fn set_logic_flags8(&mut self, result: u8) {
        self.flags &=
            !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF | FLAG_RESERVED_LOW);
        if result == 0 {
            self.flags |= FLAG_ZF;
        }
        if result & 0x80 != 0 {
            self.flags |= FLAG_SF;
        }
        if result.count_ones().is_multiple_of(2) {
            self.flags |= FLAG_PF;
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn set_logic_flags16(&mut self, result: u16) {
        self.flags &=
            !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF | FLAG_RESERVED_LOW);
        if result == 0 {
            self.flags |= FLAG_ZF;
        }
        if result & 0x8000 != 0 {
            self.flags |= FLAG_SF;
        }
        if (result as u8).count_ones().is_multiple_of(2) {
            self.flags |= FLAG_PF;
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn carry_set(&self) -> bool {
        self.flags & FLAG_CF != 0
    }

    pub(super) fn set_carry(&mut self, set: bool) {
        if set {
            self.flags |= FLAG_CF;
        } else {
            self.flags &= !FLAG_CF;
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn set_overflow(&mut self, set: bool) {
        if set {
            self.flags |= FLAG_OF;
        } else {
            self.flags &= !FLAG_OF;
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn set_mul_flags(&mut self, overflow: bool, zero: bool) {
        self.last_mul_overflow = overflow;
        self.flags &=
            !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF | FLAG_RESERVED_LOW);
        if zero {
            self.flags |= FLAG_ZF;
        }
        if overflow {
            self.flags |= FLAG_CF | FLAG_OF;
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn set_div8_unsigned_flags(&mut self, quotient: u8, remainder: u8) {
        self.set_div_flags_from_last_mul(remainder == 0 && quotient & 0x01 != 0);
    }

    pub(super) fn set_div16_flags(&mut self, quotient: u16, remainder: u16) {
        self.set_div_flags_clear_cv(remainder == 0 && quotient & 0x0001 != 0);
    }

    pub(super) fn set_idiv8_flags(&mut self, quotient: u8) {
        self.set_logic_flags8(quotient);
        self.flags &= !(FLAG_CF | FLAG_AF | FLAG_OF);
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn set_divide_error_flags_from_last_mul(&mut self, zero: bool) {
        self.set_div_flags_from_last_mul(zero);
    }

    pub(super) fn set_divide_error_flags_clear_cv(&mut self, zero: bool) {
        self.set_div_flags_clear_cv(zero);
    }

    fn set_div_flags_from_last_mul(&mut self, zero: bool) {
        self.flags &=
            !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF | FLAG_RESERVED_LOW);
        if zero {
            self.flags |= FLAG_ZF;
        }
        if self.last_mul_overflow {
            self.flags |= FLAG_CF | FLAG_OF;
        }
        self.flags |= FLAG_FIXED;
    }

    fn set_div_flags_clear_cv(&mut self, zero: bool) {
        self.flags &=
            !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF | FLAG_RESERVED_LOW);
        if zero {
            self.flags |= FLAG_ZF;
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn set_add_flags8(&mut self, lhs: u8, rhs: u8, result: u8) {
        self.set_logic_flags8(result);
        if u16::from(lhs) + u16::from(rhs) > 0xFF {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x0F) + (rhs & 0x0F) > 0x0F {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ result) & (rhs ^ result) & 0x80) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_add_flags16(&mut self, lhs: u16, rhs: u16, result: u16) {
        self.set_logic_flags16(result);
        if u32::from(lhs) + u32::from(rhs) > 0xFFFF {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x000F) + (rhs & 0x000F) > 0x000F {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ result) & (rhs ^ result) & 0x8000) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_adc_flags8(&mut self, lhs: u8, rhs: u8, carry: u8, result: u8) {
        self.set_logic_flags8(result);
        if u16::from(lhs) + u16::from(rhs) + u16::from(carry) > 0xFF {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x0F) + (rhs & 0x0F) + carry > 0x0F {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ result) & (rhs ^ result) & 0x80) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_adc_flags16(&mut self, lhs: u16, rhs: u16, carry: u16, result: u16) {
        self.set_logic_flags16(result);
        if u32::from(lhs) + u32::from(rhs) + u32::from(carry) > 0xFFFF {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x000F) + (rhs & 0x000F) + carry > 0x000F {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ result) & (rhs ^ result) & 0x8000) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_sub_flags8(&mut self, lhs: u8, rhs: u8, result: u8) {
        self.set_logic_flags8(result);
        if lhs < rhs {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x0F) < (rhs & 0x0F) {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ rhs) & (lhs ^ result) & 0x80) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_sub_flags16(&mut self, lhs: u16, rhs: u16, result: u16) {
        self.set_logic_flags16(result);
        if lhs < rhs {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x000F) < (rhs & 0x000F) {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ rhs) & (lhs ^ result) & 0x8000) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_sbb_flags8(&mut self, lhs: u8, rhs: u8, carry: u8, result: u8) {
        self.set_logic_flags8(result);
        if u16::from(lhs) < u16::from(rhs) + u16::from(carry) {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x0F) < (rhs & 0x0F) + carry {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ rhs) & (lhs ^ result) & 0x80) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_sbb_flags16(&mut self, lhs: u16, rhs: u16, carry: u16, result: u16) {
        self.set_logic_flags16(result);
        if u32::from(lhs) < u32::from(rhs) + u32::from(carry) {
            self.flags |= FLAG_CF;
        }
        if (lhs & 0x000F) < (rhs & 0x000F) + carry {
            self.flags |= FLAG_AF;
        }
        if ((lhs ^ rhs) & (lhs ^ result) & 0x8000) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    pub(super) fn set_inc_dec_flags16(&mut self, result: u16, decrement: bool) {
        let old_cf = self.flags & FLAG_CF;
        if decrement {
            self.set_sub_flags16(result.wrapping_add(1), 1, result);
        } else {
            self.set_add_flags16(result.wrapping_sub(1), 1, result);
        }
        self.flags = (self.flags & !FLAG_CF) | old_cf | FLAG_FIXED;
    }

    pub(super) fn set_inc_dec_flags8(&mut self, result: u8, decrement: bool) {
        let old_cf = self.flags & FLAG_CF;
        if decrement {
            self.set_sub_flags8(result.wrapping_add(1), 1, result);
        } else {
            self.set_add_flags8(result.wrapping_sub(1), 1, result);
        }
        self.flags = (self.flags & !FLAG_CF) | old_cf | FLAG_FIXED;
    }

    pub(super) fn condition(&self, condition: u8) -> bool {
        let cf = self.flags & FLAG_CF != 0;
        let pf = self.flags & FLAG_PF != 0;
        let zf = self.flags & FLAG_ZF != 0;
        let sf = self.flags & FLAG_SF != 0;
        let of = self.flags & FLAG_OF != 0;
        match condition {
            0x0 => of,
            0x1 => !of,
            0x2 => cf,
            0x3 => !cf,
            0x4 => zf,
            0x5 => !zf,
            0x6 => cf || zf,
            0x7 => !cf && !zf,
            0x8 => sf,
            0x9 => !sf,
            0xA => pf,
            0xB => !pf,
            0xC => sf != of,
            0xD => sf == of,
            0xE => zf || (sf != of),
            _ => !zf && (sf == of),
        }
    }

    pub(super) fn add_cycles(&mut self, bus: &mut Bus, cycles: u32) {
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        bus.step_cycles(cycles);
    }

    pub(super) fn unsupported_opcode(&mut self, opcode: u8) {
        self.last_trap = Some(CpuTrap::UnsupportedOpcode {
            cs: self.segments[SegmentRegister::Cs.index()],
            ip: self.ip.wrapping_sub(1),
            opcode,
        });
        self.state = CpuState::Suspended;
    }

    pub(super) fn unsupported_form(&mut self, opcode: u8, modrm: u8) {
        self.last_trap = Some(CpuTrap::UnsupportedInstructionForm {
            cs: self.segments[SegmentRegister::Cs.index()],
            ip: self.ip.wrapping_sub(2),
            opcode,
            modrm,
        });
        self.state = CpuState::Suspended;
    }

    pub(super) fn divide_error(&mut self, bus: &mut Bus) {
        self.enter_interrupt(0, 10, bus);
    }
}

pub(super) fn alu_group_op(reg: u8) -> Option<AluOp> {
    match reg {
        0 => Some(AluOp::Add),
        1 => Some(AluOp::Or),
        2 => Some(AluOp::Adc),
        3 => Some(AluOp::Sbb),
        4 => Some(AluOp::And),
        5 => Some(AluOp::Sub),
        6 => Some(AluOp::Xor),
        7 => Some(AluOp::Cmp),
        _ => None,
    }
}

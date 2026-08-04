use super::*;

impl Cpu {
    pub(super) fn shift_rotate_rm8(
        &mut self,
        modrm: ModRm,
        count: u8,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        self.shift_rotate_operand8(modrm, operand, count, bus);
    }

    pub(super) fn shift_rotate_operand8(
        &mut self,
        modrm: ModRm,
        operand: Operand,
        count: u8,
        bus: &mut Bus,
    ) {
        let raw_count = count;
        let count = raw_count & 0x1F;
        if count == 0 {
            let value = self.read_operand8(operand, bus);
            if modrm.reg <= 3 {
                self.update_masked_zero_rotate_flags8(modrm.reg, value);
            } else {
                self.update_masked_zero_shift_flags8(modrm.reg, value);
            }
            self.add_cycles(bus, 2);
            return;
        }
        let value = self.read_operand8(operand, bus);
        let Some(result) = self.shift_rotate8(modrm.reg, value, count) else {
            self.unsupported_form(0xD0, modrm.byte);
            return;
        };
        self.write_operand8(operand, result, bus);
        self.add_cycles(bus, 4 + u32::from(count));
    }

    pub(super) fn shift_rotate_rm16(
        &mut self,
        modrm: ModRm,
        count: u8,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        self.shift_rotate_operand16(modrm, operand, count, bus);
    }

    pub(super) fn shift_rotate_operand16(
        &mut self,
        modrm: ModRm,
        operand: Operand,
        count: u8,
        bus: &mut Bus,
    ) {
        let raw_count = count;
        let count = raw_count & 0x1F;
        if count == 0 {
            let value = self.read_operand16(operand, bus);
            if modrm.reg <= 3 {
                self.update_masked_zero_rotate_flags16(modrm.reg, value);
            } else {
                self.update_masked_zero_shift_flags16(modrm.reg, value);
            }
            self.add_cycles(bus, 2);
            return;
        }
        let value = self.read_operand16(operand, bus);
        let Some(result) = self.shift_rotate16(modrm.reg, value, count) else {
            self.unsupported_form(0xD1, modrm.byte);
            return;
        };
        self.write_operand16(operand, result, bus);
        self.add_cycles(bus, 4 + u32::from(count));
    }

    pub(super) fn shift_rotate8(&mut self, op: u8, value: u8, count: u8) -> Option<u8> {
        let mut result = value;
        match op {
            0 => {
                for _ in 0..count {
                    let carry = result & 0x80 != 0;
                    result = result.rotate_left(1);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 7) & 1) != u8::from(self.carry_set()));
            }
            1 => {
                for _ in 0..count {
                    let carry = result & 0x01 != 0;
                    result = result.rotate_right(1);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 7) ^ (result >> 6)) & 1 != 0);
            }
            2 => {
                for _ in 0..count {
                    let old_cf = self.carry_set();
                    let carry = result & 0x80 != 0;
                    result = (result << 1) | u8::from(old_cf);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 7) & 1) != u8::from(self.carry_set()));
            }
            3 => {
                for _ in 0..count {
                    let old_cf = self.carry_set();
                    let carry = result & 0x01 != 0;
                    result = (result >> 1) | (u8::from(old_cf) << 7);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 7) ^ (result >> 6)) & 1 != 0);
            }
            4 | 6 => {
                for _ in 0..count {
                    let carry = result & 0x80 != 0;
                    result <<= 1;
                    self.set_carry(carry);
                }
                let carry = self.carry_set();
                self.set_logic_flags8(result);
                self.set_carry(carry);
                self.set_overflow(((result >> 7) & 1) != u8::from(self.carry_set()));
            }
            5 => {
                for _ in 0..count {
                    let carry = result & 0x01 != 0;
                    result >>= 1;
                    self.set_carry(carry);
                }
                let carry = self.carry_set();
                self.set_logic_flags8(result);
                self.set_carry(carry);
                self.set_overflow(((result >> 7) ^ (result >> 6)) & 1 != 0);
            }
            7 => {
                for _ in 0..count {
                    let carry = result & 0x01 != 0;
                    result = ((result as i8) >> 1) as u8;
                    self.set_carry(carry);
                }
                let carry = self.carry_set();
                self.set_logic_flags8(result);
                self.set_carry(carry);
                self.set_overflow(((result >> 7) ^ (result >> 6)) & 1 != 0);
            }
            _ => return None,
        }
        self.flags |= FLAG_FIXED;
        Some(result)
    }

    fn update_masked_zero_rotate_flags8(&mut self, op: u8, value: u8) {
        match op {
            0 | 2 => self.set_overflow(((value >> 7) & 1) != u8::from(self.carry_set())),
            1 | 3 => self.set_overflow(((value >> 7) ^ (value >> 6)) & 1 != 0),
            _ => {}
        }
        self.flags |= FLAG_FIXED;
    }

    fn update_masked_zero_shift_flags8(&mut self, op: u8, value: u8) {
        let carry = self.carry_set();
        match op {
            4 | 6 => {
                self.set_logic_flags8(value);
                self.set_carry(carry);
                self.set_overflow(((value >> 7) & 1) != u8::from(carry));
            }
            5 => {
                self.set_logic_flags8(value);
                self.set_carry(carry);
                self.set_overflow(((value >> 7) ^ (value >> 6)) & 1 != 0);
            }
            7 => {
                self.set_logic_flags8(value);
                self.set_carry(carry);
                self.set_overflow(((value >> 7) ^ (value >> 6)) & 1 != 0);
            }
            _ => {}
        }
        self.flags |= FLAG_FIXED;
    }

    pub(super) fn shift_rotate16(&mut self, op: u8, value: u16, count: u8) -> Option<u16> {
        let mut result = value;
        match op {
            0 => {
                for _ in 0..count {
                    let carry = result & 0x8000 != 0;
                    result = result.rotate_left(1);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 15) & 1) != u16::from(self.carry_set()));
            }
            1 => {
                for _ in 0..count {
                    let carry = result & 0x0001 != 0;
                    result = result.rotate_right(1);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 15) ^ (result >> 14)) & 1 != 0);
            }
            2 => {
                for _ in 0..count {
                    let old_cf = self.carry_set();
                    let carry = result & 0x8000 != 0;
                    result = (result << 1) | u16::from(old_cf);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 15) & 1) != u16::from(self.carry_set()));
            }
            3 => {
                for _ in 0..count {
                    let old_cf = self.carry_set();
                    let carry = result & 0x0001 != 0;
                    result = (result >> 1) | (u16::from(old_cf) << 15);
                    self.set_carry(carry);
                }
                self.set_overflow(((result >> 15) ^ (result >> 14)) & 1 != 0);
            }
            4 | 6 => {
                for _ in 0..count {
                    let carry = result & 0x8000 != 0;
                    result <<= 1;
                    self.set_carry(carry);
                }
                let carry = self.carry_set();
                self.set_logic_flags16(result);
                self.set_carry(carry);
                self.set_overflow(((result >> 15) & 1) != u16::from(self.carry_set()));
            }
            5 => {
                for _ in 0..count {
                    let carry = result & 0x0001 != 0;
                    result >>= 1;
                    self.set_carry(carry);
                }
                let carry = self.carry_set();
                self.set_logic_flags16(result);
                self.set_carry(carry);
                self.set_overflow(((result >> 15) ^ (result >> 14)) & 1 != 0);
            }
            7 => {
                for _ in 0..count {
                    let carry = result & 0x0001 != 0;
                    result = ((result as i16) >> 1) as u16;
                    self.set_carry(carry);
                }
                let carry = self.carry_set();
                self.set_logic_flags16(result);
                self.set_carry(carry);
                self.set_overflow(((result >> 15) ^ (result >> 14)) & 1 != 0);
            }
            _ => return None,
        }
        self.flags |= FLAG_FIXED;
        Some(result)
    }

    fn update_masked_zero_rotate_flags16(&mut self, op: u8, value: u16) {
        match op {
            0 | 2 => self.set_overflow(((value >> 15) & 1) != u16::from(self.carry_set())),
            1 | 3 => self.set_overflow(((value >> 15) ^ (value >> 14)) & 1 != 0),
            _ => {}
        }
        self.flags |= FLAG_FIXED;
    }

    fn update_masked_zero_shift_flags16(&mut self, op: u8, value: u16) {
        let carry = self.carry_set();
        match op {
            4 | 6 => {
                self.set_logic_flags16(value);
                self.set_carry(carry);
                self.set_overflow(((value >> 15) & 1) != u16::from(carry));
            }
            5 => {
                self.set_logic_flags16(value);
                self.set_carry(carry);
                self.set_overflow(((value >> 15) ^ (value >> 14)) & 1 != 0);
            }
            7 => {
                self.set_logic_flags16(value);
                self.set_carry(carry);
                self.set_overflow(((value >> 15) ^ (value >> 14)) & 1 != 0);
            }
            _ => {}
        }
        self.flags |= FLAG_FIXED;
    }
}

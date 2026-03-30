use super::Cpu;
use super::registers::StatusFlags;

impl Cpu {
    pub fn adc(&mut self, val: u8) {
        let a = self.regs.a as u16;
        let v = val as u16;
        let c = if self.regs.get_flag(StatusFlags::CARRY) { 1u16 } else { 0 };
        let sum = a + v + c;
        let result = sum as u8;
        self.regs.set_flag(StatusFlags::CARRY, sum > 0xFF);
        self.regs.set_flag(StatusFlags::OVERFLOW, (!(a ^ v) & (a ^ sum)) & 0x80 != 0);
        self.regs.a = result;
        self.regs.set_zn(result);
    }

    pub fn sbc(&mut self, val: u8) {
        self.adc(!val);
    }

    pub fn compare(&mut self, reg: u8, val: u8) {
        let diff = reg.wrapping_sub(val);
        self.regs.set_flag(StatusFlags::CARRY, reg >= val);
        self.regs.set_zn(diff);
    }

    pub fn bit_test(&mut self, val: u8) {
        self.regs.set_flag(StatusFlags::ZERO, self.regs.a & val == 0);
        self.regs.set_flag(StatusFlags::OVERFLOW, val & 0x40 != 0);
        self.regs.set_flag(StatusFlags::NEGATIVE, val & 0x80 != 0);
    }

    pub fn asl_acc(&mut self) {
        let old = self.regs.a;
        self.regs.a = old << 1;
        self.regs.set_flag(StatusFlags::CARRY, old & 0x80 != 0);
        self.regs.set_zn(self.regs.a);
    }

    pub fn asl_val(&mut self, val: u8) -> u8 {
        let result = val << 1;
        self.regs.set_flag(StatusFlags::CARRY, val & 0x80 != 0);
        self.regs.set_zn(result);
        result
    }

    pub fn lsr_acc(&mut self) {
        let old = self.regs.a;
        self.regs.a = old >> 1;
        self.regs.set_flag(StatusFlags::CARRY, old & 0x01 != 0);
        self.regs.set_zn(self.regs.a);
    }

    pub fn lsr_val(&mut self, val: u8) -> u8 {
        let result = val >> 1;
        self.regs.set_flag(StatusFlags::CARRY, val & 0x01 != 0);
        self.regs.set_zn(result);
        result
    }

    pub fn rol_acc(&mut self) {
        let old = self.regs.a;
        let carry_in = if self.regs.get_flag(StatusFlags::CARRY) { 1 } else { 0 };
        self.regs.a = (old << 1) | carry_in;
        self.regs.set_flag(StatusFlags::CARRY, old & 0x80 != 0);
        self.regs.set_zn(self.regs.a);
    }

    pub fn rol_val(&mut self, val: u8) -> u8 {
        let carry_in = if self.regs.get_flag(StatusFlags::CARRY) { 1 } else { 0 };
        let result = (val << 1) | carry_in;
        self.regs.set_flag(StatusFlags::CARRY, val & 0x80 != 0);
        self.regs.set_zn(result);
        result
    }

    pub fn ror_acc(&mut self) {
        let old = self.regs.a;
        let carry_in = if self.regs.get_flag(StatusFlags::CARRY) { 0x80 } else { 0 };
        self.regs.a = (old >> 1) | carry_in;
        self.regs.set_flag(StatusFlags::CARRY, old & 0x01 != 0);
        self.regs.set_zn(self.regs.a);
    }

    pub fn ror_val(&mut self, val: u8) -> u8 {
        let carry_in = if self.regs.get_flag(StatusFlags::CARRY) { 0x80 } else { 0 };
        let result = (val >> 1) | carry_in;
        self.regs.set_flag(StatusFlags::CARRY, val & 0x01 != 0);
        self.regs.set_zn(result);
        result
    }

    pub fn inc_val(&mut self, val: u8) -> u8 {
        let result = val.wrapping_add(1);
        self.regs.set_zn(result);
        result
    }

    pub fn dec_val(&mut self, val: u8) -> u8 {
        let result = val.wrapping_sub(1);
        self.regs.set_zn(result);
        result
    }
}


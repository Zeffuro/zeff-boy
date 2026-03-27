use super::Cpu;

impl Cpu {
    pub fn rlc(&mut self, value: u8) -> u8 {
        let result = value.rotate_left(1);
        self.set_flags(result == 0, false, false, result & 1 != 0);
        result
    }

    pub fn rrc(&mut self, value: u8) -> u8 {
        let result = value.rotate_right(1);
        self.set_flags(result == 0, false, false, value & 0x01 != 0);
        result
    }

    pub fn rl(&mut self, value: u8) -> u8 {
        let carry = if self.get_c() { 1 } else { 0 };
        let result = (value << 1) | carry;
        self.set_flags(result == 0, false, false, (value & 0x80) != 0);
        result
    }

    pub fn rr(&mut self, value: u8) -> u8 {
        let carry = if self.get_c() { 0x80 } else { 0 };
        let result = (value >> 1) | carry;
        self.set_flags(result == 0, false, false, (value & 1) != 0);
        result
    }

    pub fn sla(&mut self, value: u8) -> u8 {
        let result = value << 1;
        self.set_flags(result == 0, false, false, (value & 0x80) != 0);
        result
    }

    pub fn srl(&mut self, value: u8) -> u8 {
        let result = value >> 1;
        self.set_flags(result == 0, false, false, (value & 1) != 0);
        result
    }

    pub fn sra(&mut self, value: u8) -> u8 {
        let result = (value >> 1) | (value & 0x80);
        self.set_flags(result == 0, false, false, (value & 1) != 0);
        result
    }

    pub fn swap(&mut self, value: u8) -> u8 {
        let result = value.rotate_left(4);
        self.set_flags(result == 0, false, false, false);
        result
    }

    pub fn bit(&mut self, bit: u8, value: u8) {
        let is_zero = (value & (1 << bit)) == 0;
        self.set_flags(is_zero, false, true, self.get_c());
    }

    pub fn set(&mut self, bit: u8, value: u8) -> u8 {
        value | (1 << bit)
    }

    pub fn res(&mut self, bit: u8, value: u8) -> u8 {
        value & !(1 << bit)
    }
}

use super::CPU;

impl CPU {
    pub(crate) fn inc(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        self.set_flags(result == 0, false, (value & 0x0F) + 1 > 0x0F, self.get_c());
        result
    }

    pub(crate) fn dec(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        self.set_flags(result == 0, true, (value & 0x0F) == 0, self.get_c());
        result
    }

    pub(crate) fn adc(&mut self, value: u8) {
        let a = self.a as u16;
        let val = value as u16;
        let carry = self.get_c() as u16;
        let result = a + val + carry;

        self.a = result as u8;
        self.set_flags(
            self.a == 0,
            false,
            (a ^ val ^ result) & 0x10 != 0,
            result > 0xFF,
        );
    }

    pub(crate) fn sbc(&mut self, value: u8) {
        let a = self.a;
        let carry = self.get_c() as u8;
        let result = a.wrapping_sub(value).wrapping_sub(carry);

        self.set_flags(
            result == 0,
            true,
            (a & 0x0F) < (value & 0x0F) + carry,
            (a as u16) < (value as u16) + (carry as u16),
        );
        self.a = result;
    }

    pub(crate) fn add(&mut self, value: u8) {
        let a = self.a;
        let result = a.wrapping_add(value);

        self.a = result;
        self.set_flags(
            self.a == 0,
            false,
            (a & 0x0F) + (value & 0x0F) > 0x0F,
            (a as u16) + (value as u16) > 0xFF,
        );
    }

    pub(crate) fn sub(&mut self, value: u8) {
        let a = self.a;
        let result = a.wrapping_sub(value);

        self.a = result;
        self.set_flags(result == 0, true, (a & 0x0F) < (value & 0x0F), a < value);
    }

    pub(crate) fn compare(&mut self, value: u8) {
        let temp_a = self.a;
        self.sub(value);
        self.a = temp_a;
    }

    pub(crate) fn logical_or(&mut self, value: u8) {
        self.a |= value;
        self.set_flags(self.a == 0, false, false, false);
    }

    pub(crate) fn logical_and(&mut self, value: u8) {
        self.a &= value;
        self.set_flags(self.a == 0, false, true, false);
    }

    pub(crate) fn logical_xor(&mut self, value: u8) {
        self.a ^= value;
        self.set_flags(self.a == 0, false, false, false);
    }
}

use super::Cpu;

#[derive(Debug, Clone, Copy)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
        }
    }
}

impl Cpu {
    pub fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.set_z(z);
        self.set_n(n);
        self.set_h(h);
        self.set_c(c);
    }

    pub fn get_af(&self) -> u16 {
        ((self.regs.a as u16) << 8) | (self.regs.f as u16)
    }

    pub fn set_af(&mut self, value: u16) {
        self.regs.a = (value >> 8) as u8;
        self.regs.f = (value as u8) & 0xF0;
    }

    pub fn get_bc(&self) -> u16 {
        ((self.regs.b as u16) << 8) | (self.regs.c as u16)
    }

    pub fn set_bc(&mut self, value: u16) {
        self.regs.b = (value >> 8) as u8;
        self.regs.c = value as u8;
    }

    pub fn get_de(&self) -> u16 {
        ((self.regs.d as u16) << 8) | (self.regs.e as u16)
    }

    pub fn set_de(&mut self, value: u16) {
        self.regs.d = (value >> 8) as u8;
        self.regs.e = value as u8;
    }

    pub fn get_hl(&self) -> u16 {
        ((self.regs.h as u16) << 8) | (self.regs.l as u16)
    }

    pub fn set_hl(&mut self, value: u16) {
        self.regs.h = (value >> 8) as u8;
        self.regs.l = value as u8;
    }

    pub fn get_z(&self) -> bool {
        self.regs.f & 0x80 != 0
    }

    pub fn set_z(&mut self, val: bool) {
        if val {
            self.regs.f |= 0x80;
        } else {
            self.regs.f &= !0x80;
        }
    }

    pub fn get_n(&self) -> bool {
        self.regs.f & 0x40 != 0
    }

    pub fn set_n(&mut self, val: bool) {
        if val {
            self.regs.f |= 0x40;
        } else {
            self.regs.f &= !0x40;
        }
    }

    pub fn get_h(&self) -> bool {
        self.regs.f & 0x20 != 0
    }

    pub fn set_h(&mut self, val: bool) {
        if val {
            self.regs.f |= 0x20;
        } else {
            self.regs.f &= !0x20;
        }
    }

    pub fn get_c(&self) -> bool {
        self.regs.f & 0x10 != 0
    }

    pub fn set_c(&mut self, val: bool) {
        if val {
            self.regs.f |= 0x10;
        } else {
            self.regs.f &= !0x10;
        }
    }
}

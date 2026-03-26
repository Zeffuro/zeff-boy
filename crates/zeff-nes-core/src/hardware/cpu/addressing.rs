use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;

#[allow(dead_code)]
pub(crate) enum Operand {
    Address(u16),
    Accumulator,
    Implied,
}

impl Cpu {
    pub(crate) fn addr_immediate(&mut self, _bus: &mut Bus) -> u16 {
        let addr = self.pc;
        self.pc = self.pc.wrapping_add(1);
        addr
    }

    pub(crate) fn addr_zero_page(&mut self, bus: &mut Bus) -> u16 {
        self.fetch8(bus) as u16
    }

    pub(crate) fn addr_zero_page_x(&mut self, bus: &mut Bus) -> u16 {
        self.fetch8(bus).wrapping_add(self.regs.x) as u16
    }

    pub(crate) fn addr_zero_page_y(&mut self, bus: &mut Bus) -> u16 {
        self.fetch8(bus).wrapping_add(self.regs.y) as u16
    }

    pub(crate) fn addr_absolute(&mut self, bus: &mut Bus) -> u16 {
        self.fetch16(bus)
    }

    pub(crate) fn addr_absolute_x(&mut self, bus: &mut Bus) -> (u16, bool) {
        let base = self.fetch16(bus);
        let addr = base.wrapping_add(self.regs.x as u16);
        let crossed = (base & 0xFF00) != (addr & 0xFF00);
        (addr, crossed)
    }

    pub(crate) fn addr_absolute_y(&mut self, bus: &mut Bus) -> (u16, bool) {
        let base = self.fetch16(bus);
        let addr = base.wrapping_add(self.regs.y as u16);
        let crossed = (base & 0xFF00) != (addr & 0xFF00);
        (addr, crossed)
    }

    pub(crate) fn addr_indirect_x(&mut self, bus: &mut Bus) -> u16 {
        let zp = self.fetch8(bus).wrapping_add(self.regs.x);
        let lo = bus.cpu_read(zp as u16) as u16;
        let hi = bus.cpu_read(zp.wrapping_add(1) as u16) as u16;
        (hi << 8) | lo
    }

    pub(crate) fn addr_indirect_y(&mut self, bus: &mut Bus) -> (u16, bool) {
        let zp = self.fetch8(bus);
        let lo = bus.cpu_read(zp as u16) as u16;
        let hi = bus.cpu_read(zp.wrapping_add(1) as u16) as u16;
        let base = (hi << 8) | lo;
        let addr = base.wrapping_add(self.regs.y as u16);
        let crossed = (base & 0xFF00) != (addr & 0xFF00);
        (addr, crossed)
    }

    pub(crate) fn addr_relative(&mut self, bus: &mut Bus) -> u16 {
        let offset = self.fetch8(bus) as i8;
        self.pc.wrapping_add(offset as u16)
    }

    pub(crate) fn addr_indirect(&mut self, bus: &mut Bus) -> u16 {
        let ptr = self.fetch16(bus);
        let lo = bus.cpu_read(ptr) as u16;
        // 6502 bug: high byte wraps within the same page
        let hi_addr = (ptr & 0xFF00) | ((ptr.wrapping_add(1)) & 0x00FF);
        let hi = bus.cpu_read(hi_addr) as u16;
        (hi << 8) | lo
    }
}


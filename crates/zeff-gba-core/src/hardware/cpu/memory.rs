use super::super::bus::Bus;
use super::*;

impl Cpu {
    pub(crate) fn cpu_read8(&self, bus: &Bus, addr: u32) -> u8 {
        if self.protected_bios_data_read_addr(addr) {
            (self.bios_protected_read_latch >> ((addr & 3) * 8)) as u8
        } else if gba_open_bus_read_addr(addr) {
            (self.open_bus_value(bus) >> ((addr & 3) * 8)) as u8
        } else {
            bus.read8(addr)
        }
    }

    pub(crate) fn cpu_read16(&self, bus: &Bus, addr: u32) -> u16 {
        if self.protected_bios_data_read_addr(addr) {
            (self.bios_protected_read_latch >> ((addr & 2) * 8)) as u16
        } else if gba_open_bus_read_addr(addr) {
            (self.open_bus_value(bus) >> ((addr & 2) * 8)) as u16
        } else {
            bus.read16(addr)
        }
    }

    pub(crate) fn cpu_read32(&self, bus: &Bus, addr: u32) -> u32 {
        if self.protected_bios_data_read_addr(addr) {
            self.bios_protected_read_latch
        } else if gba_open_bus_read_addr(addr) {
            self.open_bus_value(bus)
        } else {
            bus.read32(addr)
        }
    }

    fn protected_bios_data_read_addr(&self, addr: u32) -> bool {
        gba_bios_addr(addr)
            && !self
                .last_fetch
                .is_some_and(|fetched| gba_bios_addr(fetched.pc))
    }

    pub(super) fn track_bios_fetch(&mut self, fetched: FetchedInstruction) {
        if !gba_bios_addr(fetched.pc) {
            return;
        }
        self.bios_protected_read_latch = match fetched.instruction_set {
            InstructionSet::Arm => fetched.raw,
            InstructionSet::Thumb => {
                let halfword = fetched.raw & 0xFFFF;
                halfword | (halfword << 16)
            }
        };
    }

    fn open_bus_value(&self, bus: &Bus) -> u32 {
        let Some(fetched) = self.last_fetch else {
            return 0xFFFF_FFFF;
        };

        match fetched.instruction_set {
            InstructionSet::Arm => bus.read32(fetched.pc.wrapping_add(8)),
            InstructionSet::Thumb => self.thumb_open_bus_value(bus, fetched.pc),
        }
    }

    fn thumb_open_bus_value(&self, bus: &Bus, pc: u32) -> u32 {
        let halfword = |addr| u32::from(bus.read16(addr));
        match pc >> 24 {
            0x00 | 0x07 => {
                let lo = halfword(pc.wrapping_add(if pc & 2 == 0 { 4 } else { 2 }));
                let hi = halfword(pc.wrapping_add(if pc & 2 == 0 { 6 } else { 4 }));
                lo | (hi << 16)
            }
            0x03 => {
                let old = halfword(pc.wrapping_add(2));
                let next = halfword(pc.wrapping_add(4));
                if pc & 2 == 0 {
                    next | (old << 16)
                } else {
                    old | (next << 16)
                }
            }
            _ => {
                let value = halfword(pc.wrapping_add(4));
                value | (value << 16)
            }
        }
    }
}

pub(super) fn gba_open_bus_read_addr(addr: u32) -> bool {
    matches!(addr, 0x0000_4000..=0x01FF_FFFF | 0x1000_0000..=0xFFFF_FFFF)
}

pub(super) fn gba_bios_addr(addr: u32) -> bool {
    addr <= BIOS_END
}

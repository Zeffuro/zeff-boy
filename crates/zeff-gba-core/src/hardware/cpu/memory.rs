use super::super::bus::Bus;
use super::*;

impl Cpu {
    pub(crate) fn cpu_read8(&self, bus: &mut Bus, addr: u32) -> u8 {
        if self.protected_bios_data_read_addr(addr) {
            (self.bios_protected_read_latch >> ((addr & 3) * 8)) as u8
        } else if gba_open_bus_read_addr(addr) {
            (self.open_bus_value(bus) >> ((addr & 3) * 8)) as u8
        } else if let Some(value) = self.cpu_io_read16(bus, addr & !1) {
            (value >> ((addr & 1) * 8)) as u8
        } else {
            bus.read8(addr)
        }
    }

    pub(crate) fn cpu_read16(&self, bus: &mut Bus, addr: u32) -> u16 {
        if self.protected_bios_data_read_addr(addr) {
            (self.bios_protected_read_latch >> ((addr & 2) * 8)) as u16
        } else if gba_open_bus_read_addr(addr) {
            (self.open_bus_value(bus) >> ((addr & 2) * 8)) as u16
        } else if let Some(value) = self.cpu_io_read16(bus, addr) {
            value
        } else {
            bus.read16(addr)
        }
    }

    pub(crate) fn cpu_read32(&self, bus: &mut Bus, addr: u32) -> u32 {
        if self.protected_bios_data_read_addr(addr) {
            self.bios_protected_read_latch
        } else if gba_open_bus_read_addr(addr) {
            self.open_bus_value(bus)
        } else if gba_io_read_addr(addr) {
            let aligned = addr & !3;
            u32::from(
                self.cpu_io_read16(bus, aligned)
                    .unwrap_or_else(|| bus.read16(aligned)),
            ) | (u32::from(
                self.cpu_io_read16(bus, aligned + 2)
                    .unwrap_or_else(|| bus.read16(aligned + 2)),
            ) << 16)
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

    fn cpu_io_read16(&self, bus: &mut Bus, addr: u32) -> Option<u16> {
        let aligned = addr & !1;
        if !matches!(aligned, 0x0400_0000..=0x0400_03FE) {
            return None;
        }

        if gba_io_open_bus_read16_addr(aligned) {
            return Some((self.open_bus_value(bus) >> ((aligned & 2) * 8)) as u16);
        }

        if matches!(aligned & 0x3FF, 0x100..=0x10F) {
            return Some(bus.cpu_read_io16(aligned));
        }

        gba_io_read16_mask(aligned).map(|mask| bus.cpu_read_io16(aligned) & mask)
    }
}

pub(super) fn gba_open_bus_read_addr(addr: u32) -> bool {
    matches!(
        addr,
        0x0000_4000..=0x01FF_FFFF | 0x0400_0400..=0x04FF_FFFF | 0x1000_0000..=0xFFFF_FFFF
    )
}

pub(super) fn gba_bios_addr(addr: u32) -> bool {
    addr <= BIOS_END
}

fn gba_io_read_addr(addr: u32) -> bool {
    matches!(addr, 0x0400_0000..=0x0400_03FF)
}

fn gba_io_open_bus_read16_addr(addr: u32) -> bool {
    let offset = addr & 0x3FF;
    matches!(
        offset,
        0x010..=0x03F
            | 0x040..=0x047
            | 0x04C..=0x04F
            | 0x054..=0x05F
            | 0x08C..=0x08F
            | 0x0A0..=0x0B7
            | 0x0BC..=0x0C3
            | 0x0C8..=0x0CF
            | 0x0D4..=0x0DB
            | 0x0E0..=0x0FF
    )
}

fn gba_io_read16_mask(addr: u32) -> Option<u16> {
    let offset = addr & 0x3FF;
    match offset {
        0x008 | 0x00A => Some(0xDFFF),
        0x048 | 0x04A => Some(0x3F3F),
        0x050 => Some(0x3FFF),
        0x052 => Some(0x1F1F),
        0x060 => Some(0x007F),
        0x062 | 0x068 => Some(0xFFC0),
        0x064 | 0x06C | 0x074 => Some(0x4000),
        0x066 | 0x06A | 0x06E | 0x076 | 0x07A | 0x07E | 0x086 | 0x08A => Some(0x0000),
        0x070 => Some(0x00E0),
        0x072 => Some(0xE000),
        0x078 => Some(0xFF00),
        0x07C => Some(0x40FF),
        0x080 => Some(0xFF77),
        0x0B8 | 0x0C4 | 0x0D0 | 0x0DC => Some(0x0000),
        0x0BA | 0x0C6 | 0x0D2 => Some(0xF7E0),
        0x0DE => Some(0xFFE0),
        0x136 | 0x142 | 0x15A | 0x206 | 0x20A | 0x302 => Some(0x0000),
        _ => None,
    }
}

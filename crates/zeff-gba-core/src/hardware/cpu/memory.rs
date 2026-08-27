use super::super::bus::Bus;
use super::super::timing::{AccessType, DataAccessCompletion};
#[cfg(test)]
use super::super::timing::{
    DataAccessOrigin, TimerIoAccessKind, TimerIoAccessWidth, TimerIoCompletionEvent,
};
use super::*;

impl Cpu {
    pub(crate) fn cpu_read8(&mut self, bus: &mut Bus, addr: u32) -> u8 {
        self.cpu_read8_with_access(bus, addr, AccessType::NonSequential)
    }

    pub(crate) fn cpu_read16(&mut self, bus: &mut Bus, addr: u32) -> u16 {
        self.cpu_read16_with_access(bus, addr, AccessType::NonSequential)
    }

    pub(crate) fn cpu_read32(&mut self, bus: &mut Bus, addr: u32) -> u32 {
        self.cpu_read32_with_access(bus, addr, AccessType::NonSequential)
    }

    pub(super) fn cpu_read32_sequential(&mut self, bus: &mut Bus, addr: u32) -> u32 {
        self.cpu_read32_with_access(bus, addr, AccessType::Sequential)
    }

    pub(super) fn cpu_write8(&mut self, bus: &mut Bus, addr: u32, value: u8) {
        self.cpu_write8_with_access(bus, addr, value, AccessType::NonSequential);
    }

    pub(super) fn cpu_write16(&mut self, bus: &mut Bus, addr: u32, value: u16) {
        self.cpu_write16_with_access(bus, addr, value, AccessType::NonSequential);
    }

    pub(super) fn cpu_write32(&mut self, bus: &mut Bus, addr: u32, value: u32) {
        self.cpu_write32_with_access(bus, addr, value, AccessType::NonSequential);
    }

    pub(super) fn cpu_write32_sequential(&mut self, bus: &mut Bus, addr: u32, value: u32) {
        self.cpu_write32_with_access(bus, addr, value, AccessType::Sequential);
    }

    fn cpu_read8_with_access(&mut self, bus: &mut Bus, addr: u32, access: AccessType) -> u8 {
        let _completion = self.advance_data_access(bus, addr, 1, access, true);
        let value = if self.protected_bios_data_read_addr(addr) {
            (self.bios_protected_read_latch >> ((addr & 3) * 8)) as u8
        } else if gba_open_bus_read_addr(addr) {
            (self.open_bus_value(bus) >> ((addr & 3) * 8)) as u8
        } else if let Some(value) = self.cpu_io_read16(bus, addr & !1) {
            (value >> ((addr & 1) * 8)) as u8
        } else {
            bus.read8(addr)
        };
        #[cfg(test)]
        self.record_timer_io_access(
            _completion.map(|completion| completion.completion_cycle),
            addr,
            TimerIoAccessKind::Read,
            TimerIoAccessWidth::Byte,
            u16::from(value),
        );
        value
    }

    fn cpu_read16_with_access(&mut self, bus: &mut Bus, addr: u32, access: AccessType) -> u16 {
        let _completion = self.advance_data_access(bus, addr, 2, access, true);
        let value = if self.protected_bios_data_read_addr(addr) {
            (self.bios_protected_read_latch >> ((addr & 2) * 8)) as u16
        } else if gba_open_bus_read_addr(addr) {
            (self.open_bus_value(bus) >> ((addr & 2) * 8)) as u16
        } else if let Some(value) = self.cpu_io_read16(bus, addr) {
            value
        } else {
            bus.read16(addr)
        };
        #[cfg(test)]
        self.record_timer_io_access(
            _completion.map(|completion| completion.completion_cycle),
            addr & !1,
            TimerIoAccessKind::Read,
            TimerIoAccessWidth::Halfword,
            value,
        );
        value
    }

    fn cpu_read32_with_access(&mut self, bus: &mut Bus, addr: u32, access: AccessType) -> u32 {
        let _completion = self.advance_data_access(bus, addr, 4, access, true);
        let value = if self.protected_bios_data_read_addr(addr) {
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
        };
        #[cfg(test)]
        if let Some(completion) = _completion {
            let aligned = addr & !3;
            self.record_timer_io_access(
                Some(completion.first_completion_cycle),
                aligned,
                TimerIoAccessKind::Read,
                TimerIoAccessWidth::Halfword,
                value as u16,
            );
            self.record_timer_io_access(
                Some(
                    completion
                        .second_halfword_completion_cycle
                        .unwrap_or(completion.completion_cycle),
                ),
                aligned.wrapping_add(2),
                TimerIoAccessKind::Read,
                TimerIoAccessWidth::Halfword,
                (value >> 16) as u16,
            );
        }
        value
    }

    fn cpu_write8_with_access(&mut self, bus: &mut Bus, addr: u32, value: u8, access: AccessType) {
        let before_completion = timer_control_disable_write(bus, addr, 1, u32::from(value));
        let _completion = self.advance_data_access(bus, addr, 1, access, before_completion);
        bus.write8(addr, value);
        #[cfg(test)]
        self.record_timer_io_access(
            _completion.map(|completion| completion.completion_cycle),
            addr,
            TimerIoAccessKind::Write,
            TimerIoAccessWidth::Byte,
            u16::from(value),
        );
    }

    fn cpu_write16_with_access(
        &mut self,
        bus: &mut Bus,
        addr: u32,
        value: u16,
        access: AccessType,
    ) {
        let before_completion = timer_control_disable_write(bus, addr, 2, u32::from(value));
        let _completion = self.advance_data_access(bus, addr, 2, access, before_completion);
        bus.write16(addr, value);
        #[cfg(test)]
        self.record_timer_io_access(
            _completion.map(|completion| completion.completion_cycle),
            addr & !1,
            TimerIoAccessKind::Write,
            TimerIoAccessWidth::Halfword,
            value,
        );
    }

    fn cpu_write32_with_access(
        &mut self,
        bus: &mut Bus,
        addr: u32,
        value: u32,
        access: AccessType,
    ) {
        let before_completion = timer_control_disable_write(bus, addr, 4, value);
        let _completion = self.advance_data_access(bus, addr, 4, access, before_completion);
        bus.write32(addr, value);
        #[cfg(test)]
        if let Some(completion) = _completion {
            let aligned = addr & !3;
            self.record_timer_io_access(
                Some(completion.first_completion_cycle),
                aligned,
                TimerIoAccessKind::Write,
                TimerIoAccessWidth::Halfword,
                value as u16,
            );
            self.record_timer_io_access(
                Some(
                    completion
                        .second_halfword_completion_cycle
                        .unwrap_or(completion.completion_cycle),
                ),
                aligned.wrapping_add(2),
                TimerIoAccessKind::Write,
                TimerIoAccessWidth::Halfword,
                (value >> 16) as u16,
            );
        }
    }

    pub(super) fn record_data_access_only(&mut self, bus: &mut Bus, addr: u32, width_bytes: u8) {
        let _ = self.advance_data_access(bus, addr, width_bytes, AccessType::NonSequential, true);
    }

    fn advance_data_access(
        &mut self,
        bus: &mut Bus,
        addr: u32,
        width_bytes: u8,
        access: AccessType,
        before_completion: bool,
    ) -> Option<DataAccessCompletion> {
        if !self.data_access_timing_active {
            return None;
        }
        let completion = self
            .data_access_cursor
            .advance(addr, width_bytes, access, bus.waitcnt());
        let phase = completion
            .completion_cycle
            .saturating_sub(u32::from(before_completion));
        self.advance_bus_phase(bus, phase);
        Some(completion)
    }

    #[cfg(test)]
    fn record_timer_io_access(
        &mut self,
        completion_cycle: Option<u32>,
        address: u32,
        kind: TimerIoAccessKind,
        width: TimerIoAccessWidth,
        value: u16,
    ) {
        let Some(completion_cycle) = completion_cycle else {
            return;
        };
        if let Some(event) = TimerIoCompletionEvent::new(
            DataAccessOrigin::Cpu,
            completion_cycle,
            address,
            kind,
            width,
            value,
        ) {
            self.timer_io_completion_events.push(event);
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

    fn cpu_io_read16(&self, bus: &Bus, addr: u32) -> Option<u16> {
        let aligned = addr & !1;
        if !matches!(aligned, IO_START..=IO_LAST_ALIGNED_HALFWORD_ADDR) {
            return None;
        }

        if gba_io_open_bus_read16_addr(aligned) {
            return Some((self.open_bus_value(bus) >> ((aligned & 2) * 8)) as u16);
        }

        gba_io_read16_mask(aligned).map(|mask| bus.read16(aligned) & mask)
    }
}

fn timer_control_disable_write(bus: &Bus, addr: u32, width_bytes: u8, value: u32) -> bool {
    let (control_addr, control_value) = match width_bytes {
        1 if addr & 1 == 0 => (addr, value as u16),
        2 => (addr & !1, value as u16),
        4 => ((addr & !3).wrapping_add(2), (value >> 16) as u16),
        _ => return false,
    };
    let Some(offset) = control_addr.checked_sub(IO_START + 0x102) else {
        return false;
    };
    if offset > 0x0C || offset & 3 != 0 {
        return false;
    }
    let index = (offset / 4) as usize;
    bus.timers.read16(index, true) & 0x0080 != 0 && control_value & 0x0080 == 0
}

pub(super) fn gba_open_bus_read_addr(addr: u32) -> bool {
    matches!(
        addr,
        0x0000_4000..=0x01FF_FFFF
            | IO_UNUSED_START..=0x04FF_FFFF
            | 0x1000_0000..=0xFFFF_FFFF
    )
}

pub(super) fn gba_bios_addr(addr: u32) -> bool {
    addr <= BIOS_END
}

fn gba_io_read_addr(addr: u32) -> bool {
    matches!(addr, IO_START..=IO_END)
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

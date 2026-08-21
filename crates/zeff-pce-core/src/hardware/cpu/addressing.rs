use super::{Cpu, CpuBus};

const DIRECT_PAGE_BASE: u16 = 0x2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AddressMode {
    Immediate,
    DirectPage,
    DirectPageX,
    DirectPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    IndexedIndirect,
    Indirect,
    IndirectIndexed,
}

impl AddressMode {
    pub(super) const fn cycles(self) -> u32 {
        match self {
            Self::Immediate => 2,
            Self::DirectPage | Self::DirectPageX | Self::DirectPageY => 4,
            Self::Absolute | Self::AbsoluteX | Self::AbsoluteY => 5,
            Self::IndexedIndirect | Self::Indirect | Self::IndirectIndexed => 7,
        }
    }
}

impl Cpu {
    pub(super) fn read_operand<B: CpuBus>(&mut self, bus: &mut B, mode: AddressMode) -> u8 {
        if mode == AddressMode::Immediate {
            return self.fetch(bus);
        }

        let address = self.operand_address(bus, mode);
        self.read(bus, address)
    }

    pub(super) fn write_operand<B: CpuBus>(&mut self, bus: &mut B, mode: AddressMode, value: u8) {
        debug_assert_ne!(mode, AddressMode::Immediate);
        let address = self.operand_address(bus, mode);
        self.write(bus, address, value);
    }

    pub(super) fn operand_address<B: CpuBus>(&mut self, bus: &mut B, mode: AddressMode) -> u16 {
        match mode {
            AddressMode::Immediate => unreachable!("immediate operands have no address"),
            AddressMode::DirectPage => {
                let offset = self.fetch(bus);
                bus.idle();
                direct_page_address(offset)
            }
            AddressMode::DirectPageX => {
                let offset = self.fetch(bus).wrapping_add(self.registers.x);
                bus.idle();
                direct_page_address(offset)
            }
            AddressMode::DirectPageY => {
                let offset = self.fetch(bus).wrapping_add(self.registers.y);
                bus.idle();
                direct_page_address(offset)
            }
            AddressMode::Absolute => {
                let address = self.fetch_word(bus);
                bus.idle();
                address
            }
            AddressMode::AbsoluteX => {
                let address = self
                    .fetch_word(bus)
                    .wrapping_add(u16::from(self.registers.x));
                bus.idle();
                address
            }
            AddressMode::AbsoluteY => {
                let address = self
                    .fetch_word(bus)
                    .wrapping_add(u16::from(self.registers.y));
                bus.idle();
                address
            }
            AddressMode::IndexedIndirect => {
                let pointer = self.fetch(bus).wrapping_add(self.registers.x);
                bus.idle();
                let address = self.read_direct_page_word(bus, pointer);
                bus.idle();
                address
            }
            AddressMode::Indirect => {
                let pointer = self.fetch(bus);
                bus.idle();
                let address = self.read_direct_page_word(bus, pointer);
                bus.idle();
                address
            }
            AddressMode::IndirectIndexed => {
                let pointer = self.fetch(bus);
                bus.idle();
                let address = self
                    .read_direct_page_word(bus, pointer)
                    .wrapping_add(u16::from(self.registers.y));
                bus.idle();
                address
            }
        }
    }

    pub(super) fn fetch_word<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let low = self.fetch(bus);
        let high = self.fetch(bus);
        u16::from_le_bytes([low, high])
    }

    fn read_direct_page_word<B: CpuBus>(&self, bus: &mut B, pointer: u8) -> u16 {
        let low = self.read(bus, direct_page_address(pointer));
        let high = self.read(bus, direct_page_address(pointer.wrapping_add(1)));
        u16::from_le_bytes([low, high])
    }
}

#[inline]
pub(super) const fn direct_page_address(offset: u8) -> u16 {
    DIRECT_PAGE_BASE | offset as u16
}

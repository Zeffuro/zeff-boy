use super::{
    Cpu, CpuBus, StatusFlags,
    addressing::{AddressMode, direct_page_address},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestAndModify {
    Set,
    Reset,
}

impl Cpu {
    pub(super) fn execute_bit_operations<B: CpuBus>(
        &mut self,
        bus: &mut B,
        opcode: u8,
    ) -> Option<u32> {
        let cycles = match opcode {
            0x89 => self.bit(bus, AddressMode::Immediate),
            0x24 => self.bit(bus, AddressMode::DirectPage),
            0x34 => self.bit(bus, AddressMode::DirectPageX),
            0x2C => self.bit(bus, AddressMode::Absolute),
            0x3C => self.bit(bus, AddressMode::AbsoluteX),
            0x83 => self.test_memory(bus, AddressMode::DirectPage),
            0xA3 => self.test_memory(bus, AddressMode::DirectPageX),
            0x93 => self.test_memory(bus, AddressMode::Absolute),
            0xB3 => self.test_memory(bus, AddressMode::AbsoluteX),
            0x04 => self.test_and_modify(bus, AddressMode::DirectPage, TestAndModify::Set),
            0x0C => self.test_and_modify(bus, AddressMode::Absolute, TestAndModify::Set),
            0x14 => self.test_and_modify(bus, AddressMode::DirectPage, TestAndModify::Reset),
            0x1C => self.test_and_modify(bus, AddressMode::Absolute, TestAndModify::Reset),
            opcode if opcode & 0x0F == 0x07 => self.modify_direct_page_bit(bus, opcode),
            opcode if opcode & 0x0F == 0x0F => self.branch_on_direct_page_bit(bus, opcode),
            _ => return None,
        };
        Some(cycles)
    }

    fn bit<B: CpuBus>(&mut self, bus: &mut B, mode: AddressMode) -> u32 {
        let value = self.read_operand(bus, mode);
        self.update_bit_test_flags(self.registers.a, value);
        mode.cycles()
    }

    fn test_memory<B: CpuBus>(&mut self, bus: &mut B, mode: AddressMode) -> u32 {
        let mask = self.fetch(bus);
        let address = self.operand_address(bus, mode);
        bus.idle();
        let value = self.read(bus, address);
        bus.idle();
        self.update_bit_test_flags(mask, value);
        mode.cycles() + 3
    }

    fn test_and_modify<B: CpuBus>(
        &mut self,
        bus: &mut B,
        mode: AddressMode,
        operation: TestAndModify,
    ) -> u32 {
        let address = self.operand_address(bus, mode);
        let value = self.read(bus, address);
        self.update_bit_test_flags(self.registers.a, value);
        let result = match operation {
            TestAndModify::Set => value | self.registers.a,
            TestAndModify::Reset => value & !self.registers.a,
        };
        bus.idle();
        self.write(bus, address, result);
        mode.cycles() + 2
    }

    fn modify_direct_page_bit<B: CpuBus>(&mut self, bus: &mut B, opcode: u8) -> u32 {
        let bit = (opcode >> 4) & 0x07;
        let set = opcode & 0x80 != 0;
        let address = direct_page_address(self.fetch(bus));
        bus.idle();
        let value = self.read(bus, address);
        let mask = 1 << bit;
        let result = if set { value | mask } else { value & !mask };
        bus.idle();
        bus.idle();
        self.write(bus, address, result);
        7
    }

    fn branch_on_direct_page_bit<B: CpuBus>(&mut self, bus: &mut B, opcode: u8) -> u32 {
        let bit = (opcode >> 4) & 0x07;
        let branch_when_set = opcode & 0x80 != 0;
        let address = direct_page_address(self.fetch(bus));
        bus.idle();
        let offset = self.fetch(bus) as i8;
        bus.idle();
        let value = self.read(bus, address);
        let bit_is_set = value & (1 << bit) != 0;
        if bit_is_set != branch_when_set {
            return 6;
        }
        bus.idle();
        bus.idle();
        self.registers.pc = self.registers.pc.wrapping_add_signed(i16::from(offset));
        8
    }

    fn update_bit_test_flags(&mut self, mask: u8, value: u8) {
        self.registers
            .status
            .set(StatusFlags::ZERO, mask & value == 0);
        self.registers
            .status
            .set(StatusFlags::NEGATIVE, value & 0x80 != 0);
        self.registers
            .status
            .set(StatusFlags::OVERFLOW, value & 0x40 != 0);
    }
}

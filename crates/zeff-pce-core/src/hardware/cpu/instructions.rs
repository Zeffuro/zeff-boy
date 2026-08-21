use super::{Cpu, CpuBus, StatusFlags, addressing::AddressMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Register {
    Accumulator,
    X,
    Y,
    StackPointer,
}

impl Cpu {
    pub(super) fn execute_baseline<B: CpuBus>(&mut self, bus: &mut B, opcode: u8) -> Option<u32> {
        let instruction = match opcode {
            0xA9 => self.load(bus, Register::Accumulator, AddressMode::Immediate),
            0xA5 => self.load(bus, Register::Accumulator, AddressMode::DirectPage),
            0xB5 => self.load(bus, Register::Accumulator, AddressMode::DirectPageX),
            0xAD => self.load(bus, Register::Accumulator, AddressMode::Absolute),
            0xBD => self.load(bus, Register::Accumulator, AddressMode::AbsoluteX),
            0xB9 => self.load(bus, Register::Accumulator, AddressMode::AbsoluteY),
            0xA1 => self.load(bus, Register::Accumulator, AddressMode::IndexedIndirect),
            0xB2 => self.load(bus, Register::Accumulator, AddressMode::Indirect),
            0xB1 => self.load(bus, Register::Accumulator, AddressMode::IndirectIndexed),
            0xA2 => self.load(bus, Register::X, AddressMode::Immediate),
            0xA6 => self.load(bus, Register::X, AddressMode::DirectPage),
            0xB6 => self.load(bus, Register::X, AddressMode::DirectPageY),
            0xAE => self.load(bus, Register::X, AddressMode::Absolute),
            0xBE => self.load(bus, Register::X, AddressMode::AbsoluteY),
            0xA0 => self.load(bus, Register::Y, AddressMode::Immediate),
            0xA4 => self.load(bus, Register::Y, AddressMode::DirectPage),
            0xB4 => self.load(bus, Register::Y, AddressMode::DirectPageX),
            0xAC => self.load(bus, Register::Y, AddressMode::Absolute),
            0xBC => self.load(bus, Register::Y, AddressMode::AbsoluteX),

            0x81 => self.store(bus, Register::Accumulator, AddressMode::IndexedIndirect),
            0x91 => self.store(bus, Register::Accumulator, AddressMode::IndirectIndexed),
            0x92 => self.store(bus, Register::Accumulator, AddressMode::Indirect),
            0x85 => self.store(bus, Register::Accumulator, AddressMode::DirectPage),
            0x95 => self.store(bus, Register::Accumulator, AddressMode::DirectPageX),
            0x8D => self.store(bus, Register::Accumulator, AddressMode::Absolute),
            0x9D => self.store(bus, Register::Accumulator, AddressMode::AbsoluteX),
            0x99 => self.store(bus, Register::Accumulator, AddressMode::AbsoluteY),
            0x86 => self.store(bus, Register::X, AddressMode::DirectPage),
            0x96 => self.store(bus, Register::X, AddressMode::DirectPageY),
            0x8E => self.store(bus, Register::X, AddressMode::Absolute),
            0x84 => self.store(bus, Register::Y, AddressMode::DirectPage),
            0x94 => self.store(bus, Register::Y, AddressMode::DirectPageX),
            0x8C => self.store(bus, Register::Y, AddressMode::Absolute),
            0x64 => self.store_zero(bus, AddressMode::DirectPage),
            0x74 => self.store_zero(bus, AddressMode::DirectPageX),
            0x9C => self.store_zero(bus, AddressMode::Absolute),
            0x9E => self.store_zero(bus, AddressMode::AbsoluteX),

            0xAA => self.transfer(bus, Register::Accumulator, Register::X, true),
            0xA8 => self.transfer(bus, Register::Accumulator, Register::Y, true),
            0x8A => self.transfer(bus, Register::X, Register::Accumulator, true),
            0x98 => self.transfer(bus, Register::Y, Register::Accumulator, true),
            0xBA => self.transfer(bus, Register::StackPointer, Register::X, true),
            0x9A => self.transfer(bus, Register::X, Register::StackPointer, false),

            0x18 => self.update_flag(bus, StatusFlags::CARRY, false),
            0x38 => self.update_flag(bus, StatusFlags::CARRY, true),
            0x58 => self.update_flag(bus, StatusFlags::INTERRUPT, false),
            0x78 => self.update_flag(bus, StatusFlags::INTERRUPT, true),
            0xB8 => self.update_flag(bus, StatusFlags::OVERFLOW, false),
            0xD8 => self.update_flag(bus, StatusFlags::DECIMAL, false),
            0xF8 => self.update_flag(bus, StatusFlags::DECIMAL, true),
            _ => return None,
        };

        Some(instruction)
    }

    fn load<B: CpuBus>(&mut self, bus: &mut B, register: Register, mode: AddressMode) -> u32 {
        let value = self.read_operand(bus, mode);
        self.write_register(register, value);
        self.update_negative_zero(value);
        mode.cycles()
    }

    fn store<B: CpuBus>(&mut self, bus: &mut B, register: Register, mode: AddressMode) -> u32 {
        let value = self.read_register(register);
        self.write_operand(bus, mode, value);
        mode.cycles()
    }

    fn store_zero<B: CpuBus>(&mut self, bus: &mut B, mode: AddressMode) -> u32 {
        self.write_operand(bus, mode, 0);
        mode.cycles()
    }

    fn transfer<B: CpuBus>(
        &mut self,
        bus: &mut B,
        source: Register,
        destination: Register,
        update_flags: bool,
    ) -> u32 {
        let value = self.read_register(source);
        self.write_register(destination, value);
        if update_flags {
            self.update_negative_zero(value);
        }
        self.dummy_fetch(bus);
        2
    }

    fn update_flag<B: CpuBus>(&mut self, bus: &mut B, flag: StatusFlags, value: bool) -> u32 {
        self.registers.status.set(flag, value);
        self.dummy_fetch(bus);
        2
    }

    pub(super) fn read_register(&self, register: Register) -> u8 {
        match register {
            Register::Accumulator => self.registers.a,
            Register::X => self.registers.x,
            Register::Y => self.registers.y,
            Register::StackPointer => self.registers.sp,
        }
    }

    pub(super) fn write_register(&mut self, register: Register, value: u8) {
        match register {
            Register::Accumulator => self.registers.a = value,
            Register::X => self.registers.x = value,
            Register::Y => self.registers.y = value,
            Register::StackPointer => self.registers.sp = value,
        }
    }
}

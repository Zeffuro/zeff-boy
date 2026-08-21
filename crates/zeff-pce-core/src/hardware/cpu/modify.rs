use super::{Cpu, CpuBus, StatusFlags, addressing::AddressMode, instructions::Register};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModifyOperation {
    Increment,
    Decrement,
    ShiftLeft,
    ShiftRight,
    RotateLeft,
    RotateRight,
}

impl Cpu {
    pub(super) fn execute_modify<B: CpuBus>(&mut self, bus: &mut B, opcode: u8) -> Option<u32> {
        let cycles = match opcode {
            0xE0 => self.compare_register(bus, Register::X, AddressMode::Immediate),
            0xE4 => self.compare_register(bus, Register::X, AddressMode::DirectPage),
            0xEC => self.compare_register(bus, Register::X, AddressMode::Absolute),
            0xC0 => self.compare_register(bus, Register::Y, AddressMode::Immediate),
            0xC4 => self.compare_register(bus, Register::Y, AddressMode::DirectPage),
            0xCC => self.compare_register(bus, Register::Y, AddressMode::Absolute),

            0xE8 => self.modify_register(bus, Register::X, ModifyOperation::Increment),
            0xC8 => self.modify_register(bus, Register::Y, ModifyOperation::Increment),
            0xCA => self.modify_register(bus, Register::X, ModifyOperation::Decrement),
            0x88 => self.modify_register(bus, Register::Y, ModifyOperation::Decrement),
            0x1A => self.modify_register(bus, Register::Accumulator, ModifyOperation::Increment),
            0x3A => self.modify_register(bus, Register::Accumulator, ModifyOperation::Decrement),
            0xE6 => self.modify_memory(bus, AddressMode::DirectPage, ModifyOperation::Increment),
            0xF6 => self.modify_memory(bus, AddressMode::DirectPageX, ModifyOperation::Increment),
            0xEE => self.modify_memory(bus, AddressMode::Absolute, ModifyOperation::Increment),
            0xFE => self.modify_memory(bus, AddressMode::AbsoluteX, ModifyOperation::Increment),
            0xC6 => self.modify_memory(bus, AddressMode::DirectPage, ModifyOperation::Decrement),
            0xD6 => self.modify_memory(bus, AddressMode::DirectPageX, ModifyOperation::Decrement),
            0xCE => self.modify_memory(bus, AddressMode::Absolute, ModifyOperation::Decrement),
            0xDE => self.modify_memory(bus, AddressMode::AbsoluteX, ModifyOperation::Decrement),

            0x0A => self.modify_register(bus, Register::Accumulator, ModifyOperation::ShiftLeft),
            0x06 => self.modify_memory(bus, AddressMode::DirectPage, ModifyOperation::ShiftLeft),
            0x16 => self.modify_memory(bus, AddressMode::DirectPageX, ModifyOperation::ShiftLeft),
            0x0E => self.modify_memory(bus, AddressMode::Absolute, ModifyOperation::ShiftLeft),
            0x1E => self.modify_memory(bus, AddressMode::AbsoluteX, ModifyOperation::ShiftLeft),
            0x4A => self.modify_register(bus, Register::Accumulator, ModifyOperation::ShiftRight),
            0x46 => self.modify_memory(bus, AddressMode::DirectPage, ModifyOperation::ShiftRight),
            0x56 => self.modify_memory(bus, AddressMode::DirectPageX, ModifyOperation::ShiftRight),
            0x4E => self.modify_memory(bus, AddressMode::Absolute, ModifyOperation::ShiftRight),
            0x5E => self.modify_memory(bus, AddressMode::AbsoluteX, ModifyOperation::ShiftRight),
            0x2A => self.modify_register(bus, Register::Accumulator, ModifyOperation::RotateLeft),
            0x26 => self.modify_memory(bus, AddressMode::DirectPage, ModifyOperation::RotateLeft),
            0x36 => self.modify_memory(bus, AddressMode::DirectPageX, ModifyOperation::RotateLeft),
            0x2E => self.modify_memory(bus, AddressMode::Absolute, ModifyOperation::RotateLeft),
            0x3E => self.modify_memory(bus, AddressMode::AbsoluteX, ModifyOperation::RotateLeft),
            0x6A => self.modify_register(bus, Register::Accumulator, ModifyOperation::RotateRight),
            0x66 => self.modify_memory(bus, AddressMode::DirectPage, ModifyOperation::RotateRight),
            0x76 => self.modify_memory(bus, AddressMode::DirectPageX, ModifyOperation::RotateRight),
            0x6E => self.modify_memory(bus, AddressMode::Absolute, ModifyOperation::RotateRight),
            0x7E => self.modify_memory(bus, AddressMode::AbsoluteX, ModifyOperation::RotateRight),
            _ => return None,
        };
        Some(cycles)
    }

    fn compare_register<B: CpuBus>(
        &mut self,
        bus: &mut B,
        register: Register,
        mode: AddressMode,
    ) -> u32 {
        let operand = self.read_operand(bus, mode);
        self.compare(self.read_register(register), operand);
        mode.cycles()
    }

    fn modify_register<B: CpuBus>(
        &mut self,
        bus: &mut B,
        register: Register,
        operation: ModifyOperation,
    ) -> u32 {
        let result = self.modify_value(operation, self.read_register(register));
        self.write_register(register, result);
        self.dummy_fetch(bus);
        2
    }

    fn modify_memory<B: CpuBus>(
        &mut self,
        bus: &mut B,
        mode: AddressMode,
        operation: ModifyOperation,
    ) -> u32 {
        let address = self.operand_address(bus, mode);
        let value = self.read(bus, address);
        let result = self.modify_value(operation, value);
        bus.idle();
        self.write(bus, address, result);
        mode.cycles() + 2
    }

    fn modify_value(&mut self, operation: ModifyOperation, value: u8) -> u8 {
        let carry = self.registers.status.contains(StatusFlags::CARRY);
        let result = match operation {
            ModifyOperation::Increment => value.wrapping_add(1),
            ModifyOperation::Decrement => value.wrapping_sub(1),
            ModifyOperation::ShiftLeft => {
                self.registers
                    .status
                    .set(StatusFlags::CARRY, value & 0x80 != 0);
                value << 1
            }
            ModifyOperation::ShiftRight => {
                self.registers
                    .status
                    .set(StatusFlags::CARRY, value & 0x01 != 0);
                value >> 1
            }
            ModifyOperation::RotateLeft => {
                self.registers
                    .status
                    .set(StatusFlags::CARRY, value & 0x80 != 0);
                (value << 1) | u8::from(carry)
            }
            ModifyOperation::RotateRight => {
                self.registers
                    .status
                    .set(StatusFlags::CARRY, value & 0x01 != 0);
                (value >> 1) | (u8::from(carry) << 7)
            }
        };
        self.update_negative_zero(result);
        result
    }
}

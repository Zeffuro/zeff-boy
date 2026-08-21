use super::{
    Cpu, CpuBus, StatusFlags,
    addressing::{AddressMode, direct_page_address},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AluOperation {
    Or,
    And,
    ExclusiveOr,
    Add,
    Subtract,
    Compare,
}

impl Cpu {
    pub(super) fn execute_alu<B: CpuBus>(
        &mut self,
        bus: &mut B,
        opcode: u8,
        memory_operation: bool,
    ) -> Option<u32> {
        let (operation, mode) = decode(opcode)?;
        let operand = self.read_operand(bus, mode);
        let mut cycles = mode.cycles();

        if memory_operation && operation.uses_memory_operation() {
            let target = direct_page_address(self.registers.x);
            let value = self.read(bus, target);
            let result = self.apply_alu_operation(operation, value, operand);
            if operation == AluOperation::Add
                && self.registers.status.contains(StatusFlags::DECIMAL)
            {
                bus.idle();
                cycles += 1;
            }
            bus.idle();
            self.write(bus, target, result);
            return Some(cycles + 3);
        }

        if operation == AluOperation::Compare {
            self.compare(self.registers.a, operand);
        } else {
            let result = self.apply_alu_operation(operation, self.registers.a, operand);
            self.registers.a = result;
        }

        if operation.is_arithmetic() && self.registers.status.contains(StatusFlags::DECIMAL) {
            if memory_operation {
                bus.idle();
            } else {
                self.dummy_fetch(bus);
            }
            cycles += 1;
        }

        Some(cycles)
    }

    pub(super) fn update_negative_zero(&mut self, value: u8) {
        self.registers.status.set(StatusFlags::ZERO, value == 0);
        self.registers
            .status
            .set(StatusFlags::NEGATIVE, value & 0x80 != 0);
    }

    fn apply_alu_operation(&mut self, operation: AluOperation, value: u8, operand: u8) -> u8 {
        match operation {
            AluOperation::Or => {
                let result = value | operand;
                self.update_negative_zero(result);
                result
            }
            AluOperation::And => {
                let result = value & operand;
                self.update_negative_zero(result);
                result
            }
            AluOperation::ExclusiveOr => {
                let result = value ^ operand;
                self.update_negative_zero(result);
                result
            }
            AluOperation::Add => self.add(value, operand),
            AluOperation::Subtract => self.subtract(value, operand),
            AluOperation::Compare => unreachable!("compare does not produce a stored result"),
        }
    }

    fn add(&mut self, value: u8, operand: u8) -> u8 {
        let carry = u16::from(self.registers.status.contains(StatusFlags::CARRY));
        if self.registers.status.contains(StatusFlags::DECIMAL) {
            let mut low = u16::from(value & 0x0F) + u16::from(operand & 0x0F) + carry;
            if low >= 10 {
                low += 6;
            }
            let mut high = u16::from(value >> 4) + u16::from(operand >> 4) + u16::from(low > 0x0F);
            if high >= 10 {
                high += 6;
            }

            let result = ((high << 4) | (low & 0x0F)) as u8;
            self.registers.status.set(StatusFlags::CARRY, high > 0x0F);
            self.update_negative_zero(result);
            return result;
        }

        let sum = u16::from(value) + u16::from(operand) + carry;
        let result = sum as u8;
        self.registers.status.set(StatusFlags::CARRY, sum > 0xFF);
        self.registers.status.set(
            StatusFlags::OVERFLOW,
            !(value ^ operand) & (value ^ result) & 0x80 != 0,
        );
        self.update_negative_zero(result);
        result
    }

    fn subtract(&mut self, value: u8, operand: u8) -> u8 {
        let carry = i16::from(self.registers.status.contains(StatusFlags::CARRY));
        let borrow = 1 - carry;
        if self.registers.status.contains(StatusFlags::DECIMAL) {
            let mut low = i16::from(value & 0x0F) - i16::from(operand & 0x0F) - borrow;
            let mut high = i16::from(value >> 4) - i16::from(operand >> 4);
            if low < 0 {
                low -= 6;
                high -= 1;
            }
            let has_no_borrow = high >= 0;
            if high < 0 {
                high -= 6;
            }

            let result = ((high.rem_euclid(16) as u8) << 4) | low.rem_euclid(16) as u8;
            self.registers.status.set(StatusFlags::CARRY, has_no_borrow);
            self.update_negative_zero(result);
            return result;
        }

        let difference = i16::from(value) - i16::from(operand) - borrow;
        let result = difference as u8;
        self.registers
            .status
            .set(StatusFlags::CARRY, difference >= 0);
        self.registers.status.set(
            StatusFlags::OVERFLOW,
            (value ^ operand) & (value ^ result) & 0x80 != 0,
        );
        self.update_negative_zero(result);
        result
    }

    pub(super) fn compare(&mut self, value: u8, operand: u8) {
        let result = value.wrapping_sub(operand);
        self.registers
            .status
            .set(StatusFlags::CARRY, value >= operand);
        self.update_negative_zero(result);
    }
}

impl AluOperation {
    const fn uses_memory_operation(self) -> bool {
        matches!(self, Self::Or | Self::And | Self::ExclusiveOr | Self::Add)
    }

    const fn is_arithmetic(self) -> bool {
        matches!(self, Self::Add | Self::Subtract)
    }
}

fn decode(opcode: u8) -> Option<(AluOperation, AddressMode)> {
    let operation = match opcode & 0xE0 {
        0x00 => AluOperation::Or,
        0x20 => AluOperation::And,
        0x40 => AluOperation::ExclusiveOr,
        0x60 => AluOperation::Add,
        0xC0 => AluOperation::Compare,
        0xE0 => AluOperation::Subtract,
        _ => return None,
    };
    let mode = match opcode & 0x1F {
        0x09 => AddressMode::Immediate,
        0x05 => AddressMode::DirectPage,
        0x15 => AddressMode::DirectPageX,
        0x0D => AddressMode::Absolute,
        0x1D => AddressMode::AbsoluteX,
        0x19 => AddressMode::AbsoluteY,
        0x01 => AddressMode::IndexedIndirect,
        0x12 => AddressMode::Indirect,
        0x11 => AddressMode::IndirectIndexed,
        _ => return None,
    };
    Some((operation, mode))
}

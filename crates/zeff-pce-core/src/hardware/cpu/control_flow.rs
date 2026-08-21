use super::{Cpu, CpuBus, StatusFlags, instructions::Register};

const STACK_BASE: u16 = 0x2100;
const BRK_VECTOR_LOW: u16 = 0xFFF6;
const BRK_VECTOR_HIGH: u16 = 0xFFF7;

impl Cpu {
    pub(super) fn execute_control<B: CpuBus>(&mut self, bus: &mut B, opcode: u8) -> Option<u32> {
        let cycles = match opcode {
            0x48 => self.push_register(bus, Register::Accumulator),
            0x08 => self.push_status(bus),
            0xDA => self.push_register(bus, Register::X),
            0x5A => self.push_register(bus, Register::Y),
            0x68 => self.pull_register(bus, Register::Accumulator),
            0x28 => self.pull_status(bus),
            0xFA => self.pull_register(bus, Register::X),
            0x7A => self.pull_register(bus, Register::Y),
            0x20 => self.jump_to_subroutine(bus),
            0x44 => self.branch_to_subroutine(bus),
            0x60 => self.return_from_subroutine(bus),
            0x40 => self.return_from_interrupt(bus),
            0x00 => self.break_interrupt(bus),
            0x4C => self.jump_absolute(bus),
            0x6C => self.jump_indirect(bus, false),
            0x7C => self.jump_indirect(bus, true),
            0x10 => self.branch(bus, !self.registers.status.contains(StatusFlags::NEGATIVE)),
            0x30 => self.branch(bus, self.registers.status.contains(StatusFlags::NEGATIVE)),
            0x50 => self.branch(bus, !self.registers.status.contains(StatusFlags::OVERFLOW)),
            0x70 => self.branch(bus, self.registers.status.contains(StatusFlags::OVERFLOW)),
            0x90 => self.branch(bus, !self.registers.status.contains(StatusFlags::CARRY)),
            0xB0 => self.branch(bus, self.registers.status.contains(StatusFlags::CARRY)),
            0xD0 => self.branch(bus, !self.registers.status.contains(StatusFlags::ZERO)),
            0xF0 => self.branch(bus, self.registers.status.contains(StatusFlags::ZERO)),
            0x80 => self.branch_always(bus),
            _ => return None,
        };
        Some(cycles)
    }

    fn push_register<B: CpuBus>(&mut self, bus: &mut B, register: Register) -> u32 {
        let value = self.read_register(register);
        self.dummy_fetch(bus);
        self.push(bus, value);
        3
    }

    fn push_status<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        self.dummy_fetch(bus);
        self.push(bus, self.status_for_stack(true));
        3
    }

    fn pull_register<B: CpuBus>(&mut self, bus: &mut B, register: Register) -> u32 {
        self.dummy_fetch(bus);
        bus.idle();
        let value = self.pull(bus);
        self.write_register(register, value);
        self.update_negative_zero(value);
        4
    }

    fn pull_status<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        self.dummy_fetch(bus);
        bus.idle();
        let value = self.pull(bus);
        self.restore_status(value);
        4
    }

    fn jump_to_subroutine<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        let low = self.fetch(bus);
        bus.idle();
        let return_address = self.registers.pc;
        self.push(bus, (return_address >> 8) as u8);
        self.push(bus, return_address as u8);
        let high = self.fetch(bus);
        bus.idle();
        self.registers.pc = u16::from_le_bytes([low, high]);
        7
    }

    fn branch_to_subroutine<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        let offset = self.fetch(bus) as i8;
        bus.idle();
        let return_address = self.registers.pc.wrapping_sub(1);
        self.push(bus, (return_address >> 8) as u8);
        self.push(bus, return_address as u8);
        bus.idle();
        bus.idle();
        bus.idle();
        self.registers.pc = self.registers.pc.wrapping_add_signed(i16::from(offset));
        8
    }

    fn return_from_subroutine<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        self.dummy_fetch(bus);
        bus.idle();
        let low = self.pull(bus);
        let high = self.pull(bus);
        bus.idle();
        bus.idle();
        self.registers.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
        7
    }

    fn return_from_interrupt<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        self.dummy_fetch(bus);
        bus.idle();
        let status = self.pull(bus);
        let low = self.pull(bus);
        let high = self.pull(bus);
        bus.idle();
        self.restore_status(status);
        self.registers.pc = u16::from_le_bytes([low, high]);
        7
    }

    fn break_interrupt<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        self.fetch(bus);
        let return_address = self.registers.pc;
        self.push(bus, (return_address >> 8) as u8);
        self.push(bus, return_address as u8);
        self.push(bus, self.status_for_stack(true));
        self.registers.status.insert(StatusFlags::INTERRUPT);
        self.registers
            .status
            .remove(StatusFlags::DECIMAL | StatusFlags::BREAK);
        let low = self.read(bus, BRK_VECTOR_LOW);
        let high = self.read(bus, BRK_VECTOR_HIGH);
        bus.idle();
        self.registers.pc = u16::from_le_bytes([low, high]);
        8
    }

    pub(super) fn enter_hardware_interrupt_provisional<B: CpuBus>(
        &mut self,
        bus: &mut B,
        vector_low: u16,
    ) {
        self.dummy_fetch(bus);
        bus.dummy_read(self.logical_to_physical(self.registers.pc.wrapping_add(1)));
        let return_address = self.registers.pc;
        self.push(bus, (return_address >> 8) as u8);
        self.push(bus, return_address as u8);
        self.push(bus, self.status_for_stack(false));
        self.registers.status.insert(StatusFlags::INTERRUPT);
        self.registers
            .status
            .remove(StatusFlags::DECIMAL | StatusFlags::BREAK | StatusFlags::MEMORY_OPERATION);
        let low = self.read(bus, vector_low);
        let high = self.read(bus, vector_low.wrapping_add(1));
        bus.idle();
        self.registers.pc = u16::from_le_bytes([low, high]);
    }

    fn jump_absolute<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        let target = self.fetch_word(bus);
        bus.idle();
        self.registers.pc = target;
        4
    }

    fn jump_indirect<B: CpuBus>(&mut self, bus: &mut B, indexed: bool) -> u32 {
        let mut pointer = self.fetch_word(bus);
        if indexed {
            pointer = pointer.wrapping_add(u16::from(self.registers.x));
        }
        bus.idle();
        let low = self.read(bus, pointer);
        let high = self.read(bus, pointer.wrapping_add(1));
        bus.idle();
        self.registers.pc = u16::from_le_bytes([low, high]);
        7
    }

    fn branch<B: CpuBus>(&mut self, bus: &mut B, condition: bool) -> u32 {
        let offset = self.fetch(bus) as i8;
        if !condition {
            return 2;
        }
        self.dummy_fetch(bus);
        bus.idle();
        self.registers.pc = self.registers.pc.wrapping_add_signed(i16::from(offset));
        4
    }

    fn branch_always<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        let offset = self.fetch(bus) as i8;
        bus.idle();
        bus.idle();
        self.registers.pc = self.registers.pc.wrapping_add_signed(i16::from(offset));
        4
    }

    pub(super) fn push<B: CpuBus>(&mut self, bus: &mut B, value: u8) {
        self.write(bus, STACK_BASE | u16::from(self.registers.sp), value);
        self.registers.sp = self.registers.sp.wrapping_sub(1);
    }

    pub(super) fn pull<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        self.registers.sp = self.registers.sp.wrapping_add(1);
        self.read(bus, STACK_BASE | u16::from(self.registers.sp))
    }

    fn status_for_stack(&self, break_command: bool) -> u8 {
        let mut status = self.registers.status;
        status.set(StatusFlags::BREAK, break_command);
        status.bits()
    }

    fn restore_status(&mut self, value: u8) {
        self.registers.status = StatusFlags::from_bits_retain(value) & !StatusFlags::BREAK;
    }
}

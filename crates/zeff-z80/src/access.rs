use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fetch {
    Memory,
    Interrupt,
}

impl Fetch {
    pub(super) fn read<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> u8 {
        match self {
            Self::Memory => cpu.fetch_u8(bus),
            Self::Interrupt => bus.read_interrupt_instruction(cpu.regs.pc),
        }
    }
}

impl Cpu {
    pub(super) fn write_io<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        port: u8,
        value: u8,
        t_states_before: u32,
    ) {
        let cycle = IoWriteCycle {
            port,
            value,
            t_states_before,
            t_states: CYCLES_IO_MACHINE,
        };
        bus.io_write_cycle(cycle);
    }

    pub(super) fn immediate_io_t_states_before(&self) -> u32 {
        u32::from(self.last_m1_fetch_count) * CYCLES_M1 + CYCLES_IMMEDIATE_IO_OPERAND
    }

    pub(super) fn ed_io_t_states_before(&self) -> u32 {
        u32::from(self.last_m1_fetch_count) * CYCLES_M1
    }

    pub(super) fn conditional_relative_jump<B: Z80Bus>(&mut self, bus: &B, condition: u8) -> u32 {
        let displacement = self.fetch_u8(bus) as i8;
        if self.condition_is_true(condition) {
            self.regs.pc = self.regs.pc.wrapping_add_signed(i16::from(displacement));
            CYCLES_JR
        } else {
            CYCLES_JR_NOT_TAKEN
        }
    }

    pub(super) fn condition_is_true(&self, condition: u8) -> bool {
        match condition {
            CONDITION_NZ => self.regs.f & Z80_FLAG_ZERO == 0,
            CONDITION_Z => self.regs.f & Z80_FLAG_ZERO != 0,
            CONDITION_NC => self.regs.f & Z80_FLAG_CARRY == 0,
            CONDITION_C => self.regs.f & Z80_FLAG_CARRY != 0,
            CONDITION_PO => self.regs.f & Z80_FLAG_PARITY_OVERFLOW == 0,
            CONDITION_PE => self.regs.f & Z80_FLAG_PARITY_OVERFLOW != 0,
            CONDITION_P => self.regs.f & Z80_FLAG_SIGN == 0,
            CONDITION_M => self.regs.f & Z80_FLAG_SIGN != 0,
            _ => unreachable!("condition index is always three bits"),
        }
    }

    pub(super) fn read_reg16(&self, pair: u8) -> u16 {
        match pair {
            0 => self.regs.bc(),
            1 => self.regs.de(),
            2 => self.regs.hl(),
            3 => self.regs.sp,
            _ => unreachable!("16-bit register pair index is always two bits"),
        }
    }

    pub(super) fn write_reg16(&mut self, pair: u8, value: u16) {
        match pair {
            0 => self.regs.set_bc(value),
            1 => self.regs.set_de(value),
            2 => self.regs.set_hl(value),
            3 => self.regs.sp = value,
            _ => unreachable!("16-bit register pair index is always two bits"),
        }
    }

    pub(super) fn read_stack_reg16(&self, pair: u8) -> u16 {
        match pair {
            0 => self.regs.bc(),
            1 => self.regs.de(),
            2 => self.regs.hl(),
            3 => self.regs.af(),
            _ => unreachable!("stack register pair index is always two bits"),
        }
    }

    pub(super) fn write_stack_reg16(&mut self, pair: u8, value: u16) {
        match pair {
            0 => self.regs.set_bc(value),
            1 => self.regs.set_de(value),
            2 => self.regs.set_hl(value),
            3 => self.regs.set_af(value),
            _ => unreachable!("stack register pair index is always two bits"),
        }
    }

    pub(super) fn read_reg8<B: Z80Bus>(&self, bus: &B, register: u8) -> u8 {
        if register == REGISTER_MEMORY_INDEX {
            return bus.cpu_read(self.regs.hl());
        }
        self.read_register8(register)
    }

    pub(super) fn read_register8(&self, register: u8) -> u8 {
        match register {
            0 => self.regs.b,
            1 => self.regs.c,
            2 => self.regs.d,
            3 => self.regs.e,
            4 => self.regs.h,
            5 => self.regs.l,
            REGISTER_A_INDEX => self.regs.a,
            _ => unreachable!("register index excludes indirect memory"),
        }
    }

    pub(super) fn write_reg8<B: Z80Bus>(&mut self, bus: &mut B, register: u8, value: u8) {
        match register {
            0 => self.regs.b = value,
            1 => self.regs.c = value,
            2 => self.regs.d = value,
            3 => self.regs.e = value,
            4 => self.regs.h = value,
            5 => self.regs.l = value,
            REGISTER_MEMORY_INDEX => bus.cpu_write(self.regs.hl(), value),
            REGISTER_A_INDEX => self.regs.a = value,
            _ => unreachable!("8-bit register index is always three bits"),
        }
    }

    pub(super) fn push_u16<B: Z80Bus>(&mut self, bus: &mut B, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        bus.cpu_write(self.regs.sp, hi);
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        bus.cpu_write(self.regs.sp, lo);
    }

    pub(super) fn fetch_u8<B: Z80Bus>(&mut self, bus: &B) -> u8 {
        let value = bus.cpu_read(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        if usize::from(self.instruction_byte_count) < self.instruction_bytes.len() {
            self.instruction_bytes[usize::from(self.instruction_byte_count)] = value;
            self.instruction_byte_count += 1;
        }
        value
    }

    pub(super) fn increment_refresh_register(&mut self) {
        self.last_m1_fetch_count = self.last_m1_fetch_count.saturating_add(1);
        let bit7 = self.regs.r & REFRESH_COUNTER_BIT_7_MASK;
        let low = self.regs.r.wrapping_add(1) & REFRESH_COUNTER_MASK;
        self.regs.r = bit7 | low;
    }
}

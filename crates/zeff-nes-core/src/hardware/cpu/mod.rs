mod addressing;
mod alu;
pub mod registers;

pub use registers::{Registers, StatusFlags};

use crate::hardware::bus::Bus;
use crate::hardware::constants::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Running,
    Halted,
    Suspended,
}

#[derive(Debug)]
pub struct Cpu {
    pub pc: u16,
    pub sp: u8,
    pub regs: Registers,
    pub state: CpuState,
    pub cycles: u64,
    pub last_step_cycles: u64,
    pub nmi_pending: bool,
    pub irq_line: bool,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            pc: 0,
            sp: 0xFD,
            regs: Registers::power_on(),
            state: CpuState::Running,
            cycles: 7,
            last_step_cycles: 0,
            nmi_pending: false,
            irq_line: false,
            last_opcode: 0,
            last_opcode_pc: 0,
        }
    }

    pub fn reset(&mut self, bus: &mut Bus) {
        let lo = bus.cpu_read(RESET_VECTOR_LO) as u16;
        let hi = bus.cpu_read(RESET_VECTOR_HI) as u16;
        self.pc = (hi << 8) | lo;
        self.sp = self.sp.wrapping_sub(3);
        self.regs.set_flag(StatusFlags::INTERRUPT, true);
        self.cycles = 7;
    }

    pub fn step(&mut self, bus: &mut Bus) -> u64 {
        if self.state != CpuState::Running {
            self.last_step_cycles = 1;
            self.cycles += 1;
            return 1;
        }

        if self.nmi_pending {
            self.nmi_pending = false;
            let cycles = self.service_nmi(bus);
            self.last_step_cycles = cycles;
            self.cycles += cycles;
            return cycles;
        }

        if self.irq_line && !self.regs.get_flag(StatusFlags::INTERRUPT) {
            let cycles = self.service_irq(bus);
            self.last_step_cycles = cycles;
            self.cycles += cycles;
            return cycles;
        }

        self.last_opcode_pc = self.pc;
        let opcode = self.fetch8(bus);
        self.last_opcode = opcode;
        let base_cycles = crate::hardware::opcodes::cycles::CYCLE_TABLE[opcode as usize] as u64;
        let extra = crate::hardware::opcodes::dispatch::execute_opcode(self, bus, opcode) as u64;
        let cycles = base_cycles + extra;
        self.last_step_cycles = cycles;
        self.cycles += cycles;
        cycles
    }

    pub(crate) fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let v = bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    pub(crate) fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }

    pub(crate) fn push8(&mut self, bus: &mut Bus, val: u8) {
        bus.cpu_write(STACK_BASE | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    pub(crate) fn pop8(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.cpu_read(STACK_BASE | self.sp as u16)
    }

    pub(crate) fn push16(&mut self, bus: &mut Bus, val: u16) {
        self.push8(bus, (val >> 8) as u8);
        self.push8(bus, val as u8);
    }

    pub(crate) fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.pop8(bus) as u16;
        let hi = self.pop8(bus) as u16;
        (hi << 8) | lo
    }

    fn service_nmi(&mut self, bus: &mut Bus) -> u64 {
        self.push16(bus, self.pc);
        self.push8(bus, self.regs.status_for_push(false));
        self.regs.set_flag(StatusFlags::INTERRUPT, true);
        let lo = bus.cpu_read(NMI_VECTOR_LO) as u16;
        let hi = bus.cpu_read(NMI_VECTOR_HI) as u16;
        self.pc = (hi << 8) | lo;
        7
    }

    fn service_irq(&mut self, bus: &mut Bus) -> u64 {
        self.push16(bus, self.pc);
        self.push8(bus, self.regs.status_for_push(false));
        self.regs.set_flag(StatusFlags::INTERRUPT, true);
        let lo = bus.cpu_read(IRQ_VECTOR_LO) as u16;
        let hi = bus.cpu_read(IRQ_VECTOR_HI) as u16;
        self.pc = (hi << 8) | lo;
        7
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u16(self.pc);
        w.write_u8(self.sp);
        w.write_u8(self.regs.a);
        w.write_u8(self.regs.x);
        w.write_u8(self.regs.y);
        w.write_u8(self.regs.p.bits());
        w.write_u8(match self.state {
            CpuState::Running => 0,
            CpuState::Halted => 1,
            CpuState::Suspended => 2,
        });
        w.write_u64(self.cycles);
        w.write_u64(self.last_step_cycles);
        w.write_bool(self.nmi_pending);
        w.write_bool(self.irq_line);
        w.write_u8(self.last_opcode);
        w.write_u16(self.last_opcode_pc);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.pc = r.read_u16()?;
        self.sp = r.read_u8()?;
        self.regs.a = r.read_u8()?;
        self.regs.x = r.read_u8()?;
        self.regs.y = r.read_u8()?;
        self.regs.p = StatusFlags::from_bits_truncate(r.read_u8()?);
        self.state = match r.read_u8()? {
            0 => CpuState::Running,
            1 => CpuState::Halted,
            2 => CpuState::Suspended,
            other => anyhow::bail!("invalid CPU state tag: {other}"),
        };
        self.cycles = r.read_u64()?;
        self.last_step_cycles = r.read_u64()?;
        self.nmi_pending = r.read_bool()?;
        self.irq_line = r.read_bool()?;
        self.last_opcode = r.read_u8()?;
        self.last_opcode_pc = r.read_u16()?;
        Ok(())
    }
}

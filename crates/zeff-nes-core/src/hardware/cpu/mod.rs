mod addressing;
mod alu;
pub mod registers;

pub use registers::{Registers, StatusFlags};

use crate::hardware::bus::Bus;
use crate::hardware::constants::*;

/// Bus operations required by the 2A03 instruction engine.
pub trait CpuBus {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8);
    fn cpu_read_after_elapsed_cycles(&mut self, addr: u16, elapsed_cycles: u64) -> u8;
    fn cpu_write_after_elapsed_cycles(&mut self, addr: u16, value: u8, elapsed_cycles: u64);
    fn prepare_cpu_instruction_accesses(&mut self);
    fn finish_cpu_instruction_accesses(&mut self, total_cycles: u64, pc: u16);
    fn take_nmi_edge_for_vector(&mut self) -> bool {
        false
    }
}

impl CpuBus for Bus {
    #[inline]
    fn cpu_read(&mut self, addr: u16) -> u8 {
        Bus::cpu_read_timed(self, addr)
    }

    #[inline]
    fn cpu_write(&mut self, addr: u16, value: u8) {
        Bus::cpu_write_timed(self, addr, value);
    }

    #[inline]
    fn cpu_read_after_elapsed_cycles(&mut self, addr: u16, elapsed_cycles: u64) -> u8 {
        Bus::cpu_read_after_elapsed_cycles(self, addr, elapsed_cycles)
    }

    #[inline]
    fn cpu_write_after_elapsed_cycles(&mut self, addr: u16, value: u8, elapsed_cycles: u64) {
        Bus::cpu_write_after_elapsed_cycles(self, addr, value, elapsed_cycles);
    }

    #[inline]
    fn prepare_cpu_instruction_accesses(&mut self) {
        Bus::prepare_cpu_instruction_accesses(self);
    }

    #[inline]
    fn finish_cpu_instruction_accesses(&mut self, total_cycles: u64, pc: u16) {
        Bus::finish_cpu_instruction_accesses(self, total_cycles, pc);
    }

    #[inline]
    fn take_nmi_edge_for_vector(&mut self) -> bool {
        Bus::take_nmi_edge_for_vector(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Running,
    Halted,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuStepKind {
    Instruction,
    Nmi,
    Irq,
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JamPhase {
    FirstHigh,
    FirstLow,
    SecondLow,
    StableHigh,
}

impl JamPhase {
    fn address_and_advance(&mut self) -> u16 {
        match *self {
            Self::FirstHigh => {
                *self = Self::FirstLow;
                0xFFFF
            }
            Self::FirstLow => {
                *self = Self::SecondLow;
                0xFFFE
            }
            Self::SecondLow => {
                *self = Self::StableHigh;
                0xFFFE
            }
            Self::StableHigh => 0xFFFF,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::FirstHigh => 1,
            Self::FirstLow => 2,
            Self::SecondLow => 3,
            Self::StableHigh => 4,
        }
    }

    fn from_tag(tag: u8) -> anyhow::Result<Option<Self>> {
        match tag {
            0 => Ok(None),
            1 => Ok(Some(Self::FirstHigh)),
            2 => Ok(Some(Self::FirstLow)),
            3 => Ok(Some(Self::SecondLow)),
            4 => Ok(Some(Self::StableHigh)),
            other => anyhow::bail!("invalid CPU JAM phase tag: {other}"),
        }
    }
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
    nmi_poll_delay: u8,
    irq_inhibit_delay: u8,
    irq_inhibit_before_delay: bool,
    irq_poll_delay: u8,
    pub last_step_kind: CpuStepKind,
    pub last_step_branch_taken_same_page: bool,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
    jam_phase: Option<JamPhase>,
    instruction_bytes: [u8; 3],
    instruction_byte_count: u8,
    pub nmi_count: u64,
    pub irq_count: u64,
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
            nmi_poll_delay: 0,
            irq_inhibit_delay: 0,
            irq_inhibit_before_delay: false,
            irq_poll_delay: 0,
            last_step_kind: CpuStepKind::Idle,
            last_step_branch_taken_same_page: false,
            last_opcode: 0,
            last_opcode_pc: 0,
            jam_phase: None,
            instruction_bytes: [0; 3],
            instruction_byte_count: 0,
            nmi_count: 0,
            irq_count: 0,
        }
    }

    pub fn power_on(&mut self, bus: &mut Bus) {
        self.load_reset_vector(bus);
        self.finish_reset();
    }

    pub fn reset(&mut self, bus: &mut Bus) {
        self.load_reset_vector(bus);
        self.sp = self.sp.wrapping_sub(3);
        self.finish_reset();
    }

    fn load_reset_vector(&mut self, bus: &mut Bus) {
        let lo = bus.cpu_read(RESET_VECTOR_LO) as u16;
        let hi = bus.cpu_read(RESET_VECTOR_HI) as u16;
        self.pc = (hi << 8) | lo;
    }

    fn finish_reset(&mut self) {
        self.regs.set_flag(StatusFlags::INTERRUPT, true);
        self.state = CpuState::Running;
        self.cycles = 7;
        self.last_step_cycles = 0;
        self.nmi_pending = false;
        self.irq_line = false;
        self.nmi_poll_delay = 0;
        self.irq_inhibit_delay = 0;
        self.irq_inhibit_before_delay = self.regs.get_flag(StatusFlags::INTERRUPT);
        self.irq_poll_delay = 0;
        self.last_step_kind = CpuStepKind::Idle;
        self.last_step_branch_taken_same_page = false;
        self.last_opcode = 0;
        self.last_opcode_pc = self.pc;
        self.jam_phase = None;
        self.instruction_bytes = [0; 3];
        self.instruction_byte_count = 0;
    }

    #[inline]
    pub fn step(&mut self, bus: &mut Bus) -> u64 {
        if bus.cpu_step_start_tick.is_some() {
            return self.step_with_bus(bus);
        }

        bus.cpu_odd_cycle = self.cycles & 1 != 0;
        bus.begin_cpu_step_timing(zeff_emu_common::time::MasterTicks::new(self.cycles));
        let cycles = self.step_with_bus(bus);
        let dma_cycles = bus.dma_stall_cycles;
        bus.dma_stall_cycles = 0;
        self.cycles += dma_cycles;
        let total_cycles = cycles + dma_cycles;
        let _ = bus.finish_cpu_step_timing(total_cycles);
        total_cycles
    }

    pub(crate) fn step_with_bus<B: CpuBus>(&mut self, bus: &mut B) -> u64 {
        self.instruction_bytes = [0; 3];
        self.instruction_byte_count = 0;
        bus.prepare_cpu_instruction_accesses();
        self.last_step_kind = CpuStepKind::Idle;
        self.last_step_branch_taken_same_page = false;

        if self.state != CpuState::Running {
            let address = if self.state == CpuState::Halted {
                self.jam_phase
                    .get_or_insert(JamPhase::StableHigh)
                    .address_and_advance()
            } else {
                self.pc
            };
            bus.finish_cpu_instruction_accesses(1, address);
            self.last_step_cycles = 1;
            self.cycles += 1;
            return 1;
        }

        if self.nmi_pending && self.nmi_poll_delay == 0 {
            self.nmi_pending = false;
            let cycles = self.service_nmi(bus);
            bus.finish_cpu_instruction_accesses(cycles, self.pc);
            self.last_step_kind = CpuStepKind::Nmi;
            self.last_step_cycles = cycles;
            self.cycles += cycles;
            return cycles;
        }

        if self.irq_line && !self.irq_inhibited() {
            let cycles = self.service_irq(bus);
            bus.finish_cpu_instruction_accesses(cycles, self.pc);
            self.last_step_kind = CpuStepKind::Irq;
            self.last_step_cycles = cycles;
            self.cycles += cycles;
            return cycles;
        }

        self.last_step_kind = CpuStepKind::Instruction;
        self.last_opcode_pc = self.pc;
        let opcode = self.fetch8(bus);
        self.last_opcode = opcode;
        let base_cycles = crate::hardware::opcodes::cycles::CYCLE_TABLE[opcode as usize] as u64;
        let extra = crate::hardware::opcodes::dispatch::execute_opcode(self, bus, opcode) as u64;
        let cycles = base_cycles + extra;
        bus.finish_cpu_instruction_accesses(cycles, self.pc);
        self.tick_irq_delays();
        self.last_step_cycles = cycles;
        self.cycles += cycles;
        cycles
    }

    fn irq_inhibited(&self) -> bool {
        if self.irq_poll_delay > 0 {
            return true;
        }
        if self.irq_inhibit_delay > 0 {
            self.irq_inhibit_before_delay
        } else {
            self.regs.get_flag(StatusFlags::INTERRUPT)
        }
    }

    pub(crate) fn delay_irq_inhibit_change(&mut self) {
        self.irq_inhibit_before_delay = self.regs.get_flag(StatusFlags::INTERRUPT);
        self.irq_inhibit_delay = 2;
    }

    pub(crate) fn clear_irq_inhibit_delay(&mut self) {
        self.irq_inhibit_delay = 0;
        self.irq_inhibit_before_delay = self.regs.get_flag(StatusFlags::INTERRUPT);
    }

    pub(crate) fn delay_irq_poll_once(&mut self) {
        self.irq_poll_delay = 1;
    }

    pub(crate) fn delay_nmi_poll_once(&mut self) {
        self.nmi_poll_delay = 1;
    }

    pub(crate) fn enter_jam(&mut self) {
        self.jam_phase = Some(JamPhase::FirstHigh);
        self.state = CpuState::Halted;
    }

    pub(crate) fn is_jammed(&self) -> bool {
        self.jam_phase.is_some()
    }

    pub(crate) fn resume_from_debug(&mut self) {
        self.state = if self.is_jammed() {
            CpuState::Halted
        } else {
            CpuState::Running
        };
    }

    pub(crate) fn mark_branch_taken_same_page(&mut self) {
        self.last_step_branch_taken_same_page = true;
    }

    fn tick_irq_delays(&mut self) {
        self.nmi_poll_delay = self.nmi_poll_delay.saturating_sub(1);
        self.irq_inhibit_delay = self.irq_inhibit_delay.saturating_sub(1);
        self.irq_poll_delay = self.irq_poll_delay.saturating_sub(1);
    }

    pub(crate) fn fetch8<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        let v = bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        if usize::from(self.instruction_byte_count) < self.instruction_bytes.len() {
            self.instruction_bytes[usize::from(self.instruction_byte_count)] = v;
            self.instruction_byte_count += 1;
        }
        v
    }

    pub(crate) fn instruction_bytes(&self) -> &[u8] {
        &self.instruction_bytes[..usize::from(self.instruction_byte_count)]
    }

    pub(crate) fn fetch16<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }

    pub(crate) fn push8<B: CpuBus>(&mut self, bus: &mut B, val: u8) {
        bus.cpu_write(STACK_BASE | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    pub(crate) fn pop8<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.cpu_read(STACK_BASE | self.sp as u16)
    }

    pub(crate) fn push16<B: CpuBus>(&mut self, bus: &mut B, val: u16) {
        self.push8(bus, (val >> 8) as u8);
        self.push8(bus, val as u8);
    }

    pub(crate) fn pop16<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.pop8(bus) as u16;
        let hi = self.pop8(bus) as u16;
        (hi << 8) | lo
    }

    fn service_nmi<B: CpuBus>(&mut self, bus: &mut B) -> u64 {
        self.nmi_count = self.nmi_count.wrapping_add(1);
        let _ = bus.cpu_read(self.pc);
        let _ = bus.cpu_read(self.pc);
        self.push16(bus, self.pc);
        self.push8(bus, self.regs.status_for_push(false));
        self.regs.set_flag(StatusFlags::INTERRUPT, true);
        self.clear_irq_inhibit_delay();
        let lo = bus.cpu_read(NMI_VECTOR_LO) as u16;
        let hi = bus.cpu_read(NMI_VECTOR_HI) as u16;
        self.pc = (hi << 8) | lo;
        7
    }

    fn service_irq<B: CpuBus>(&mut self, bus: &mut B) -> u64 {
        self.irq_count = self.irq_count.wrapping_add(1);
        let _ = bus.cpu_read(self.pc);
        let _ = bus.cpu_read(self.pc);
        self.push16(bus, self.pc);
        let vector_edge = bus.take_nmi_edge_for_vector();
        let nmi_hijacked = self.nmi_pending || vector_edge;
        self.push8(bus, self.regs.status_for_push(false));
        self.regs.set_flag(StatusFlags::INTERRUPT, true);
        self.clear_irq_inhibit_delay();
        let (vector_lo, vector_hi) = if nmi_hijacked {
            self.nmi_pending = false;
            self.nmi_poll_delay = 0;
            self.nmi_count = self.nmi_count.wrapping_add(1);
            (NMI_VECTOR_LO, NMI_VECTOR_HI)
        } else {
            (IRQ_VECTOR_LO, IRQ_VECTOR_HI)
        };
        let lo = bus.cpu_read(vector_lo) as u16;
        let hi = bus.cpu_read(vector_hi) as u16;
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
        self.nmi_poll_delay = 0;
        self.clear_irq_inhibit_delay();
        self.irq_poll_delay = 0;
        self.last_step_kind = CpuStepKind::Idle;
        self.last_step_branch_taken_same_page = false;
        self.last_opcode = r.read_u8()?;
        self.last_opcode_pc = r.read_u16()?;
        self.jam_phase = (self.state == CpuState::Halted).then_some(JamPhase::StableHigh);
        self.instruction_bytes = [0; 3];
        self.instruction_byte_count = 0;
        self.nmi_count = 0;
        self.irq_count = 0;
        Ok(())
    }

    pub(crate) fn write_jam_state(&self, w: &mut crate::save_state::StateWriter) {
        let phase = self
            .jam_phase
            .or((self.state == CpuState::Halted).then_some(JamPhase::StableHigh));
        w.write_u8(phase.map_or(0, JamPhase::tag));
    }

    pub(crate) fn read_jam_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        let phase = JamPhase::from_tag(r.read_u8()?)?;
        if phase.is_some() && self.state == CpuState::Running {
            anyhow::bail!("running CPU cannot carry JAM phase state");
        }
        if phase.is_none() && self.state == CpuState::Halted {
            anyhow::bail!("halted CPU is missing JAM phase state");
        }
        self.jam_phase = phase;
        Ok(())
    }
}

impl<B: CpuBus> zeff_emu_common::cpu::CpuCore<B> for Cpu {
    type Step = u64;

    #[inline]
    fn step_cpu(&mut self, bus: &mut B) -> Self::Step {
        self.step_with_bus(bus)
    }
}

#[cfg(test)]
mod alu_proptests;
#[cfg(test)]
mod conformance_tests;

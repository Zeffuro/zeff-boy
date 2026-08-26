mod alu;
mod bitops;
mod registers;

pub use registers::Registers;

use crate::hardware::bus::{Bus, OamCorruptionType};
use crate::hardware::opcodes::cycles::CYCLE_TABLE;
use crate::hardware::opcodes::dispatch::execute_opcode;
use crate::hardware::types::CpuState;
use crate::hardware::types::ImeState;
use crate::hardware::types::constants::*;
#[cfg(test)]
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GbCpuTiming {
    pub cpu_t_cycles: u64,
    pub master_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GbCpuRead {
    pub value: u8,
    pub timing: GbCpuTiming,
}

pub trait GbCpuBus {
    fn cpu_read_byte_timed(&mut self, addr: u16, access_start_master_ticks: u64) -> GbCpuRead;
    fn cpu_write_byte_timed(
        &mut self,
        addr: u16,
        value: u8,
        access_start_master_ticks: u64,
    ) -> GbCpuTiming;
    fn advance_cpu_t_cycles(&mut self, cpu_t_cycles: u64) -> GbCpuTiming;
    fn advance_stopped_t_cycles(&mut self, cpu_t_cycles: u64) -> GbCpuTiming {
        self.advance_cpu_t_cycles(cpu_t_cycles)
    }
    fn enter_stop_mode(&mut self) {}
    fn pending_interrupts_for_cpu(&self) -> u8;
    fn pending_interrupts_for_halt(&self) -> u8;
    fn clear_interrupt_bit(&mut self, bit: usize);
    fn maybe_trigger_oam_write_corruption(&mut self, addr: u16);
    fn try_cgb_speed_switch(&mut self) -> Option<GbCpuTiming>;
}

impl GbCpuBus for Bus {
    #[inline]
    fn cpu_read_byte_timed(&mut self, addr: u16, access_start_master_ticks: u64) -> GbCpuRead {
        let blocked_by_oam_dma = Bus::oam_dma_blocks_cpu_access(self, addr);
        let master_ticks = Bus::advance_cpu_t_cycles(self, 4);
        let value = Bus::cpu_read_byte_after_oam_dma_check(
            self,
            addr,
            blocked_by_oam_dma,
            access_start_master_ticks.wrapping_add(master_ticks),
        );
        GbCpuRead {
            value,
            timing: GbCpuTiming {
                cpu_t_cycles: 4,
                master_ticks,
            },
        }
    }

    #[inline]
    fn cpu_write_byte_timed(
        &mut self,
        addr: u16,
        value: u8,
        access_start_master_ticks: u64,
    ) -> GbCpuTiming {
        let blocked_by_oam_dma = Bus::oam_dma_blocks_cpu_access(self, addr);
        let oam_accessible_at_access = (OAM_START..=OAM_END)
            .contains(&addr)
            .then(|| Bus::cpu_oam_write_accessible(self));
        let access_master_ticks = Bus::advance_cpu_t_cycles(self, 4);
        let extra_t_cycles = Bus::cpu_write_byte_after_oam_dma_and_oam_access_check(
            self,
            addr,
            value,
            blocked_by_oam_dma,
            oam_accessible_at_access,
            access_start_master_ticks.wrapping_add(access_master_ticks),
        );
        let extra_master_ticks = if extra_t_cycles == 0 {
            0
        } else {
            Bus::advance_cpu_t_cycles(self, extra_t_cycles)
        };
        GbCpuTiming {
            cpu_t_cycles: 4_u64.wrapping_add(extra_t_cycles),
            master_ticks: access_master_ticks.wrapping_add(extra_master_ticks),
        }
    }

    #[inline]
    fn advance_cpu_t_cycles(&mut self, cpu_t_cycles: u64) -> GbCpuTiming {
        GbCpuTiming {
            cpu_t_cycles,
            master_ticks: Bus::advance_cpu_t_cycles(self, cpu_t_cycles),
        }
    }

    #[inline]
    fn advance_stopped_t_cycles(&mut self, cpu_t_cycles: u64) -> GbCpuTiming {
        GbCpuTiming {
            cpu_t_cycles,
            master_ticks: Bus::advance_stopped_t_cycles(self, cpu_t_cycles),
        }
    }

    #[inline]
    fn enter_stop_mode(&mut self) {
        Bus::enter_stop_mode(self);
    }

    #[inline]
    fn pending_interrupts_for_cpu(&self) -> u8 {
        Bus::pending_interrupts_for_cpu(self)
    }

    #[inline]
    fn pending_interrupts_for_halt(&self) -> u8 {
        Bus::pending_interrupts_for_halt(self)
    }

    #[inline]
    fn clear_interrupt_bit(&mut self, bit: usize) {
        Bus::clear_interrupt_bit(self, bit);
    }

    #[inline]
    fn maybe_trigger_oam_write_corruption(&mut self, addr: u16) {
        Bus::maybe_trigger_oam_corruption(self, addr, OamCorruptionType::Write);
    }

    fn try_cgb_speed_switch(&mut self) -> Option<GbCpuTiming> {
        Bus::maybe_switch_cgb_speed(self).then(|| {
            let (cpu_t_cycles, master_ticks) = Bus::advance_cgb_speed_switch_delay(self);
            GbCpuTiming {
                cpu_t_cycles,
                master_ticks,
            }
        })
    }
}

#[derive(Debug)]
pub struct Cpu {
    pub pc: u16,
    pub sp: u16,
    pub regs: Registers,
    pub ime: ImeState,
    pub running: CpuState,
    pub cycles: u64,
    pub last_step_cycles: u64,
    pub timed_cycles_accounted: u64,
    pub last_step_master_ticks: u64,
    pub timed_master_ticks_accounted: u64,
    pub halt_bug_active: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            pc: 0x100,
            sp: 0xFFFE,
            regs: Registers::default(),
            ime: ImeState::Disabled,
            running: CpuState::Running,
            cycles: 0,
            last_step_cycles: 0,
            timed_cycles_accounted: 0,
            last_step_master_ticks: 0,
            timed_master_ticks_accounted: 0,
            halt_bug_active: false,
        }
    }

    #[inline]
    pub fn step(&mut self, bus: &mut Bus) {
        self.step_with_bus(bus);
    }

    pub fn step_with_bus(&mut self, bus: &mut impl GbCpuBus) {
        self.timed_cycles_accounted = 0;
        self.timed_master_ticks_accounted = 0;

        if self.running == CpuState::Stopped {
            self.account_timing(bus.advance_stopped_t_cycles(4));
            self.commit_step_cycles();
            return;
        }

        if self.running == CpuState::Halted {
            let pending = bus.pending_interrupts_for_halt();
            if pending == 0 {
                self.tick_internal_timed(bus, 4);
                self.commit_step_cycles();
                return;
            }

            self.running = CpuState::Running;
            if self.ime == ImeState::Enabled && self.handle_interrupts(bus) {
                self.commit_step_cycles();
                return;
            }
        } else if self.ime == ImeState::Enabled
            && bus.pending_interrupts_for_cpu() != 0
            && self.handle_interrupts(bus)
        {
            self.commit_step_cycles();
            return;
        }

        let ime_was_pending_enable = matches!(self.ime, ImeState::PendingEnable);
        let opcode = self.fetch8_timed(bus);
        execute_opcode(self, bus, opcode);

        let expected_cycles = CYCLE_TABLE[opcode as usize] as u64;
        if self.timed_cycles_accounted < expected_cycles {
            self.tick_internal_timed(bus, expected_cycles - self.timed_cycles_accounted);
        }

        self.commit_step_cycles();

        if ime_was_pending_enable && matches!(self.ime, ImeState::PendingEnable) {
            self.ime = ImeState::Enabled;
        }
    }

    fn commit_step_cycles(&mut self) {
        self.last_step_cycles = self.timed_cycles_accounted;
        self.last_step_master_ticks = self.timed_master_ticks_accounted;
        self.cycles += self.last_step_cycles;
    }

    pub fn handle_interrupts(&mut self, bus: &mut impl GbCpuBus) -> bool {
        let triggered = bus.pending_interrupts_for_cpu();
        if triggered == 0 || self.ime != ImeState::Enabled {
            return false;
        }

        const INT_VECTORS: [u16; 5] = [INT_VBLANK, INT_STAT, INT_TIMER, INT_SERIAL, INT_JOYPAD];

        self.ime = ImeState::Disabled;

        self.tick_internal_timed(bus, 8);
        let return_addr = self.pc;
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (return_addr >> 8) as u8);

        let triggered_after_high_push = bus.pending_interrupts_for_cpu();
        let dispatch_bit = (triggered_after_high_push != 0).then(|| {
            let bit = triggered_after_high_push.trailing_zeros() as usize;
            bus.clear_interrupt_bit(bit);
            bit
        });

        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (return_addr & 0xFF) as u8);
        self.tick_internal_timed(bus, 4);
        self.pc = dispatch_bit.map_or(0x0000, |bit| INT_VECTORS[bit]);

        true
    }

    #[inline]
    pub fn fetch8_timed(&mut self, bus: &mut impl GbCpuBus) -> u8 {
        let val = self.bus_read_timed(bus, self.pc);
        self.advance_pc_after_fetch();
        val
    }

    pub fn fetch16_timed(&mut self, bus: &mut impl GbCpuBus) -> u16 {
        let low = self.fetch8_timed(bus) as u16;
        let high = self.fetch8_timed(bus) as u16;
        low | (high << 8)
    }

    pub fn push16_timed(&mut self, bus: &mut impl GbCpuBus, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value & 0xFF) as u8);
    }

    pub fn pop16_timed(&mut self, bus: &mut impl GbCpuBus) -> u16 {
        let low = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let high = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (high << 8) | low
    }

    pub fn push16_timed_oam(&mut self, bus: &mut impl GbCpuBus, value: u16) {
        bus.maybe_trigger_oam_write_corruption(self.sp);
        self.push16_timed(bus, value);
    }

    pub fn pop16_timed_oam(&mut self, bus: &mut impl GbCpuBus) -> u16 {
        self.pop16_timed(bus)
    }

    pub fn jump(&mut self, addr: u16) {
        self.pc = addr;
    }

    pub fn jump_relative(&mut self, offset: i8) {
        self.pc = self.pc.wrapping_add_signed(offset as i16);
    }

    #[inline]
    pub fn bus_read_timed(&mut self, bus: &mut impl GbCpuBus, addr: u16) -> u8 {
        let read = bus.cpu_read_byte_timed(addr, self.timed_master_ticks_accounted);
        self.account_timing(read.timing);
        read.value
    }

    #[inline]
    pub fn bus_write_timed(&mut self, bus: &mut impl GbCpuBus, addr: u16, value: u8) {
        let timing = bus.cpu_write_byte_timed(addr, value, self.timed_master_ticks_accounted);
        self.account_timing(timing);
    }

    #[inline]
    pub fn tick_internal_timed(&mut self, bus: &mut impl GbCpuBus, t_cycles: u64) {
        let timing = bus.advance_cpu_t_cycles(t_cycles);
        self.account_timing(timing);
    }

    pub fn trigger_halt_bug(&mut self) {
        self.halt_bug_active = true;
    }

    #[inline]
    pub fn inc_rp_timed(&mut self, bus: &mut impl GbCpuBus, value: u16) -> u16 {
        self.tick_internal_timed(bus, 4);
        bus.maybe_trigger_oam_write_corruption(value);
        value.wrapping_add(1)
    }

    #[inline]
    pub fn dec_rp_timed(&mut self, bus: &mut impl GbCpuBus, value: u16) -> u16 {
        self.tick_internal_timed(bus, 4);
        bus.maybe_trigger_oam_write_corruption(value);
        value.wrapping_sub(1)
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u16(self.pc);
        writer.write_u16(self.sp);
        writer.write_u8(self.regs.a);
        writer.write_u8(self.regs.f);
        writer.write_u8(self.regs.b);
        writer.write_u8(self.regs.c);
        writer.write_u8(self.regs.d);
        writer.write_u8(self.regs.e);
        writer.write_u8(self.regs.h);
        writer.write_u8(self.regs.l);
        writer.write_u8(encode_ime_state(self.ime));
        writer.write_u8(encode_cpu_state(self.running));
        writer.write_u64(self.cycles);
        writer.write_u64(self.last_step_cycles);
        writer.write_u64(self.timed_cycles_accounted);
        writer.write_bool(self.halt_bug_active);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            pc: reader.read_u16()?,
            sp: reader.read_u16()?,
            regs: Registers {
                a: reader.read_u8()?,
                f: reader.read_u8()?,
                b: reader.read_u8()?,
                c: reader.read_u8()?,
                d: reader.read_u8()?,
                e: reader.read_u8()?,
                h: reader.read_u8()?,
                l: reader.read_u8()?,
            },
            ime: decode_ime_state(reader.read_u8()?)?,
            running: decode_cpu_state(reader.read_u8()?)?,
            cycles: reader.read_u64()?,
            last_step_cycles: reader.read_u64()?,
            timed_cycles_accounted: reader.read_u64()?,
            last_step_master_ticks: 0,
            timed_master_ticks_accounted: 0,
            halt_bug_active: reader.read_bool()?,
        })
    }

    #[inline]
    pub(in crate::hardware) fn account_timing(&mut self, timing: GbCpuTiming) {
        self.timed_cycles_accounted = self
            .timed_cycles_accounted
            .wrapping_add(timing.cpu_t_cycles);
        self.timed_master_ticks_accounted = self
            .timed_master_ticks_accounted
            .wrapping_add(timing.master_ticks);
    }

    fn advance_pc_after_fetch(&mut self) {
        if self.halt_bug_active {
            self.halt_bug_active = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
    }
}

impl<B: GbCpuBus> zeff_emu_common::cpu::CpuCore<B> for Cpu {
    type Step = u64;

    #[inline]
    fn step_cpu(&mut self, bus: &mut B) -> Self::Step {
        self.step_with_bus(bus);
        self.last_step_cycles
    }
}

fn encode_ime_state(state: ImeState) -> u8 {
    match state {
        ImeState::Enabled => 0,
        ImeState::Disabled => 1,
        ImeState::PendingEnable => 2,
    }
}

fn decode_ime_state(tag: u8) -> Result<ImeState> {
    match tag {
        0 => Ok(ImeState::Enabled),
        1 => Ok(ImeState::Disabled),
        2 => Ok(ImeState::PendingEnable),
        _ => bail!("invalid IME state tag in save-state file: {tag}"),
    }
}

fn encode_cpu_state(state: CpuState) -> u8 {
    match state {
        CpuState::Running => 0,
        CpuState::Halted => 1,
        CpuState::Stopped => 2,
        CpuState::InterruptHandling => 3,
        CpuState::Reset => 4,
        CpuState::Suspended => 5,
    }
}

fn decode_cpu_state(tag: u8) -> Result<CpuState> {
    match tag {
        0 => Ok(CpuState::Running),
        1 => Ok(CpuState::Halted),
        2 => Ok(CpuState::Stopped),
        3 => Ok(CpuState::InterruptHandling),
        4 => Ok(CpuState::Reset),
        5 => Ok(CpuState::Suspended),
        _ => bail!("invalid CPU state tag in save-state file: {tag}"),
    }
}

#[cfg(test)]
mod alu_proptests;
#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod tests;

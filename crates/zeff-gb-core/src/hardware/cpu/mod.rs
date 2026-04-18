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
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

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
            halt_bug_active: false,
        }
    }

    #[inline]
    pub fn step(&mut self, bus: &mut Bus) {
        self.timed_cycles_accounted = 0;
        let pending = bus.if_reg & bus.ie & 0x1F;

        if self.running == CpuState::Halted {
            if pending == 0 {
                self.tick_internal_timed(bus, 4);
                self.commit_step_cycles();
                return;
            }

            self.running = CpuState::Running;
            if self.ime == ImeState::Enabled {
                self.tick_internal_timed(bus, 4);
                if self.handle_interrupts(bus) {
                    self.commit_step_cycles();
                    return;
                }
            } else {
                self.tick_internal_timed(bus, 4);
            }
        } else if self.ime == ImeState::Enabled && pending != 0 && self.handle_interrupts(bus) {
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
        self.cycles += self.last_step_cycles;
    }

    pub fn handle_interrupts(&mut self, bus: &mut Bus) -> bool {
        let triggered = bus.if_reg & bus.ie;
        if triggered == 0 || self.ime != ImeState::Enabled {
            return false;
        }

        const INT_VECTORS: [u16; 5] = [INT_VBLANK, INT_STAT, INT_TIMER, INT_SERIAL, INT_JOYPAD];

        let bit = (triggered & 0x1F).trailing_zeros() as usize;
        if bit >= 5 {
            return false;
        }

        bus.if_reg &= !(1 << bit);
        self.ime = ImeState::Disabled;

        self.tick_internal_timed(bus, 8);
        self.push16_timed(bus, self.pc);
        self.tick_internal_timed(bus, 4);
        self.pc = INT_VECTORS[bit];

        true
    }

    #[inline]
    pub fn fetch8_timed(&mut self, bus: &mut Bus) -> u8 {
        let val = self.bus_read_timed(bus, self.pc);
        self.advance_pc_after_fetch();
        val
    }

    pub fn fetch16_timed(&mut self, bus: &mut Bus) -> u16 {
        let low = self.fetch8_timed(bus) as u16;
        let high = self.fetch8_timed(bus) as u16;
        low | (high << 8)
    }

    pub fn push16_timed(&mut self, bus: &mut Bus, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value & 0xFF) as u8);
    }

    pub fn pop16_timed(&mut self, bus: &mut Bus) -> u16 {
        let low = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let high = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (high << 8) | low
    }

    pub fn push16_timed_oam(&mut self, bus: &mut Bus, value: u16) {
        bus.maybe_trigger_oam_corruption(self.sp, OamCorruptionType::Write);
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value >> 8) as u8);
        bus.maybe_trigger_oam_corruption(self.sp, OamCorruptionType::Write);
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value & 0xFF) as u8);
    }

    pub fn pop16_timed_oam(&mut self, bus: &mut Bus) -> u16 {
        bus.maybe_trigger_oam_corruption(self.sp, OamCorruptionType::Read);
        let low = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        bus.maybe_trigger_oam_corruption(self.sp, OamCorruptionType::Read);
        let high = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (high << 8) | low
    }

    pub fn jump(&mut self, addr: u16) {
        self.pc = addr;
    }

    pub fn jump_relative(&mut self, offset: i8) {
        self.pc = self.pc.wrapping_add_signed(offset as i16);
    }

    #[inline]
    pub fn bus_read_timed(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        self.tick_peripherals(bus, 4);
        bus.cpu_read_byte(addr)
    }

    #[inline]
    pub fn bus_write_timed(&mut self, bus: &mut Bus, addr: u16, value: u8) {
        self.tick_peripherals(bus, 4);
        let extra_t_cycles = bus.cpu_write_byte(addr, value);
        if extra_t_cycles != 0 {
            self.tick_peripherals(bus, extra_t_cycles);
        }
    }

    pub fn tick_internal_timed(&mut self, bus: &mut Bus, t_cycles: u64) {
        self.tick_peripherals(bus, t_cycles);
    }

    pub fn trigger_halt_bug(&mut self) {
        self.halt_bug_active = true;
    }

    #[inline]
    pub fn inc_rp_timed(&mut self, bus: &mut Bus, value: u16) -> u16 {
        self.tick_internal_timed(bus, 4);
        bus.maybe_trigger_oam_corruption(value, OamCorruptionType::Write);
        value.wrapping_add(1)
    }

    #[inline]
    pub fn dec_rp_timed(&mut self, bus: &mut Bus, value: u16) -> u16 {
        self.tick_internal_timed(bus, 4);
        bus.maybe_trigger_oam_corruption(value, OamCorruptionType::Write);
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
            halt_bug_active: reader.read_bool()?,
        })
    }

    #[inline]
    fn tick_peripherals(&mut self, bus: &mut Bus, t_cycles: u64) {
        self.timed_cycles_accounted = self.timed_cycles_accounted.wrapping_add(t_cycles);

        let is_double_speed = bus.hardware_mode == HardwareMode::CGBDouble;
        let system_t_cycles = if is_double_speed {
            t_cycles / 2
        } else {
            t_cycles
        };

        bus.step_timer(t_cycles);
        bus.step_serial(t_cycles);
        bus.step_apu(system_t_cycles);

        let previous_ppu_mode = bus.ppu_mode();
        let ppu_interrupt = bus.step_ppu(system_t_cycles);
        bus.if_reg |= ppu_interrupt;

        let current_ppu_mode = bus.ppu_mode();
        bus.maybe_step_hblank_hdma(previous_ppu_mode, current_ppu_mode);

        bus.step_oam_dma(t_cycles);

        bus.cartridge.step(system_t_cycles);
    }

    fn advance_pc_after_fetch(&mut self) {
        if self.halt_bug_active {
            self.halt_bug_active = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
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
mod tests;
#[cfg(test)]
mod alu_proptests;

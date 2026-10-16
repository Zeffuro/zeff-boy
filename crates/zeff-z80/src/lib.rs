#![forbid(unsafe_code)]

use std::num::NonZeroU64;

use zeff_emu_common::cpu::{CpuCore, CpuStep};

mod access;
mod alu;
mod bounded;
pub use bounded::{BoundedStep, PrefixState};
mod bus_segment;
pub use bus_segment::BusState;
mod cb;
mod constants;
pub use constants::*;
mod ed;
mod exchange;
mod flags;
mod index;
mod indexed_cb;
mod indexed_memory;
mod interrupt;
mod load;
mod misc;
mod rmw;
mod rotate;
mod stack;
mod state;
mod unprefixed;

pub trait Z80Bus {
    fn cpu_read(&self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8);

    fn io_read(&mut self, port: u8) -> u8;
    fn io_write(&mut self, port: u8, value: u8);
    #[inline]
    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        self.io_write(cycle.port, cycle.value);
    }
    fn maskable_interrupt_pending(&self) -> bool;
    fn non_maskable_interrupt_pending(&self) -> bool;
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool;
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        Z80_INTERRUPT_ACK_OPCODE
    }
    fn read_interrupt_instruction(&mut self, pc: u16) -> u8 {
        self.cpu_read(pc)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResetState {
    pub pc: u16,
    pub sp: u16,
}

impl ResetState {
    pub const fn new(pc: u16, sp: u16) -> Self {
        Self { pc, sp }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Running,
    Halted,
    Suspended,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InterruptMode {
    #[default]
    Im0,
    Im1,
    Im2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuTrap {
    UnsupportedOpcode { pc: u16, opcode: u8 },
    UnsupportedPrefixedOpcode { pc: u16, prefix: u8, opcode: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchedInstruction {
    pub pc: u16,
    pub opcode: u8,
    pub cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoWriteCycle {
    pub port: u8,
    pub value: u8,
    pub t_states_before: u32,
    pub t_states: u32,
}

impl CpuStep for FetchedInstruction {
    #[inline]
    fn cpu_cycles(&self) -> Option<NonZeroU64> {
        NonZeroU64::new(u64::from(self.cycles))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub ix: u16,
    pub iy: u16,
    pub sp: u16,
    pub pc: u16,
    pub i: u8,
    pub r: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ShadowRegisters {
    a: u8,
    f: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
}

impl Registers {
    pub fn af(self) -> u16 {
        u16::from_be_bytes([self.a, self.f])
    }

    pub fn bc(self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }

    pub fn de(self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }

    pub fn hl(self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    fn set_bc(&mut self, value: u16) {
        let [hi, lo] = value.to_be_bytes();
        self.b = hi;
        self.c = lo;
    }

    fn set_de(&mut self, value: u16) {
        let [hi, lo] = value.to_be_bytes();
        self.d = hi;
        self.e = lo;
    }

    fn set_hl(&mut self, value: u16) {
        let [hi, lo] = value.to_be_bytes();
        self.h = hi;
        self.l = lo;
    }

    fn set_af(&mut self, value: u16) {
        let [hi, lo] = value.to_be_bytes();
        self.a = hi;
        self.f = lo;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    reset_state: ResetState,
    regs: Registers,
    shadow: ShadowRegisters,
    state: CpuState,
    interrupt_mode: InterruptMode,
    interrupt_flip_flop_1: bool,
    interrupt_flip_flop_2: bool,
    enable_interrupts_delay: u8,
    cycles: u64,
    last_opcode_pc: u16,
    last_opcode: u8,
    last_step_was_interrupt: bool,
    instruction_bytes: [u8; 4],
    instruction_byte_count: u8,
    last_m1_fetch_count: u8,
    trap: Option<CpuTrap>,
}

impl Cpu {
    pub fn new() -> Self {
        Self::new_with_reset(ResetState::new(Z80_RESET_PC, Z80_RESET_SP))
    }

    pub fn new_with_reset(reset_state: ResetState) -> Self {
        let mut cpu = Self {
            reset_state,
            regs: Registers::default(),
            shadow: ShadowRegisters::default(),
            state: CpuState::Running,
            interrupt_mode: InterruptMode::default(),
            interrupt_flip_flop_1: false,
            interrupt_flip_flop_2: false,
            enable_interrupts_delay: 0,
            cycles: 0,
            last_opcode_pc: reset_state.pc,
            last_opcode: 0,
            last_step_was_interrupt: false,
            instruction_bytes: [0; 4],
            instruction_byte_count: 0,
            last_m1_fetch_count: 0,
            trap: None,
        };
        cpu.reset();
        cpu
    }

    pub fn reset(&mut self) {
        self.regs = Registers {
            sp: self.reset_state.sp,
            pc: self.reset_state.pc,
            ..Registers::default()
        };
        self.shadow = ShadowRegisters::default();
        self.state = CpuState::Running;
        self.interrupt_mode = InterruptMode::default();
        self.interrupt_flip_flop_1 = false;
        self.interrupt_flip_flop_2 = false;
        self.enable_interrupts_delay = 0;
        self.cycles = 0;
        self.last_opcode_pc = self.reset_state.pc;
        self.last_opcode = 0;
        self.last_step_was_interrupt = false;
        self.instruction_bytes = [0; 4];
        self.instruction_byte_count = 0;
        self.last_m1_fetch_count = 0;
        self.trap = None;
    }

    #[inline(always)]
    pub fn step<B: Z80Bus>(&mut self, bus: &mut B) -> Option<FetchedInstruction> {
        self.step_with_bus(bus)
    }

    #[inline(always)]
    pub fn step_with_bus<B: Z80Bus>(&mut self, bus: &mut B) -> Option<FetchedInstruction> {
        match self.begin_step(bus) {
            bounded::StepStart::Blocked => return None,
            bounded::StepStart::Handled(instruction) => return Some(instruction),
            bounded::StepStart::Interrupt(service) => return Some(service.execute(self, bus)),
            bounded::StepStart::Opcode => {}
        }

        let pc = self.regs.pc;
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        self.last_opcode_pc = pc;
        self.last_opcode = opcode;
        let cycles = self.execute_unprefixed(bus, pc, opcode);

        self.finish_instruction(cycles);
        Some(FetchedInstruction { pc, opcode, cycles })
    }

    pub fn regs(&self) -> Registers {
        self.regs
    }

    pub fn state(&self) -> CpuState {
        self.state
    }

    pub fn interrupt_mode(&self) -> InterruptMode {
        self.interrupt_mode
    }

    pub fn interrupts_enabled(&self) -> bool {
        self.interrupt_flip_flop_1
    }

    pub fn saved_interrupts_enabled(&self) -> bool {
        self.interrupt_flip_flop_2
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn last_opcode_pc(&self) -> u16 {
        self.last_opcode_pc
    }

    pub fn last_opcode(&self) -> u8 {
        self.last_opcode
    }

    pub fn last_step_was_interrupt(&self) -> bool {
        self.last_step_was_interrupt
    }

    pub fn last_m1_fetch_count(&self) -> u8 {
        self.last_m1_fetch_count
    }

    pub fn instruction_bytes(&self) -> &[u8] {
        &self.instruction_bytes[..usize::from(self.instruction_byte_count)]
    }

    pub fn trap(&self) -> Option<CpuTrap> {
        self.trap
    }

    pub fn is_suspended(&self) -> bool {
        self.state == CpuState::Suspended
    }

    pub fn is_halted(&self) -> bool {
        self.state == CpuState::Halted
    }

    pub fn suspend(&mut self) {
        self.state = CpuState::Suspended;
    }

    pub fn resume(&mut self) {
        if self.state == CpuState::Suspended {
            self.state = CpuState::Running;
        }
    }

    pub fn begin_guest_call<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        target: u16,
    ) -> (u16, u16, bool, bool, u8) {
        let state = (
            self.regs.pc,
            self.regs.sp,
            self.interrupt_flip_flop_1,
            self.interrupt_flip_flop_2,
            self.enable_interrupts_delay,
        );
        self.push_u16(bus, self.regs.pc);
        self.regs.pc = target;
        self.interrupt_flip_flop_1 = false;
        self.interrupt_flip_flop_2 = false;
        self.enable_interrupts_delay = 0;
        self.state = CpuState::Running;
        state
    }

    pub fn finish_guest_call(&mut self, iff1: bool, iff2: bool, delay: u8) {
        self.interrupt_flip_flop_1 = iff1;
        self.interrupt_flip_flop_2 = iff2;
        self.enable_interrupts_delay = delay;
        self.state = CpuState::Suspended;
    }

    pub fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        write_registers(w, self.regs);
        write_shadow_registers(w, self.shadow);
        w.write_u8(cpu_state_to_byte(self.state));
        w.write_u8(interrupt_mode_to_byte(self.interrupt_mode));
        w.write_bool(self.interrupt_flip_flop_1);
        w.write_bool(self.interrupt_flip_flop_2);
        w.write_u8(self.enable_interrupts_delay);
        w.write_u64(self.cycles);
        w.write_u16(self.last_opcode_pc);
        w.write_u8(self.last_opcode);
        write_trap(w, self.trap);
    }

    pub fn read_state(
        &mut self,
        r: &mut zeff_emu_common::save_state::StateReader<'_>,
    ) -> anyhow::Result<()> {
        self.regs = read_registers(r)?;
        self.shadow = read_shadow_registers(r)?;
        self.state = byte_to_cpu_state(r.read_u8()?)?;
        self.interrupt_mode = byte_to_interrupt_mode(r.read_u8()?)?;
        self.interrupt_flip_flop_1 = r.read_bool()?;
        self.interrupt_flip_flop_2 = r.read_bool()?;
        self.enable_interrupts_delay = r.read_u8()?;
        if self.enable_interrupts_delay > 2 {
            anyhow::bail!(
                "invalid Z80 delayed-interrupt countdown: {}",
                self.enable_interrupts_delay
            );
        }
        self.cycles = r.read_u64()?;
        self.last_opcode_pc = r.read_u16()?;
        self.last_opcode = r.read_u8()?;
        self.last_step_was_interrupt = false;
        self.instruction_bytes = [0; 4];
        self.instruction_byte_count = 0;
        self.last_m1_fetch_count = 0;
        self.trap = read_trap(r)?;
        Ok(())
    }
}

impl<B: Z80Bus> CpuCore<B> for Cpu {
    type Step = Option<FetchedInstruction>;

    #[inline(always)]
    fn step_cpu(&mut self, bus: &mut B) -> Self::Step {
        self.step_with_bus(bus)
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

fn write_registers(w: &mut zeff_emu_common::save_state::StateWriter, regs: Registers) {
    w.write_u8(regs.a);
    w.write_u8(regs.f);
    w.write_u8(regs.b);
    w.write_u8(regs.c);
    w.write_u8(regs.d);
    w.write_u8(regs.e);
    w.write_u8(regs.h);
    w.write_u8(regs.l);
    w.write_u16(regs.ix);
    w.write_u16(regs.iy);
    w.write_u16(regs.sp);
    w.write_u16(regs.pc);
    w.write_u8(regs.i);
    w.write_u8(regs.r);
}

fn read_registers(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
) -> anyhow::Result<Registers> {
    Ok(Registers {
        a: r.read_u8()?,
        f: r.read_u8()?,
        b: r.read_u8()?,
        c: r.read_u8()?,
        d: r.read_u8()?,
        e: r.read_u8()?,
        h: r.read_u8()?,
        l: r.read_u8()?,
        ix: r.read_u16()?,
        iy: r.read_u16()?,
        sp: r.read_u16()?,
        pc: r.read_u16()?,
        i: r.read_u8()?,
        r: r.read_u8()?,
    })
}

fn write_shadow_registers(w: &mut zeff_emu_common::save_state::StateWriter, regs: ShadowRegisters) {
    w.write_u8(regs.a);
    w.write_u8(regs.f);
    w.write_u8(regs.b);
    w.write_u8(regs.c);
    w.write_u8(regs.d);
    w.write_u8(regs.e);
    w.write_u8(regs.h);
    w.write_u8(regs.l);
}

fn read_shadow_registers(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
) -> anyhow::Result<ShadowRegisters> {
    Ok(ShadowRegisters {
        a: r.read_u8()?,
        f: r.read_u8()?,
        b: r.read_u8()?,
        c: r.read_u8()?,
        d: r.read_u8()?,
        e: r.read_u8()?,
        h: r.read_u8()?,
        l: r.read_u8()?,
    })
}

fn cpu_state_to_byte(state: CpuState) -> u8 {
    match state {
        CpuState::Running => 0,
        CpuState::Halted => 1,
        CpuState::Suspended => 2,
    }
}

fn byte_to_cpu_state(value: u8) -> anyhow::Result<CpuState> {
    match value {
        0 => Ok(CpuState::Running),
        1 => Ok(CpuState::Halted),
        2 => Ok(CpuState::Suspended),
        _ => anyhow::bail!("invalid Z80 CPU state tag in save-state: {value}"),
    }
}

fn interrupt_mode_to_byte(mode: InterruptMode) -> u8 {
    match mode {
        InterruptMode::Im0 => 0,
        InterruptMode::Im1 => 1,
        InterruptMode::Im2 => 2,
    }
}

fn byte_to_interrupt_mode(value: u8) -> anyhow::Result<InterruptMode> {
    match value {
        0 => Ok(InterruptMode::Im0),
        1 => Ok(InterruptMode::Im1),
        2 => Ok(InterruptMode::Im2),
        _ => anyhow::bail!("invalid Z80 interrupt mode tag in save-state: {value}"),
    }
}

fn write_trap(w: &mut zeff_emu_common::save_state::StateWriter, trap: Option<CpuTrap>) {
    match trap {
        None => w.write_u8(0),
        Some(CpuTrap::UnsupportedOpcode { pc, opcode }) => {
            w.write_u8(1);
            w.write_u16(pc);
            w.write_u8(opcode);
        }
        Some(CpuTrap::UnsupportedPrefixedOpcode { pc, prefix, opcode }) => {
            w.write_u8(2);
            w.write_u16(pc);
            w.write_u8(prefix);
            w.write_u8(opcode);
        }
    }
}

fn read_trap(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
) -> anyhow::Result<Option<CpuTrap>> {
    match r.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(CpuTrap::UnsupportedOpcode {
            pc: r.read_u16()?,
            opcode: r.read_u8()?,
        })),
        2 => Ok(Some(CpuTrap::UnsupportedPrefixedOpcode {
            pc: r.read_u16()?,
            prefix: r.read_u8()?,
            opcode: r.read_u8()?,
        })),
        value => anyhow::bail!("invalid Z80 CPU trap tag in save-state: {value}"),
    }
}

#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod flat_bus_tests;
#[cfg(test)]
mod tests;

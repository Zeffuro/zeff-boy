use super::bus::Bus;
use super::constants::{
    Z80_FLAG_BIT_3, Z80_FLAG_BIT_5, Z80_FLAG_CARRY, Z80_FLAG_HALF_CARRY, Z80_FLAG_PARITY_OVERFLOW,
    Z80_FLAG_SIGN, Z80_FLAG_SUBTRACT, Z80_FLAG_ZERO, Z80_INTERRUPT_ACK_OPCODE,
    Z80_INTERRUPT_VECTOR_IM1, Z80_RESET_PC, Z80_RESET_SP,
};

mod access;
mod alu;
mod cb;
mod ed;
mod exchange;
mod flags;
mod index;
mod interrupt;
mod misc;
mod rotate;
mod unprefixed;

const CYCLES_NOP: u32 = 4;
const CYCLES_LD_RR_NN: u32 = 10;
const CYCLES_INDEX_LD_RR_NN: u32 = 14;
const CYCLES_INDEX_LD_R_N: u32 = 11;
const CYCLES_INC_DEC_RR: u32 = 6;
const CYCLES_INDEX_INC_DEC_RR: u32 = 10;
const CYCLES_ADD_HL_RR: u32 = 11;
const CYCLES_INDEX_ADD_RR: u32 = 15;
const CYCLES_LD_R_N: u32 = 7;
const CYCLES_LD_HL_N: u32 = 10;
const CYCLES_INDEX_LD_MEM_N: u32 = 19;
const CYCLES_LD_R_R: u32 = 4;
const CYCLES_INDEX_LD_R_R: u32 = 8;
const CYCLES_LD_R_HL_OR_HL_R: u32 = 7;
const CYCLES_INDEX_LD_R_MEM: u32 = 19;
const CYCLES_INDEX_LD_MEM_R: u32 = 19;
const CYCLES_JR: u32 = 12;
const CYCLES_JR_NOT_TAKEN: u32 = 7;
const CYCLES_DJNZ: u32 = 13;
const CYCLES_DJNZ_NOT_TAKEN: u32 = 8;
const CYCLES_LD_INDIRECT_A: u32 = 7;
const CYCLES_LD_A_INDIRECT: u32 = 7;
const CYCLES_ACCUMULATOR_ROTATE: u32 = 4;
const CYCLES_LD_NN_A: u32 = 13;
const CYCLES_LD_A_NN: u32 = 13;
const CYCLES_LD_NN_HL: u32 = 16;
const CYCLES_LD_HL_NN: u32 = 16;
const CYCLES_INDEX_LD_NN_RR: u32 = 20;
const CYCLES_INDEX_LD_RR_NN_INDIRECT: u32 = 20;
const CYCLES_LD_SP_HL: u32 = 6;
const CYCLES_INDEX_LD_SP_RR: u32 = 10;
const CYCLES_LD_A_N: u32 = 7;
const CYCLES_INC_DEC_R: u32 = 4;
const CYCLES_INDEX_INC_DEC_R: u32 = 8;
const CYCLES_INC_DEC_HL: u32 = 11;
const CYCLES_INDEX_INC_DEC_MEM: u32 = 23;
const CYCLES_HALT: u32 = 4;
const CYCLES_DI_EI: u32 = 4;
const CYCLES_ALU_R: u32 = 4;
const CYCLES_ALU_HL: u32 = 7;
const CYCLES_INDEX_ALU_MEM: u32 = 19;
const CYCLES_ALU_N: u32 = 7;
const CYCLES_JP_NN: u32 = 10;
const CYCLES_JP_HL: u32 = 4;
const CYCLES_INDEX_JP_RR: u32 = 8;
const CYCLES_CALL_NN: u32 = 17;
const CYCLES_CALL_NN_NOT_TAKEN: u32 = 10;
const CYCLES_RET: u32 = 10;
const CYCLES_RET_CC: u32 = 11;
const CYCLES_RET_CC_NOT_TAKEN: u32 = 5;
const CYCLES_RST: u32 = 11;
const CYCLES_PUSH_RR: u32 = 11;
const CYCLES_POP_RR: u32 = 10;
const CYCLES_INDEX_PUSH_RR: u32 = 15;
const CYCLES_INDEX_POP_RR: u32 = 14;
const CYCLES_IN_A_N: u32 = 11;
const CYCLES_OUT_N_A: u32 = 11;
const CYCLES_CB_R: u32 = 8;
const CYCLES_CB_HL: u32 = 15;
const CYCLES_CB_BIT_HL: u32 = 12;
const CYCLES_INDEX_CB: u32 = 23;
const CYCLES_ED_IO: u32 = 12;
const CYCLES_ED_BLOCK: u32 = 16;
const CYCLES_ED_BLOCK_REPEAT: u32 = 21;
const CYCLES_ED_16BIT_ALU: u32 = 15;
const CYCLES_ED_16BIT_MEMORY: u32 = 20;
const CYCLES_ED_NEG: u32 = 8;
const CYCLES_ED_NIBBLE_ROTATE: u32 = 18;
const CYCLES_ED_SPECIAL_REGISTER: u32 = 9;
const CYCLES_INTERRUPT_ACK: u32 = 13;
const CYCLES_IM: u32 = 8;
const CYCLES_ED_NOP: u32 = 8;
const CYCLES_RETI_RETN: u32 = 14;
const CYCLES_EX_AF_AF_SHADOW: u32 = 4;
const CYCLES_EX_DE_HL: u32 = 4;
const CYCLES_EX_SP_HL: u32 = 19;
const CYCLES_INDEX_EX_SP_RR: u32 = 23;
const CYCLES_EXX: u32 = 4;
const CYCLES_FLAG_OP: u32 = 4;
const CYCLES_UNSUPPORTED: u32 = 0;
const REFRESH_COUNTER_MASK: u8 = 0x7F;
const REFRESH_COUNTER_BIT_7_MASK: u8 = 0x80;
const Z80_PREFIX_CB: u8 = 0xCB;
const Z80_PREFIX_DD: u8 = 0xDD;
const Z80_PREFIX_ED: u8 = 0xED;
const Z80_PREFIX_FD: u8 = 0xFD;
const Z80_PREFIX_OVERHEAD: u32 = 4;
const REGISTER_MEMORY_INDEX: u8 = 6;
const REGISTER_A_INDEX: u8 = 7;
const CONDITION_NZ: u8 = 0;
const CONDITION_Z: u8 = 1;
const CONDITION_NC: u8 = 2;
const CONDITION_C: u8 = 3;
const CONDITION_PO: u8 = 4;
const CONDITION_PE: u8 = 5;
const CONDITION_P: u8 = 6;
const CONDITION_M: u8 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Running,
    Halted,
    Suspended,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InterruptMode {
    Im0,
    #[default]
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

#[derive(Clone, Debug)]
pub struct Cpu {
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
    trap: Option<CpuTrap>,
}

impl Cpu {
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: Registers::default(),
            shadow: ShadowRegisters::default(),
            state: CpuState::Running,
            interrupt_mode: InterruptMode::default(),
            interrupt_flip_flop_1: false,
            interrupt_flip_flop_2: false,
            enable_interrupts_delay: 0,
            cycles: 0,
            last_opcode_pc: Z80_RESET_PC,
            last_opcode: 0,
            trap: None,
        };
        cpu.reset();
        cpu
    }

    pub fn reset(&mut self) {
        self.regs = Registers {
            sp: Z80_RESET_SP,
            pc: Z80_RESET_PC,
            ..Registers::default()
        };
        self.shadow = ShadowRegisters::default();
        self.state = CpuState::Running;
        self.interrupt_mode = InterruptMode::default();
        self.interrupt_flip_flop_1 = false;
        self.interrupt_flip_flop_2 = false;
        self.enable_interrupts_delay = 0;
        self.cycles = 0;
        self.last_opcode_pc = Z80_RESET_PC;
        self.last_opcode = 0;
        self.trap = None;
    }

    pub fn step(&mut self, bus: &mut Bus) -> Option<FetchedInstruction> {
        if self.state == CpuState::Suspended {
            return None;
        }

        if let Some(interrupt) = self.try_service_maskable_interrupt(bus) {
            return Some(interrupt);
        }

        if self.state == CpuState::Halted {
            let cycles = CYCLES_HALT;
            self.cycles = self.cycles.wrapping_add(u64::from(cycles));
            return Some(FetchedInstruction {
                pc: self.regs.pc,
                opcode: 0x76,
                cycles,
            });
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

    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
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

    pub(crate) fn read_state(
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
        self.cycles = r.read_u64()?;
        self.last_opcode_pc = r.read_u16()?;
        self.last_opcode = r.read_u8()?;
        self.trap = read_trap(r)?;
        Ok(())
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
        _ => anyhow::bail!("invalid Sega 8-bit CPU state tag in save-state: {value}"),
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
        _ => anyhow::bail!("invalid Sega 8-bit interrupt mode tag in save-state: {value}"),
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
        value => anyhow::bail!("invalid Sega 8-bit CPU trap tag in save-state: {value}"),
    }
}

#[cfg(test)]
mod tests;

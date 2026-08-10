use super::bus::Bus;
use super::constants::ADDRESS_MASK;

mod alu;
mod execute;
mod instructions;
mod memory;
mod shifts;
mod strings;
#[cfg(test)]
mod tests;

const REG_AX: u8 = 0;
const REG_CX: u8 = 1;
const REG_DX: u8 = 2;
const REG_BX: u8 = 3;
const REG_SP: u8 = 4;
const REG_BP: u8 = 5;
const REG_SI: u8 = 6;
const REG_DI: u8 = 7;

const FLAG_CF: u16 = 0x0001;
const FLAG_FIXED: u16 = 0xF002;
const FLAG_PF: u16 = 0x0004;
const FLAG_AF: u16 = 0x0010;
const FLAG_ZF: u16 = 0x0040;
const FLAG_SF: u16 = 0x0080;
const FLAG_BRK: u16 = 0x0100;
const FLAG_IF: u16 = 0x0200;
const FLAG_DF: u16 = 0x0400;
const FLAG_OF: u16 = 0x0800;
const FLAG_RESERVED_LOW: u16 = 0x0028;
const FLAG_POPF_WRITABLE: u16 =
    FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_BRK | FLAG_IF | FLAG_OF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepeatPrefix {
    Repe,
    Repne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Running,
    Halted,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentRegister {
    Es,
    Cs,
    Ss,
    Ds,
}

impl SegmentRegister {
    fn index(self) -> usize {
        match self {
            Self::Es => 0,
            Self::Cs => 1,
            Self::Ss => 2,
            Self::Ds => 3,
        }
    }

    fn from_prefix(prefix: u8) -> Option<Self> {
        match prefix {
            0x26 => Some(Self::Es),
            0x2E => Some(Self::Cs),
            0x36 => Some(Self::Ss),
            0x3E => Some(Self::Ds),
            _ => None,
        }
    }

    fn from_modrm_reg(reg: u8) -> Self {
        match reg & 0x03 {
            0 => Self::Es,
            1 => Self::Cs,
            2 => Self::Ss,
            _ => Self::Ds,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchedInstruction {
    pub cs: u16,
    pub ip: u16,
    pub pc: u32,
    pub opcode: u8,
    pub cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuTrap {
    UnsupportedOpcode {
        cs: u16,
        ip: u16,
        opcode: u8,
    },
    UnsupportedInstructionForm {
        cs: u16,
        ip: u16,
        opcode: u8,
        modrm: u8,
    },
    DivideError {
        cs: u16,
        ip: u16,
        opcode: u8,
        modrm: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModRm {
    byte: u8,
    mode: u8,
    reg: u8,
    rm: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operand {
    Register(u8),
    Memory(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AluOp {
    Add,
    Adc,
    Or,
    And,
    Sub,
    Sbb,
    Xor,
    Cmp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cpu {
    pub regs: [u16; 8],
    pub segments: [u16; 4],
    pub ip: u16,
    pub flags: u16,
    pub cycles: u64,
    pub state: CpuState,
    pub last_fetch: Option<FetchedInstruction>,
    pub last_opcode: u8,
    pub last_trap: Option<CpuTrap>,
    pub(crate) interrupt_shadow: u8,
    pub(crate) brk_shadow: u8,
    last_mul_overflow: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        let mut cpu = Self {
            regs: [0; 8],
            segments: [0; 4],
            ip: 0,
            flags: FLAG_FIXED,
            cycles: 0,
            state: CpuState::Running,
            last_fetch: None,
            last_opcode: 0,
            last_trap: None,
            interrupt_shadow: 0,
            brk_shadow: 0,
            last_mul_overflow: false,
        };
        cpu.reset();
        cpu
    }

    pub fn reset(&mut self) {
        self.regs = [0; 8];
        self.segments = [0; 4];
        self.segments[SegmentRegister::Cs.index()] = 0xFFFF;
        self.ip = 0;
        self.flags = FLAG_FIXED;
        self.cycles = 0;
        self.state = CpuState::Running;
        self.last_fetch = None;
        self.last_opcode = 0;
        self.last_trap = None;
        self.interrupt_shadow = 0;
        self.brk_shadow = 0;
        self.last_mul_overflow = false;
    }

    pub fn apply_cartridge_start_state(&mut self, color: bool) {
        self.reset();
        self.regs[REG_AX as usize] = if color { 0xFF87 } else { 0xFF85 };
        self.regs[REG_BX as usize] = if color { 0x0043 } else { 0x0040 };
        self.regs[REG_SP as usize] = 0x2000;
        self.regs[REG_SI as usize] = if color { 0x0457 } else { 0x023D };
        self.regs[REG_DI as usize] = if color { 0x040B } else { 0x040D };
        self.segments[SegmentRegister::Es.index()] = 0x0000;
        self.segments[SegmentRegister::Cs.index()] = 0xFFFF;
        self.segments[SegmentRegister::Ss.index()] = 0x0000;
        self.segments[SegmentRegister::Ds.index()] = if color { 0xFE00 } else { 0xFF00 };
        self.flags = if color { 0xF086 } else { 0xF082 };
    }

    pub fn is_suspended(&self) -> bool {
        self.state == CpuState::Suspended
    }

    pub fn suspend(&mut self) {
        self.state = CpuState::Suspended;
    }

    pub fn resume(&mut self) {
        if self.state == CpuState::Suspended {
            self.state = CpuState::Running;
        }
    }

    pub fn pc(&self) -> u32 {
        self.physical_address(SegmentRegister::Cs, self.ip)
    }

    fn normalize_popped_flags(value: u16) -> u16 {
        (value & FLAG_POPF_WRITABLE) | FLAG_FIXED
    }

    pub fn step(&mut self, bus: &mut Bus) -> Option<FetchedInstruction> {
        if self.state == CpuState::Suspended {
            return None;
        }

        if self.service_pending_interrupt_or_brk(bus) {
            return None;
        }

        match self.state {
            CpuState::Suspended => return None,
            CpuState::Halted => {
                self.add_cycles(bus, 1);
                return None;
            }
            CpuState::Running => {}
        }

        let start_cs = self.segments[SegmentRegister::Cs.index()];
        let start_ip = self.ip;
        let start_pc = self.pc();
        let before_cycles = self.cycles;
        let mut segment_override = None;
        let mut repeat_prefix = None;
        let opcode = loop {
            let byte = self.fetch8(bus);
            if let Some(seg) = SegmentRegister::from_prefix(byte) {
                segment_override = Some(seg);
                continue;
            }
            match byte {
                0xF0 => {
                    continue;
                }
                0xF2 => {
                    repeat_prefix = Some(RepeatPrefix::Repne);
                    continue;
                }
                0xF3 => {
                    repeat_prefix = Some(RepeatPrefix::Repe);
                    continue;
                }
                _ => {}
            }
            break byte;
        };
        let pending_fetch = FetchedInstruction {
            cs: start_cs,
            ip: start_ip,
            pc: start_pc,
            opcode,
            cycles: 0,
        };
        self.last_fetch = Some(pending_fetch);
        self.last_opcode = opcode;
        self.execute(opcode, segment_override, repeat_prefix, bus);
        if self.should_resume_repeated_string(opcode, repeat_prefix) {
            self.ip = start_ip;
        }
        let cycles = self
            .cycles
            .wrapping_sub(before_cycles)
            .min(u64::from(u32::MAX)) as u32;
        let fetched = FetchedInstruction {
            cs: start_cs,
            ip: start_ip,
            pc: start_pc,
            opcode,
            cycles,
        };
        self.last_fetch = Some(fetched);
        self.last_opcode = opcode;
        Some(fetched)
    }

    fn service_pending_interrupt_or_brk(&mut self, bus: &mut Bus) -> bool {
        if self.state == CpuState::Halted && bus.has_pending_interrupt_signal() {
            self.state = CpuState::Running;
        }

        let hardware_suppressed = self.consume_interrupt_shadow();
        let brk_suppressed = self.consume_brk_shadow();

        if !brk_suppressed && self.flags & FLAG_BRK != 0 {
            self.enter_interrupt(1, 32, bus);
            return true;
        }

        if !hardware_suppressed
            && self.flags & FLAG_IF != 0
            && let Some(vector) = bus.pending_interrupt_vector()
        {
            self.enter_interrupt(vector, 32, bus);
            return true;
        }

        false
    }

    pub(super) fn enter_interrupt(&mut self, vector: u8, cycles: u32, bus: &mut Bus) {
        self.state = CpuState::Running;
        self.push16(self.flags | FLAG_FIXED, bus);
        self.push16(self.segments[SegmentRegister::Cs.index()], bus);
        self.push16(self.ip, bus);
        self.flags &= !(FLAG_BRK | FLAG_IF);
        self.flags |= FLAG_FIXED;
        self.interrupt_shadow = 0;
        self.brk_shadow = 0;
        let vector_addr = u32::from(vector) * 4;
        self.ip = bus.read16(vector_addr);
        self.segments[SegmentRegister::Cs.index()] = bus.read16(vector_addr + 2);
        self.add_cycles(bus, cycles);
    }

    pub(super) fn set_popped_flags(&mut self, value: u16) {
        let previous = self.flags;
        self.flags = Self::normalize_popped_flags(value);
        self.defer_enabled_flag_transitions(previous);
    }

    pub(super) fn enable_interrupt_flag(&mut self) {
        if self.flags & FLAG_IF == 0 {
            self.interrupt_shadow = 1;
        }
        self.flags |= FLAG_IF | FLAG_FIXED;
    }

    pub(super) fn defer_after_ss_load(&mut self) {
        self.interrupt_shadow = 1;
        self.brk_shadow = 1;
    }

    fn defer_enabled_flag_transitions(&mut self, previous: u16) {
        if previous & FLAG_IF == 0 && self.flags & FLAG_IF != 0 {
            self.interrupt_shadow = 1;
        }
        if previous & FLAG_BRK == 0 && self.flags & FLAG_BRK != 0 {
            self.brk_shadow = 1;
        }
    }

    fn consume_interrupt_shadow(&mut self) -> bool {
        if self.interrupt_shadow == 0 {
            return false;
        }
        self.interrupt_shadow -= 1;
        true
    }

    fn consume_brk_shadow(&mut self) -> bool {
        if self.brk_shadow == 0 {
            return false;
        }
        self.brk_shadow -= 1;
        true
    }

    fn should_resume_repeated_string(
        &self,
        opcode: u8,
        repeat_prefix: Option<RepeatPrefix>,
    ) -> bool {
        let Some(prefix) = repeat_prefix else {
            return false;
        };
        if self.state != CpuState::Running || self.get_reg16(REG_CX) == 0 {
            return false;
        }

        match opcode {
            0x6C..=0x6F | 0xA4 | 0xA5 | 0xAA..=0xAD => true,
            0xA6 | 0xA7 | 0xAE | 0xAF => {
                let zf = self.flags & FLAG_ZF != 0;
                match prefix {
                    RepeatPrefix::Repe => zf,
                    RepeatPrefix::Repne => !zf,
                }
            }
            _ => false,
        }
    }
}

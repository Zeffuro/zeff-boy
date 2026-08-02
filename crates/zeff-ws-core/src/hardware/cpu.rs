use super::bus::Bus;
use super::constants::ADDRESS_MASK;

const REG_AX: u8 = 0;
const REG_DX: u8 = 2;
const REG_BX: u8 = 3;
const REG_SP: u8 = 4;
const REG_BP: u8 = 5;
const REG_SI: u8 = 6;
const REG_DI: u8 = 7;

const FLAG_CF: u16 = 0x0001;
const FLAG_FIXED: u16 = 0x0002;
const FLAG_PF: u16 = 0x0004;
const FLAG_ZF: u16 = 0x0040;
const FLAG_SF: u16 = 0x0080;
const FLAG_IF: u16 = 0x0200;
const FLAG_DF: u16 = 0x0400;
const FLAG_OF: u16 = 0x0800;

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
    Or,
    And,
    Sub,
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
    }

    pub fn is_suspended(&self) -> bool {
        self.state == CpuState::Suspended
    }

    pub fn suspend(&mut self) {
        self.state = CpuState::Suspended;
    }

    pub fn resume(&mut self) {
        if self.state != CpuState::Suspended {
            self.state = CpuState::Running;
        }
    }

    pub fn pc(&self) -> u32 {
        self.physical_address(SegmentRegister::Cs, self.ip)
    }

    pub fn step(&mut self, bus: &mut Bus) -> Option<FetchedInstruction> {
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
        let opcode = loop {
            let byte = self.fetch8(bus);
            if let Some(seg) = SegmentRegister::from_prefix(byte) {
                segment_override = Some(seg);
                continue;
            }
            break byte;
        };
        self.execute(opcode, segment_override, bus);
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

    fn execute(
        &mut self,
        opcode: u8,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        match opcode {
            0x00 => self.alu_rm_reg8(AluOp::Add, segment_override, bus),
            0x01 => self.alu_rm_reg16(AluOp::Add, segment_override, bus),
            0x02 => self.alu_reg_rm8(AluOp::Add, segment_override, bus),
            0x03 => self.alu_reg_rm16(AluOp::Add, segment_override, bus),
            0x04 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Add, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x05 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Add, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x06 => self.push_segment(SegmentRegister::Es, bus),
            0x07 => self.pop_segment(SegmentRegister::Es, bus),
            0x08 => self.alu_rm_reg8(AluOp::Or, segment_override, bus),
            0x09 => self.alu_rm_reg16(AluOp::Or, segment_override, bus),
            0x0A => self.alu_reg_rm8(AluOp::Or, segment_override, bus),
            0x0B => self.alu_reg_rm16(AluOp::Or, segment_override, bus),
            0x0C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Or, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x0D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Or, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x0E => self.push_segment(SegmentRegister::Cs, bus),
            0x16 => self.push_segment(SegmentRegister::Ss, bus),
            0x17 => self.pop_segment(SegmentRegister::Ss, bus),
            0x1E => self.push_segment(SegmentRegister::Ds, bus),
            0x1F => self.pop_segment(SegmentRegister::Ds, bus),
            0x20 => self.alu_rm_reg8(AluOp::And, segment_override, bus),
            0x21 => self.alu_rm_reg16(AluOp::And, segment_override, bus),
            0x22 => self.alu_reg_rm8(AluOp::And, segment_override, bus),
            0x23 => self.alu_reg_rm16(AluOp::And, segment_override, bus),
            0x24 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::And, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x25 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::And, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x28 => self.alu_rm_reg8(AluOp::Sub, segment_override, bus),
            0x29 => self.alu_rm_reg16(AluOp::Sub, segment_override, bus),
            0x2A => self.alu_reg_rm8(AluOp::Sub, segment_override, bus),
            0x2B => self.alu_reg_rm16(AluOp::Sub, segment_override, bus),
            0x2C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Sub, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x2D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Sub, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x30 => self.alu_rm_reg8(AluOp::Xor, segment_override, bus),
            0x31 => self.alu_rm_reg16(AluOp::Xor, segment_override, bus),
            0x32 => self.alu_reg_rm8(AluOp::Xor, segment_override, bus),
            0x33 => self.alu_reg_rm16(AluOp::Xor, segment_override, bus),
            0x34 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Xor, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x35 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Xor, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x38 => self.alu_rm_reg8(AluOp::Cmp, segment_override, bus),
            0x39 => self.alu_rm_reg16(AluOp::Cmp, segment_override, bus),
            0x3A => self.alu_reg_rm8(AluOp::Cmp, segment_override, bus),
            0x3B => self.alu_reg_rm16(AluOp::Cmp, segment_override, bus),
            0x3C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                self.alu8(AluOp::Cmp, lhs, rhs);
                self.add_cycles(bus, 4);
            }
            0x3D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                self.alu16(AluOp::Cmp, lhs, rhs);
                self.add_cycles(bus, 4);
            }
            0x40..=0x47 => {
                let reg = opcode - 0x40;
                let value = self.get_reg16(reg).wrapping_add(1);
                self.set_inc_dec_flags16(value, false);
                self.set_reg16(reg, value);
                self.add_cycles(bus, 3);
            }
            0x48..=0x4F => {
                let reg = opcode - 0x48;
                let value = self.get_reg16(reg).wrapping_sub(1);
                self.set_inc_dec_flags16(value, true);
                self.set_reg16(reg, value);
                self.add_cycles(bus, 3);
            }
            0x50..=0x57 => {
                self.push16(self.get_reg16(opcode - 0x50), bus);
                self.add_cycles(bus, 4);
            }
            0x58..=0x5F => {
                let value = self.pop16(bus);
                self.set_reg16(opcode - 0x58, value);
                self.add_cycles(bus, 4);
            }
            0x70..=0x7F => {
                let rel = self.fetch8(bus) as i8;
                if self.condition(opcode & 0x0F) {
                    self.ip = self.ip.wrapping_add_signed(i16::from(rel));
                }
                self.add_cycles(bus, 4);
            }
            0x80 => self.alu_rm_imm8(false, segment_override, bus),
            0x81 => self.alu_rm_imm16(false, segment_override, bus),
            0x83 => self.alu_rm_imm16(true, segment_override, bus),
            0x88 => self.mov_rm_reg8(segment_override, bus),
            0x89 => self.mov_rm_reg16(segment_override, bus),
            0x8A => self.mov_reg_rm8(segment_override, bus),
            0x8B => self.mov_reg_rm16(segment_override, bus),
            0x8C => self.mov_rm_sreg(segment_override, bus),
            0x8E => self.mov_sreg_rm(segment_override, bus),
            0x90 => self.add_cycles(bus, 3),
            0x9C => {
                self.push16(self.flags | FLAG_FIXED, bus);
                self.add_cycles(bus, 8);
            }
            0x9D => {
                self.flags = self.pop16(bus) | FLAG_FIXED;
                self.add_cycles(bus, 8);
            }
            0xA0 => {
                let offset = self.fetch16(bus);
                let value = bus.read8(self.overridden_address(segment_override, offset));
                self.set_reg8(REG_AX, value);
                self.add_cycles(bus, 10);
            }
            0xA1 => {
                let offset = self.fetch16(bus);
                let value = bus.read16(self.overridden_address(segment_override, offset));
                self.set_reg16(REG_AX, value);
                self.add_cycles(bus, 10);
            }
            0xA2 => {
                let offset = self.fetch16(bus);
                bus.write8(
                    self.overridden_address(segment_override, offset),
                    self.get_reg8(REG_AX),
                );
                self.add_cycles(bus, 10);
            }
            0xA3 => {
                let offset = self.fetch16(bus);
                bus.write16(
                    self.overridden_address(segment_override, offset),
                    self.get_reg16(REG_AX),
                );
                self.add_cycles(bus, 10);
            }
            0xB0..=0xB7 => {
                let value = self.fetch8(bus);
                self.set_reg8(opcode - 0xB0, value);
                self.add_cycles(bus, 4);
            }
            0xB8..=0xBF => {
                let value = self.fetch16(bus);
                self.set_reg16(opcode - 0xB8, value);
                self.add_cycles(bus, 4);
            }
            0xC3 => {
                self.ip = self.pop16(bus);
                self.add_cycles(bus, 8);
            }
            0xC6 => self.mov_rm_imm8(segment_override, bus),
            0xC7 => self.mov_rm_imm16(segment_override, bus),
            0xCB => {
                self.ip = self.pop16(bus);
                let cs = self.pop16(bus);
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 12);
            }
            0xE4 => {
                let port = u16::from(self.fetch8(bus));
                let value = bus.io_read8(port);
                self.set_reg8(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xE5 => {
                let port = u16::from(self.fetch8(bus));
                let value = bus.io_read16(port);
                self.set_reg16(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xE6 => {
                let port = u16::from(self.fetch8(bus));
                bus.io_write8(port, self.get_reg8(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xE7 => {
                let port = u16::from(self.fetch8(bus));
                bus.io_write16(port, self.get_reg16(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xE8 => {
                let rel = self.fetch16(bus) as i16;
                self.push16(self.ip, bus);
                self.ip = self.ip.wrapping_add_signed(rel);
                self.add_cycles(bus, 12);
            }
            0xE9 => {
                let rel = self.fetch16(bus) as i16;
                self.ip = self.ip.wrapping_add_signed(rel);
                self.add_cycles(bus, 4);
            }
            0xEA => {
                let ip = self.fetch16(bus);
                let cs = self.fetch16(bus);
                self.ip = ip;
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 8);
            }
            0xEB => {
                let rel = self.fetch8(bus) as i8;
                self.ip = self.ip.wrapping_add_signed(i16::from(rel));
                self.add_cycles(bus, 4);
            }
            0xEC => {
                let value = bus.io_read8(self.get_reg16(REG_DX));
                self.set_reg8(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xED => {
                let value = bus.io_read16(self.get_reg16(REG_DX));
                self.set_reg16(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xEE => {
                bus.io_write8(self.get_reg16(REG_DX), self.get_reg8(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xEF => {
                bus.io_write16(self.get_reg16(REG_DX), self.get_reg16(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xF4 => {
                self.state = CpuState::Halted;
                self.add_cycles(bus, 2);
            }
            0xFA => {
                self.flags &= !FLAG_IF;
                self.flags |= FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFB => {
                self.flags |= FLAG_IF | FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFC => {
                self.flags &= !FLAG_DF;
                self.flags |= FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFD => {
                self.flags |= FLAG_DF | FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            _ => self.unsupported_opcode(opcode),
        }
    }

    fn mov_rm_reg8(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        self.write_operand8(operand, self.get_reg8(modrm.reg), bus);
        self.add_cycles(bus, 4);
    }

    fn mov_rm_reg16(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        self.write_operand16(operand, self.get_reg16(modrm.reg), bus);
        self.add_cycles(bus, 4);
    }

    fn mov_reg_rm8(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.read_operand8(operand, bus);
        self.set_reg8(modrm.reg, value);
        self.add_cycles(bus, 4);
    }

    fn mov_reg_rm16(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.read_operand16(operand, bus);
        self.set_reg16(modrm.reg, value);
        self.add_cycles(bus, 4);
    }

    fn mov_rm_sreg(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let segment = SegmentRegister::from_modrm_reg(modrm.reg);
        self.write_operand16(operand, self.segments[segment.index()], bus);
        self.add_cycles(bus, 4);
    }

    fn mov_sreg_rm(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let segment = SegmentRegister::from_modrm_reg(modrm.reg);
        let value = self.read_operand16(operand, bus);
        self.segments[segment.index()] = value;
        self.add_cycles(bus, 4);
    }

    fn mov_rm_imm8(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        if modrm.reg != 0 {
            self.unsupported_form(0xC6, modrm.byte);
            return;
        }
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.fetch8(bus);
        self.write_operand8(operand, value, bus);
        self.add_cycles(bus, 4);
    }

    fn mov_rm_imm16(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        if modrm.reg != 0 {
            self.unsupported_form(0xC7, modrm.byte);
            return;
        }
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.fetch16(bus);
        self.write_operand16(operand, value, bus);
        self.add_cycles(bus, 4);
    }

    fn alu_rm_reg8(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand8(operand, bus);
        let rhs = self.get_reg8(modrm.reg);
        let result = self.alu8(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand8(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    fn alu_rm_reg16(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand16(operand, bus);
        let rhs = self.get_reg16(modrm.reg);
        let result = self.alu16(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand16(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    fn alu_reg_rm8(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.get_reg8(modrm.reg);
        let rhs = self.read_operand8(operand, bus);
        let result = self.alu8(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.set_reg8(modrm.reg, result);
        }
        self.add_cycles(bus, 4);
    }

    fn alu_reg_rm16(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.get_reg16(modrm.reg);
        let rhs = self.read_operand16(operand, bus);
        let result = self.alu16(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.set_reg16(modrm.reg, result);
        }
        self.add_cycles(bus, 4);
    }

    fn alu_rm_imm8(&mut self, signed_imm: bool, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let Some(op) = alu_group_op(modrm.reg) else {
            self.unsupported_form(0x80, modrm.byte);
            return;
        };
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand8(operand, bus);
        let imm = self.fetch8(bus);
        let rhs = if signed_imm { imm as i8 as u8 } else { imm };
        let result = self.alu8(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand8(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    fn alu_rm_imm16(
        &mut self,
        signed_imm: bool,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let opcode = if signed_imm { 0x83 } else { 0x81 };
        let modrm = self.fetch_modrm(bus);
        let Some(op) = alu_group_op(modrm.reg) else {
            self.unsupported_form(opcode, modrm.byte);
            return;
        };
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand16(operand, bus);
        let rhs = if signed_imm {
            self.fetch8(bus) as i8 as i16 as u16
        } else {
            self.fetch16(bus)
        };
        let result = self.alu16(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand16(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    fn push_segment(&mut self, segment: SegmentRegister, bus: &mut Bus) {
        self.push16(self.segments[segment.index()], bus);
        self.add_cycles(bus, 4);
    }

    fn pop_segment(&mut self, segment: SegmentRegister, bus: &mut Bus) {
        let value = self.pop16(bus);
        self.segments[segment.index()] = value;
        self.add_cycles(bus, 4);
    }

    fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let value = bus.read8(self.pc());
        self.ip = self.ip.wrapping_add(1);
        value
    }

    fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch8(bus);
        let hi = self.fetch8(bus);
        u16::from_le_bytes([lo, hi])
    }

    fn fetch_modrm(&mut self, bus: &mut Bus) -> ModRm {
        let byte = self.fetch8(bus);
        ModRm {
            byte,
            mode: byte >> 6,
            reg: (byte >> 3) & 0x07,
            rm: byte & 0x07,
        }
    }

    fn decode_rm_operand(
        &mut self,
        modrm: ModRm,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) -> Operand {
        if modrm.mode == 0b11 {
            return Operand::Register(modrm.rm);
        }

        let (base, uses_bp) = match modrm.rm {
            0 => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_SI)),
                false,
            ),
            1 => (
                self.get_reg16(REG_BX).wrapping_add(self.get_reg16(REG_DI)),
                false,
            ),
            2 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_SI)),
                true,
            ),
            3 => (
                self.get_reg16(REG_BP).wrapping_add(self.get_reg16(REG_DI)),
                true,
            ),
            4 => (self.get_reg16(REG_SI), false),
            5 => (self.get_reg16(REG_DI), false),
            6 if modrm.mode == 0 => (self.fetch16(bus), false),
            6 => (self.get_reg16(REG_BP), true),
            _ => (self.get_reg16(REG_BX), false),
        };

        let offset = match modrm.mode {
            0 => base,
            1 => base.wrapping_add_signed(i16::from(self.fetch8(bus) as i8)),
            2 => base.wrapping_add(self.fetch16(bus)),
            _ => unreachable!("register operands returned above"),
        };
        let default_segment = if uses_bp {
            SegmentRegister::Ss
        } else {
            SegmentRegister::Ds
        };
        Operand::Memory(self.overridden_address(segment_override.or(Some(default_segment)), offset))
    }

    fn read_operand8(&mut self, operand: Operand, bus: &mut Bus) -> u8 {
        match operand {
            Operand::Register(reg) => self.get_reg8(reg),
            Operand::Memory(addr) => bus.read8(addr),
        }
    }

    fn read_operand16(&mut self, operand: Operand, bus: &mut Bus) -> u16 {
        match operand {
            Operand::Register(reg) => self.get_reg16(reg),
            Operand::Memory(addr) => bus.read16(addr),
        }
    }

    fn write_operand8(&mut self, operand: Operand, value: u8, bus: &mut Bus) {
        match operand {
            Operand::Register(reg) => self.set_reg8(reg, value),
            Operand::Memory(addr) => bus.write8(addr, value),
        }
    }

    fn write_operand16(&mut self, operand: Operand, value: u16, bus: &mut Bus) {
        match operand {
            Operand::Register(reg) => self.set_reg16(reg, value),
            Operand::Memory(addr) => bus.write16(addr, value),
        }
    }

    fn get_reg8(&self, reg: u8) -> u8 {
        let word = self.regs[usize::from(reg & 0x03)];
        if reg & 0x04 == 0 {
            word as u8
        } else {
            (word >> 8) as u8
        }
    }

    fn set_reg8(&mut self, reg: u8, value: u8) {
        let slot = &mut self.regs[usize::from(reg & 0x03)];
        if reg & 0x04 == 0 {
            *slot = (*slot & 0xFF00) | u16::from(value);
        } else {
            *slot = (*slot & 0x00FF) | (u16::from(value) << 8);
        }
    }

    fn get_reg16(&self, reg: u8) -> u16 {
        self.regs[usize::from(reg & 0x07)]
    }

    fn set_reg16(&mut self, reg: u8, value: u16) {
        self.regs[usize::from(reg & 0x07)] = value;
    }

    fn overridden_address(&self, segment_override: Option<SegmentRegister>, offset: u16) -> u32 {
        self.physical_address(segment_override.unwrap_or(SegmentRegister::Ds), offset)
    }

    fn physical_address(&self, segment: SegmentRegister, offset: u16) -> u32 {
        ((u32::from(self.segments[segment.index()]) << 4).wrapping_add(u32::from(offset)))
            & ADDRESS_MASK
    }

    fn push16(&mut self, value: u16, bus: &mut Bus) {
        let sp = self.get_reg16(REG_SP).wrapping_sub(2);
        self.set_reg16(REG_SP, sp);
        let addr = self.physical_address(SegmentRegister::Ss, sp);
        bus.write16(addr, value);
    }

    fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let sp = self.get_reg16(REG_SP);
        let addr = self.physical_address(SegmentRegister::Ss, sp);
        let value = bus.read16(addr);
        self.set_reg16(REG_SP, sp.wrapping_add(2));
        value
    }

    fn alu8(&mut self, op: AluOp, lhs: u8, rhs: u8) -> u8 {
        match op {
            AluOp::Add => {
                let result = lhs.wrapping_add(rhs);
                self.set_add_flags8(lhs, rhs, result);
                result
            }
            AluOp::Or => {
                let result = lhs | rhs;
                self.set_logic_flags8(result);
                result
            }
            AluOp::And => {
                let result = lhs & rhs;
                self.set_logic_flags8(result);
                result
            }
            AluOp::Sub | AluOp::Cmp => {
                let result = lhs.wrapping_sub(rhs);
                self.set_sub_flags8(lhs, rhs, result);
                result
            }
            AluOp::Xor => {
                let result = lhs ^ rhs;
                self.set_logic_flags8(result);
                result
            }
        }
    }

    fn alu16(&mut self, op: AluOp, lhs: u16, rhs: u16) -> u16 {
        match op {
            AluOp::Add => {
                let result = lhs.wrapping_add(rhs);
                self.set_add_flags16(lhs, rhs, result);
                result
            }
            AluOp::Or => {
                let result = lhs | rhs;
                self.set_logic_flags16(result);
                result
            }
            AluOp::And => {
                let result = lhs & rhs;
                self.set_logic_flags16(result);
                result
            }
            AluOp::Sub | AluOp::Cmp => {
                let result = lhs.wrapping_sub(rhs);
                self.set_sub_flags16(lhs, rhs, result);
                result
            }
            AluOp::Xor => {
                let result = lhs ^ rhs;
                self.set_logic_flags16(result);
                result
            }
        }
    }

    fn set_logic_flags8(&mut self, result: u8) {
        self.flags &= !(FLAG_CF | FLAG_PF | FLAG_ZF | FLAG_SF | FLAG_OF);
        if result == 0 {
            self.flags |= FLAG_ZF;
        }
        if result & 0x80 != 0 {
            self.flags |= FLAG_SF;
        }
        if result.count_ones() % 2 == 0 {
            self.flags |= FLAG_PF;
        }
        self.flags |= FLAG_FIXED;
    }

    fn set_logic_flags16(&mut self, result: u16) {
        self.flags &= !(FLAG_CF | FLAG_PF | FLAG_ZF | FLAG_SF | FLAG_OF);
        if result == 0 {
            self.flags |= FLAG_ZF;
        }
        if result & 0x8000 != 0 {
            self.flags |= FLAG_SF;
        }
        if (result as u8).count_ones() % 2 == 0 {
            self.flags |= FLAG_PF;
        }
        self.flags |= FLAG_FIXED;
    }

    fn set_add_flags8(&mut self, lhs: u8, rhs: u8, result: u8) {
        self.set_logic_flags8(result);
        if u16::from(lhs) + u16::from(rhs) > 0xFF {
            self.flags |= FLAG_CF;
        }
        if ((lhs ^ result) & (rhs ^ result) & 0x80) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    fn set_add_flags16(&mut self, lhs: u16, rhs: u16, result: u16) {
        self.set_logic_flags16(result);
        if u32::from(lhs) + u32::from(rhs) > 0xFFFF {
            self.flags |= FLAG_CF;
        }
        if ((lhs ^ result) & (rhs ^ result) & 0x8000) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    fn set_sub_flags8(&mut self, lhs: u8, rhs: u8, result: u8) {
        self.set_logic_flags8(result);
        if lhs < rhs {
            self.flags |= FLAG_CF;
        }
        if ((lhs ^ rhs) & (lhs ^ result) & 0x80) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    fn set_sub_flags16(&mut self, lhs: u16, rhs: u16, result: u16) {
        self.set_logic_flags16(result);
        if lhs < rhs {
            self.flags |= FLAG_CF;
        }
        if ((lhs ^ rhs) & (lhs ^ result) & 0x8000) != 0 {
            self.flags |= FLAG_OF;
        }
    }

    fn set_inc_dec_flags16(&mut self, result: u16, decrement: bool) {
        let old_cf = self.flags & FLAG_CF;
        if decrement {
            self.set_sub_flags16(result.wrapping_add(1), 1, result);
        } else {
            self.set_add_flags16(result.wrapping_sub(1), 1, result);
        }
        self.flags = (self.flags & !FLAG_CF) | old_cf | FLAG_FIXED;
    }

    fn condition(&self, condition: u8) -> bool {
        let cf = self.flags & FLAG_CF != 0;
        let pf = self.flags & FLAG_PF != 0;
        let zf = self.flags & FLAG_ZF != 0;
        let sf = self.flags & FLAG_SF != 0;
        let of = self.flags & FLAG_OF != 0;
        match condition {
            0x0 => of,
            0x1 => !of,
            0x2 => cf,
            0x3 => !cf,
            0x4 => zf,
            0x5 => !zf,
            0x6 => cf || zf,
            0x7 => !cf && !zf,
            0x8 => sf,
            0x9 => !sf,
            0xA => pf,
            0xB => !pf,
            0xC => sf != of,
            0xD => sf == of,
            0xE => zf || (sf != of),
            _ => !zf && (sf == of),
        }
    }

    fn add_cycles(&mut self, bus: &mut Bus, cycles: u32) {
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        bus.step_cycles(cycles);
    }

    fn unsupported_opcode(&mut self, opcode: u8) {
        self.last_trap = Some(CpuTrap::UnsupportedOpcode {
            cs: self.segments[SegmentRegister::Cs.index()],
            ip: self.ip.wrapping_sub(1),
            opcode,
        });
        self.state = CpuState::Suspended;
    }

    fn unsupported_form(&mut self, opcode: u8, modrm: u8) {
        self.last_trap = Some(CpuTrap::UnsupportedInstructionForm {
            cs: self.segments[SegmentRegister::Cs.index()],
            ip: self.ip.wrapping_sub(2),
            opcode,
            modrm,
        });
        self.state = CpuState::Suspended;
    }
}

fn alu_group_op(reg: u8) -> Option<AluOp> {
    match reg {
        0 => Some(AluOp::Add),
        1 => Some(AluOp::Or),
        4 => Some(AluOp::And),
        5 => Some(AluOp::Sub),
        6 => Some(AluOp::Xor),
        7 => Some(AluOp::Cmp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::{Cartridge, compute_footer_checksum};

    fn rom_with_reset_code(code: &[u8]) -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        rom[..code.len()].copy_from_slice(code);
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    fn bus_with_code(code: &[u8]) -> Bus {
        Bus::new(Cartridge::load(&rom_with_reset_code(code)).unwrap())
    }

    #[test]
    fn reset_fetches_from_x86_reset_vector() {
        let mut cpu = Cpu::new();
        let mut bus = bus_with_code(&[0x90, 0xF4]);
        let fetched = cpu.step(&mut bus).unwrap();
        assert_eq!(fetched.pc, 0xFFFF0);
        assert_eq!(fetched.opcode, 0xEA);
        assert_eq!(cpu.state, CpuState::Running);
        let fetched = cpu.step(&mut bus).unwrap();
        assert_eq!(fetched.pc, 0xF0000);
        assert_eq!(fetched.opcode, 0x90);
        cpu.step(&mut bus);
        assert_eq!(cpu.state, CpuState::Halted);
    }

    #[test]
    fn mov_immediate_and_out_port_update_bank_register() {
        let mut cpu = Cpu::new();
        let mut bus = bus_with_code(&[0xB0, 0x07, 0xE6, 0xC2, 0xF4]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(bus.cartridge.bank0(), 7);
    }

    #[test]
    fn register_direct_modrm_mov_and_xor_work() {
        let mut cpu = Cpu::new();
        let mut bus = bus_with_code(&[0xB8, 0x34, 0x12, 0x8B, 0xD8, 0x31, 0xC0, 0xF4]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs[REG_BX as usize], 0x1234);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs[REG_AX as usize], 0);
        assert!(cpu.flags & FLAG_ZF != 0);
    }
}

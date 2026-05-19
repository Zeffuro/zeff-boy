use super::bus::Bus;
use super::timing;

mod swi;

pub const RESET_VECTOR: u32 = 0x0800_0000;
pub const CPSR_MODE_MASK: u32 = 0x1F;
pub const CPSR_THUMB: u32 = 1 << 5;
const CPSR_NEGATIVE: u32 = 1 << 31;
const CPSR_ZERO: u32 = 1 << 30;
const CPSR_CARRY: u32 = 1 << 29;
const CPSR_OVERFLOW: u32 = 1 << 28;
const CPSR_IRQ_DISABLE: u32 = 1 << 7;
const CPSR_FIQ_DISABLE: u32 = 1 << 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuState {
    Running,
    Halted,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuMode {
    User,
    Fiq,
    Irq,
    Supervisor,
    Abort,
    Undefined,
    System,
    Unknown(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionSet {
    Arm,
    Thumb,
}

impl InstructionSet {
    pub fn width_bytes(self) -> u8 {
        match self {
            Self::Arm => 4,
            Self::Thumb => 2,
        }
    }

    pub fn pipeline_offset(self) -> u32 {
        match self {
            Self::Arm => 8,
            Self::Thumb => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmInstructionClass {
    BranchExchange,
    Branch,
    BlockDataTransfer,
    SingleDataTransfer,
    DataProcessing,
    Multiply,
    SoftwareInterrupt,
    Coprocessor,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbInstructionClass {
    MoveShiftedRegister,
    AddSubtract,
    Immediate,
    Alu,
    HiRegisterBranchExchange,
    PcRelativeLoad,
    LoadStore,
    LoadStoreHalfword,
    SpRelativeLoad,
    LoadAddress,
    AddOffsetSp,
    PushPop,
    MultipleLoadStore,
    ConditionalBranchOrSwi,
    UnconditionalBranch,
    LongBranchWithLink,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodedInstruction {
    Arm {
        condition: u8,
        class: ArmInstructionClass,
    },
    Thumb {
        class: ThumbInstructionClass,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchedInstruction {
    pub pc: u32,
    pub raw: u32,
    pub instruction_set: InstructionSet,
    pub width_bytes: u8,
    pub fetch_cycles: u32,
    pub decoded: DecodedInstruction,
}

impl CpuMode {
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x1F {
            0x10 => Self::User,
            0x11 => Self::Fiq,
            0x12 => Self::Irq,
            0x13 => Self::Supervisor,
            0x17 => Self::Abort,
            0x1B => Self::Undefined,
            0x1F => Self::System,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cpu {
    pub regs: [u32; 16],
    pub cpsr: u32,
    pub spsr: u32,
    pub cycles: u64,
    pub state: CpuState,
    pub last_opcode_pc: u32,
    pub break_after_next_stub: bool,
    pub next_fetch_sequential: bool,
    pub last_fetch: Option<FetchedInstruction>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            regs: [0; 16],
            cpsr: CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x1F,
            spsr: 0,
            cycles: 0,
            state: CpuState::Running,
            last_opcode_pc: RESET_VECTOR,
            break_after_next_stub: false,
            next_fetch_sequential: false,
            last_fetch: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
        self.regs[15] = RESET_VECTOR;
        self.regs[13] = 0x0300_7F00;
    }

    pub fn pc(&self) -> u32 {
        self.regs[15]
    }

    pub fn set_pc(&mut self, value: u32) {
        self.regs[15] = value;
    }

    pub fn is_suspended(&self) -> bool {
        self.state == CpuState::Suspended
    }

    pub fn suspend(&mut self) {
        self.state = CpuState::Suspended;
    }

    pub fn resume(&mut self) {
        self.state = CpuState::Running;
    }

    pub fn mode(&self) -> CpuMode {
        CpuMode::from_bits((self.cpsr & CPSR_MODE_MASK) as u8)
    }

    pub fn thumb_state(&self) -> bool {
        self.cpsr & CPSR_THUMB != 0
    }

    pub fn instruction_set(&self) -> InstructionSet {
        if self.thumb_state() {
            InstructionSet::Thumb
        } else {
            InstructionSet::Arm
        }
    }

    pub fn visible_pc(&self) -> u32 {
        self.pc()
            .wrapping_add(self.instruction_set().pipeline_offset())
    }

    pub fn request_debug_step(&mut self) {
        self.break_after_next_stub = true;
        self.resume();
    }

    pub(crate) fn step(&mut self, bus: &mut Bus) -> Option<FetchedInstruction> {
        if self.state != CpuState::Running {
            return None;
        }

        let fetched = self.fetch_decode_stub(bus);
        self.execute_fetched(bus, fetched);
        self.cycles = self.cycles.wrapping_add(1);

        if self.break_after_next_stub {
            self.break_after_next_stub = false;
            self.suspend();
        }

        Some(fetched)
    }

    pub(crate) fn fetch_decode_stub(&mut self, bus: &Bus) -> FetchedInstruction {
        let instruction_set = self.instruction_set();
        let width_bytes = instruction_set.width_bytes();
        let pc = align_pc(self.pc(), instruction_set);
        self.last_opcode_pc = pc;

        let raw = match instruction_set {
            InstructionSet::Arm => bus.read32(pc),
            InstructionSet::Thumb => u32::from(bus.read16(pc)),
        };
        let fetch_cycles =
            timing::instruction_fetch_cycles(pc, width_bytes, self.next_fetch_sequential);

        let fetched = FetchedInstruction {
            pc,
            raw,
            instruction_set,
            width_bytes,
            fetch_cycles,
            decoded: decode_stub(raw, instruction_set),
        };

        if self.state == CpuState::Running {
            self.regs[15] = pc.wrapping_add(u32::from(width_bytes));
            self.cycles = self.cycles.wrapping_add(u64::from(fetch_cycles));
            self.next_fetch_sequential = true;
            self.last_fetch = Some(fetched);
        }

        fetched
    }

    fn execute_fetched(&mut self, bus: &mut Bus, fetched: FetchedInstruction) {
        match fetched.decoded {
            DecodedInstruction::Arm { condition, class } => {
                if !self.condition_passed(condition) {
                    return;
                }

                match class {
                    ArmInstructionClass::BranchExchange => {
                        self.execute_arm_branch_exchange(fetched.raw)
                    }
                    ArmInstructionClass::Branch => self.execute_arm_branch(fetched.pc, fetched.raw),
                    ArmInstructionClass::BlockDataTransfer => {
                        self.execute_arm_block_data_transfer(bus, fetched.pc, fetched.raw)
                    }
                    ArmInstructionClass::SingleDataTransfer => {
                        self.execute_arm_single_data_transfer(bus, fetched.pc, fetched.raw)
                    }
                    ArmInstructionClass::DataProcessing => {
                        self.execute_arm_data_processing(fetched.pc, fetched.raw)
                    }
                    ArmInstructionClass::Multiply => self.execute_arm_multiply(fetched.raw),
                    ArmInstructionClass::SoftwareInterrupt => {
                        self.execute_software_interrupt(bus, fetched.raw & 0x00FF_FFFF)
                    }
                    ArmInstructionClass::Coprocessor | ArmInstructionClass::Unknown => {}
                }
            }
            DecodedInstruction::Thumb { class } => match class {
                ThumbInstructionClass::MoveShiftedRegister => {
                    self.execute_thumb_move_shifted_register(fetched.raw as u16)
                }
                ThumbInstructionClass::AddSubtract => {
                    self.execute_thumb_add_subtract(fetched.raw as u16)
                }
                ThumbInstructionClass::Immediate => {
                    self.execute_thumb_immediate(fetched.raw as u16)
                }
                ThumbInstructionClass::Alu => self.execute_thumb_alu(fetched.raw as u16),
                ThumbInstructionClass::HiRegisterBranchExchange => {
                    self.execute_thumb_hi_register_branch_exchange(fetched.pc, fetched.raw as u16)
                }
                ThumbInstructionClass::PcRelativeLoad => {
                    self.execute_thumb_pc_relative_load(bus, fetched.pc, fetched.raw as u16)
                }
                ThumbInstructionClass::LoadStore => {
                    self.execute_thumb_load_store(bus, fetched.raw as u16)
                }
                ThumbInstructionClass::LoadStoreHalfword => {
                    self.execute_thumb_load_store_halfword(bus, fetched.raw as u16)
                }
                ThumbInstructionClass::SpRelativeLoad => {
                    self.execute_thumb_sp_relative_load(bus, fetched.raw as u16)
                }
                ThumbInstructionClass::LoadAddress => {
                    self.execute_thumb_load_address(fetched.pc, fetched.raw as u16)
                }
                ThumbInstructionClass::AddOffsetSp => {
                    self.execute_thumb_add_offset_sp(fetched.raw as u16)
                }
                ThumbInstructionClass::PushPop => {
                    self.execute_thumb_push_pop(bus, fetched.raw as u16)
                }
                ThumbInstructionClass::MultipleLoadStore => {
                    self.execute_thumb_multiple_load_store(bus, fetched.raw as u16)
                }
                ThumbInstructionClass::ConditionalBranchOrSwi => {
                    if (fetched.raw as u16) & 0x0F00 == 0x0F00 {
                        self.execute_software_interrupt(bus, fetched.raw & 0xFF);
                    } else {
                        self.execute_thumb_conditional_branch(fetched.pc, fetched.raw as u16)
                    }
                }
                ThumbInstructionClass::UnconditionalBranch => {
                    self.execute_thumb_unconditional_branch(fetched.pc, fetched.raw as u16)
                }
                ThumbInstructionClass::LongBranchWithLink => {
                    self.execute_thumb_long_branch_with_link(fetched.pc, fetched.raw as u16)
                }
                ThumbInstructionClass::Unknown => {}
            },
        }
    }

    fn set_nz(&mut self, value: u32) {
        self.cpsr &= !(CPSR_NEGATIVE | CPSR_ZERO);
        if value & 0x8000_0000 != 0 {
            self.cpsr |= CPSR_NEGATIVE;
        }
        if value == 0 {
            self.cpsr |= CPSR_ZERO;
        }
    }

    fn set_nzc(&mut self, value: u32, carry: bool) {
        self.set_nz(value);
        self.cpsr &= !CPSR_CARRY;
        if carry {
            self.cpsr |= CPSR_CARRY;
        }
    }

    fn set_nzcv(&mut self, value: u32, carry: bool, overflow: bool) {
        self.set_nzc(value, carry);
        self.cpsr &= !CPSR_OVERFLOW;
        if overflow {
            self.cpsr |= CPSR_OVERFLOW;
        }
    }

    fn carry(&self) -> bool {
        self.cpsr & CPSR_CARRY != 0
    }

    fn reg_read_arm(&self, reg: usize, pc: u32) -> u32 {
        if reg == 15 {
            pc.wrapping_add(8)
        } else {
            self.regs[reg]
        }
    }

    fn reg_read_thumb(&self, reg: usize, pc: u32) -> u32 {
        if reg == 15 {
            pc.wrapping_add(4)
        } else {
            self.regs[reg]
        }
    }

    fn write_reg(&mut self, reg: usize, value: u32, thumb: bool) {
        if reg == 15 {
            self.set_pc(if thumb { value & !1 } else { value & !3 });
            self.next_fetch_sequential = false;
        } else {
            self.regs[reg] = value;
        }
    }

    fn condition_passed(&self, condition: u8) -> bool {
        let negative = self.cpsr & CPSR_NEGATIVE != 0;
        let zero = self.cpsr & CPSR_ZERO != 0;
        let carry = self.cpsr & CPSR_CARRY != 0;
        let overflow = self.cpsr & CPSR_OVERFLOW != 0;

        match condition & 0xF {
            0x0 => zero,
            0x1 => !zero,
            0x2 => carry,
            0x3 => !carry,
            0x4 => negative,
            0x5 => !negative,
            0x6 => overflow,
            0x7 => !overflow,
            0x8 => carry && !zero,
            0x9 => !carry || zero,
            0xA => negative == overflow,
            0xB => negative != overflow,
            0xC => !zero && negative == overflow,
            0xD => zero || negative != overflow,
            0xE => true,
            _ => false,
        }
    }

    fn execute_arm_data_processing(&mut self, pc: u32, raw: u32) {
        let opcode = ((raw >> 21) & 0xF) as u8;
        let set_flags = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let lhs = self.reg_read_arm(rn, pc);
        let (rhs, shifter_carry) = if raw & (1 << 25) != 0 {
            arm_immediate_operand(raw, self.carry())
        } else {
            self.arm_register_operand(raw, pc)
        };

        let write_result = !matches!(opcode, 0x8..=0xB);
        let result = match opcode {
            0x0 | 0x8 => lhs & rhs,
            0x1 | 0x9 => lhs ^ rhs,
            0x2 | 0xA => lhs.wrapping_sub(rhs),
            0x3 => rhs.wrapping_sub(lhs),
            0x4 | 0xB => lhs.wrapping_add(rhs),
            0x5 => lhs.wrapping_add(rhs).wrapping_add(u32::from(self.carry())),
            0x6 => lhs.wrapping_sub(rhs).wrapping_sub(u32::from(!self.carry())),
            0x7 => rhs.wrapping_sub(lhs).wrapping_sub(u32::from(!self.carry())),
            0xC => lhs | rhs,
            0xD => rhs,
            0xE => lhs & !rhs,
            0xF => !rhs,
            _ => return,
        };

        if set_flags || !write_result {
            match opcode {
                0x0 | 0x1 | 0x8 | 0x9 | 0xC | 0xD | 0xE | 0xF => {
                    self.set_nzc(result, shifter_carry)
                }
                0x2 | 0xA => self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result)),
                0x3 => self.set_nzcv(result, rhs >= lhs, sub_overflow(rhs, lhs, result)),
                0x4 | 0xB => {
                    let (sum, carry) = lhs.overflowing_add(rhs);
                    self.set_nzcv(sum, carry, add_overflow(lhs, rhs, sum));
                }
                0x5 => {
                    let carry_in = u32::from(self.carry());
                    let (sum1, c1) = lhs.overflowing_add(rhs);
                    let (sum2, c2) = sum1.overflowing_add(carry_in);
                    self.set_nzcv(sum2, c1 || c2, add_overflow(lhs, rhs, sum1));
                }
                0x6 => {
                    let borrow = u32::from(!self.carry());
                    let rhs = rhs.wrapping_add(borrow);
                    self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result));
                }
                0x7 => {
                    let borrow = u32::from(!self.carry());
                    let lhs2 = lhs.wrapping_add(borrow);
                    self.set_nzcv(result, rhs >= lhs2, sub_overflow(rhs, lhs2, result));
                }
                _ => {}
            }
        }

        if write_result {
            self.write_reg(rd, result, false);
        }
    }

    fn arm_register_operand(&self, raw: u32, pc: u32) -> (u32, bool) {
        let rm = (raw & 0xF) as usize;
        let value = self.reg_read_arm(rm, pc);
        let shift_type = (raw >> 5) & 0x3;
        let by_register = raw & (1 << 4) != 0;
        let amount = if by_register {
            let rs = ((raw >> 8) & 0xF) as usize;
            self.reg_read_arm(rs, pc) & 0xFF
        } else {
            (raw >> 7) & 0x1F
        };
        shift_operand(value, shift_type, amount, by_register, self.carry())
    }

    fn execute_arm_multiply(&mut self, raw: u32) {
        let accumulate = raw & (1 << 21) != 0;
        let set_flags = raw & (1 << 20) != 0;
        let rd = ((raw >> 16) & 0xF) as usize;
        let rn = ((raw >> 12) & 0xF) as usize;
        let rs = ((raw >> 8) & 0xF) as usize;
        let rm = (raw & 0xF) as usize;
        let mut result = self.regs[rm].wrapping_mul(self.regs[rs]);
        if accumulate {
            result = result.wrapping_add(self.regs[rn]);
        }
        self.regs[rd] = result;
        if set_flags {
            self.set_nz(result);
        }
    }

    fn execute_arm_single_data_transfer(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        if raw & 0x0E00_0090 == 0x0000_0090 && raw & 0x60 != 0 {
            self.execute_arm_halfword_data_transfer(bus, pc, raw);
            return;
        }

        let immediate_register = raw & (1 << 25) != 0;
        let pre_index = raw & (1 << 24) != 0;
        let add = raw & (1 << 23) != 0;
        let byte = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let base = self.reg_read_arm(rn, pc);
        let offset = if immediate_register {
            self.arm_register_operand(raw, pc).0
        } else {
            raw & 0xFFF
        };
        let offset_base = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre_index { offset_base } else { base };

        if load {
            let value = if byte {
                u32::from(bus.read8(addr))
            } else {
                rotate_right(bus.read32(addr), (addr & 3) * 8)
            };
            self.write_reg(rd, value, false);
        } else if byte {
            bus.write8(addr, self.reg_read_arm(rd, pc) as u8);
        } else {
            bus.write32(
                addr,
                self.reg_read_arm(rd, pc)
                    .wrapping_add(if rd == 15 { 4 } else { 0 }),
            );
        }

        if !pre_index || writeback {
            self.regs[rn] = offset_base;
        }
    }

    fn execute_arm_halfword_data_transfer(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        let pre_index = raw & (1 << 24) != 0;
        let add = raw & (1 << 23) != 0;
        let immediate = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let mode = (raw >> 5) & 0x3;
        let base = self.reg_read_arm(rn, pc);
        let offset = if immediate {
            ((raw >> 4) & 0xF0) | (raw & 0xF)
        } else {
            self.reg_read_arm((raw & 0xF) as usize, pc)
        };
        let offset_base = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre_index { offset_base } else { base };

        if load {
            let value = match mode {
                0b01 => u32::from(bus.read16(addr)),
                0b10 => sign_extend(u32::from(bus.read8(addr)), 8) as u32,
                0b11 => sign_extend(u32::from(bus.read16(addr)), 16) as u32,
                _ => 0,
            };
            self.write_reg(rd, value, false);
        } else if mode == 0b01 {
            bus.write16(addr, self.reg_read_arm(rd, pc) as u16);
        }

        if !pre_index || writeback {
            self.regs[rn] = offset_base;
        }
    }

    fn execute_arm_block_data_transfer(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        let pre = raw & (1 << 24) != 0;
        let up = raw & (1 << 23) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let reg_list = raw & 0xFFFF;
        let count = reg_list.count_ones().max(1);
        let base = self.regs[rn];
        let mut addr = match (up, pre) {
            (true, false) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(4 * (count - 1)),
            (false, true) => base.wrapping_sub(4 * count),
        };

        if reg_list == 0 {
            if load {
                self.write_reg(15, bus.read32(addr), false);
            } else {
                bus.write32(addr, pc.wrapping_add(12));
            }
        } else {
            for reg in 0..16 {
                if reg_list & (1 << reg) == 0 {
                    continue;
                }
                if load {
                    let value = bus.read32(addr);
                    self.write_reg(reg, value, false);
                } else {
                    let mut value = self.reg_read_arm(reg, pc);
                    if reg == 15 {
                        value = value.wrapping_add(4);
                    }
                    bus.write32(addr, value);
                }
                addr = addr.wrapping_add(4);
            }
        }

        if writeback {
            self.regs[rn] = if up {
                base.wrapping_add(4 * count)
            } else {
                base.wrapping_sub(4 * count)
            };
        }
    }

    fn execute_arm_branch_exchange(&mut self, raw: u32) {
        let rm = (raw & 0xF) as usize;
        self.branch_exchange(self.regs[rm]);
    }

    fn execute_arm_branch(&mut self, pc: u32, raw: u32) {
        let offset = sign_extend(raw & 0x00FF_FFFF, 24) << 2;
        if raw & (1 << 24) != 0 {
            self.regs[14] = pc.wrapping_add(4);
        }
        self.set_pc(pc.wrapping_add(8).wrapping_add_signed(offset));
        self.next_fetch_sequential = false;
    }

    fn execute_thumb_move_shifted_register(&mut self, raw: u16) {
        let op = (raw >> 11) & 0x3;
        let offset = u32::from((raw >> 6) & 0x1F);
        let rs = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let value = self.regs[rs];
        let (result, carry) = match op {
            0 => shift_operand(value, 0, offset, false, self.carry()),
            1 => shift_operand(value, 1, offset, false, self.carry()),
            2 => shift_operand(value, 2, offset, false, self.carry()),
            _ => return,
        };
        self.regs[rd] = result;
        self.set_nzc(result, carry);
    }

    fn execute_thumb_add_subtract(&mut self, raw: u16) {
        let immediate = raw & (1 << 10) != 0;
        let subtract = raw & (1 << 9) != 0;
        let rn = ((raw >> 6) & 0x7) as usize;
        let rs = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let lhs = self.regs[rs];
        let rhs = if immediate {
            u32::from((raw >> 6) & 0x7)
        } else {
            self.regs[rn]
        };
        let result = if subtract {
            lhs.wrapping_sub(rhs)
        } else {
            lhs.wrapping_add(rhs)
        };
        self.regs[rd] = result;
        if subtract {
            self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result));
        } else {
            let (_, carry) = lhs.overflowing_add(rhs);
            self.set_nzcv(result, carry, add_overflow(lhs, rhs, result));
        }
    }

    fn execute_thumb_immediate(&mut self, raw: u16) {
        let op = (raw >> 11) & 0x3;
        let rd = ((raw >> 8) & 0x7) as usize;
        let imm = u32::from(raw & 0xFF);
        match op {
            0 => {
                self.regs[rd] = imm;
                self.set_nz(imm);
            }
            1 => {
                let lhs = self.regs[rd];
                let result = lhs.wrapping_sub(imm);
                self.set_nzcv(result, lhs >= imm, sub_overflow(lhs, imm, result));
            }
            2 => {
                let lhs = self.regs[rd];
                let result = lhs.wrapping_add(imm);
                self.regs[rd] = result;
                let (_, carry) = lhs.overflowing_add(imm);
                self.set_nzcv(result, carry, add_overflow(lhs, imm, result));
            }
            3 => {
                let lhs = self.regs[rd];
                let result = lhs.wrapping_sub(imm);
                self.regs[rd] = result;
                self.set_nzcv(result, lhs >= imm, sub_overflow(lhs, imm, result));
            }
            _ => {}
        }
    }

    fn execute_thumb_alu(&mut self, raw: u16) {
        let op = (raw >> 6) & 0xF;
        let rs = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let lhs = self.regs[rd];
        let rhs = self.regs[rs];
        let old_carry = self.carry();
        let (result, write, flags) = match op {
            0x0 => (lhs & rhs, true, Some((false, false))),
            0x1 => (lhs ^ rhs, true, Some((false, false))),
            0x2 => {
                let (v, c) = shift_operand(lhs, 0, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x3 => {
                let (v, c) = shift_operand(lhs, 1, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x4 => {
                let (v, c) = shift_operand(lhs, 2, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x5 => {
                let carry_in = u32::from(old_carry);
                let (s1, c1) = lhs.overflowing_add(rhs);
                let (s2, c2) = s1.overflowing_add(carry_in);
                (s2, true, Some((c1 || c2, add_overflow(lhs, rhs, s1))))
            }
            0x6 => {
                let rhs2 = rhs.wrapping_add(u32::from(!old_carry));
                (
                    lhs.wrapping_sub(rhs2),
                    true,
                    Some((lhs >= rhs2, sub_overflow(lhs, rhs2, lhs.wrapping_sub(rhs2)))),
                )
            }
            0x7 => {
                let (v, c) = shift_operand(lhs, 3, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x8 => (lhs & rhs, false, Some((old_carry, false))),
            0x9 => (
                0u32.wrapping_sub(rhs),
                true,
                Some((rhs == 0, sub_overflow(0, rhs, 0u32.wrapping_sub(rhs)))),
            ),
            0xA => (
                lhs.wrapping_sub(rhs),
                false,
                Some((lhs >= rhs, sub_overflow(lhs, rhs, lhs.wrapping_sub(rhs)))),
            ),
            0xB => {
                let result = lhs.wrapping_add(rhs);
                let (_, carry) = lhs.overflowing_add(rhs);
                (result, false, Some((carry, add_overflow(lhs, rhs, result))))
            }
            0xC => (lhs | rhs, true, Some((old_carry, false))),
            0xD => (lhs.wrapping_mul(rhs), true, Some((old_carry, false))),
            0xE => (lhs & !rhs, true, Some((old_carry, false))),
            0xF => (!rhs, true, Some((old_carry, false))),
            _ => return,
        };
        if write {
            self.regs[rd] = result;
        }
        if let Some((carry, overflow)) = flags {
            if matches!(op, 0x5 | 0x6 | 0x9 | 0xA | 0xB) {
                self.set_nzcv(result, carry, overflow);
            } else {
                self.set_nzc(result, carry);
            }
        }
    }

    fn execute_thumb_conditional_branch(&mut self, pc: u32, raw: u16) {
        let condition = ((raw >> 8) & 0xF) as u8;
        if condition >= 0xE || !self.condition_passed(condition) {
            return;
        }

        let offset = sign_extend(u32::from(raw & 0x00FF), 8) << 1;
        self.set_pc(pc.wrapping_add(4).wrapping_add_signed(offset));
        self.next_fetch_sequential = false;
    }

    fn execute_thumb_unconditional_branch(&mut self, pc: u32, raw: u16) {
        let offset = sign_extend(u32::from(raw & 0x07FF), 11) << 1;
        self.set_pc(pc.wrapping_add(4).wrapping_add_signed(offset));
        self.next_fetch_sequential = false;
    }

    fn execute_thumb_long_branch_with_link(&mut self, pc: u32, raw: u16) {
        let offset = u32::from(raw & 0x07FF);
        if raw & 0x0800 == 0 {
            self.regs[14] = pc
                .wrapping_add(4)
                .wrapping_add_signed(sign_extend(offset, 11) << 12);
        } else {
            let target = self.regs[14].wrapping_add(offset << 1);
            self.regs[14] = pc.wrapping_add(2) | 1;
            self.set_pc(target & !1);
            self.next_fetch_sequential = false;
        }
    }

    fn execute_thumb_hi_register_branch_exchange(&mut self, pc: u32, raw: u16) {
        let op = (raw >> 8) & 0x3;
        let h1 = ((raw >> 7) & 1) as usize;
        let h2 = ((raw >> 6) & 1) as usize;
        let rs = ((raw >> 3) & 0x7) as usize | (h2 << 3);
        let rd = (raw & 0x7) as usize | (h1 << 3);
        match op {
            0 => self.write_reg(
                rd,
                self.reg_read_thumb(rd, pc)
                    .wrapping_add(self.reg_read_thumb(rs, pc)),
                true,
            ),
            1 => {
                let lhs = self.reg_read_thumb(rd, pc);
                let rhs = self.reg_read_thumb(rs, pc);
                let result = lhs.wrapping_sub(rhs);
                self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result));
            }
            2 => self.write_reg(rd, self.reg_read_thumb(rs, pc), true),
            3 => self.branch_exchange(self.reg_read_thumb(rs, pc)),
            _ => {}
        }
    }

    fn execute_thumb_pc_relative_load(&mut self, bus: &Bus, pc: u32, raw: u16) {
        let rd = ((raw >> 8) & 0x7) as usize;
        let addr = (pc.wrapping_add(4) & !3).wrapping_add(u32::from(raw & 0xFF) << 2);
        self.regs[rd] = bus.read32(addr);
    }

    fn execute_thumb_load_store(&mut self, bus: &mut Bus, raw: u16) {
        let rb = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        if raw & 0xF000 == 0x5000 {
            let ro = ((raw >> 6) & 0x7) as usize;
            let addr = self.regs[rb].wrapping_add(self.regs[ro]);
            match (raw >> 9) & 0x7 {
                0b000 => bus.write32(addr, self.regs[rd]),
                0b001 => bus.write16(addr, self.regs[rd] as u16),
                0b010 => bus.write8(addr, self.regs[rd] as u8),
                0b011 => self.regs[rd] = sign_extend(u32::from(bus.read8(addr)), 8) as u32,
                0b100 => self.regs[rd] = rotate_right(bus.read32(addr), (addr & 3) * 8),
                0b101 => self.regs[rd] = u32::from(bus.read16(addr)),
                0b110 => self.regs[rd] = u32::from(bus.read8(addr)),
                0b111 => self.regs[rd] = sign_extend(u32::from(bus.read16(addr)), 16) as u32,
                _ => {}
            }
        } else {
            let byte = raw & (1 << 12) != 0;
            let load = raw & (1 << 11) != 0;
            let offset = u32::from((raw >> 6) & 0x1F) << if byte { 0 } else { 2 };
            let addr = self.regs[rb].wrapping_add(offset);
            if load {
                self.regs[rd] = if byte {
                    u32::from(bus.read8(addr))
                } else {
                    rotate_right(bus.read32(addr), (addr & 3) * 8)
                };
            } else if byte {
                bus.write8(addr, self.regs[rd] as u8);
            } else {
                bus.write32(addr, self.regs[rd]);
            }
        }
    }

    fn execute_thumb_load_store_halfword(&mut self, bus: &mut Bus, raw: u16) {
        let rb = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let load = raw & (1 << 11) != 0;
        let addr = self.regs[rb].wrapping_add(u32::from((raw >> 6) & 0x1F) << 1);
        if load {
            self.regs[rd] = u32::from(bus.read16(addr));
        } else {
            bus.write16(addr, self.regs[rd] as u16);
        }
    }

    fn execute_thumb_sp_relative_load(&mut self, bus: &mut Bus, raw: u16) {
        let load = raw & (1 << 11) != 0;
        let rd = ((raw >> 8) & 0x7) as usize;
        let addr = self.regs[13].wrapping_add(u32::from(raw & 0xFF) << 2);
        if load {
            self.regs[rd] = bus.read32(addr);
        } else {
            bus.write32(addr, self.regs[rd]);
        }
    }

    fn execute_thumb_load_address(&mut self, pc: u32, raw: u16) {
        let rd = ((raw >> 8) & 0x7) as usize;
        let offset = u32::from(raw & 0xFF) << 2;
        self.regs[rd] = if raw & (1 << 11) == 0 {
            (pc.wrapping_add(4) & !3).wrapping_add(offset)
        } else {
            self.regs[13].wrapping_add(offset)
        };
    }

    fn execute_thumb_add_offset_sp(&mut self, raw: u16) {
        if raw & 0x0F00 == 0x0000 {
            let offset = u32::from(raw & 0x7F) << 2;
            if raw & (1 << 7) != 0 {
                self.regs[13] = self.regs[13].wrapping_sub(offset);
            } else {
                self.regs[13] = self.regs[13].wrapping_add(offset);
            }
        }
    }

    fn execute_thumb_push_pop(&mut self, bus: &mut Bus, raw: u16) {
        let pop = raw & (1 << 11) != 0;
        let extra = raw & (1 << 8) != 0;
        let list = raw & 0xFF;
        if pop {
            for reg in 0..8 {
                if list & (1 << reg) != 0 {
                    self.regs[reg] = bus.read32(self.regs[13]);
                    self.regs[13] = self.regs[13].wrapping_add(4);
                }
            }
            if extra {
                let pc = bus.read32(self.regs[13]);
                self.regs[13] = self.regs[13].wrapping_add(4);
                self.write_reg(15, pc, true);
            }
        } else {
            let count = list.count_ones() + u32::from(extra);
            self.regs[13] = self.regs[13].wrapping_sub(4 * count);
            let mut addr = self.regs[13];
            for reg in 0..8 {
                if list & (1 << reg) != 0 {
                    bus.write32(addr, self.regs[reg]);
                    addr = addr.wrapping_add(4);
                }
            }
            if extra {
                bus.write32(addr, self.regs[14]);
            }
        }
    }

    fn execute_thumb_multiple_load_store(&mut self, bus: &mut Bus, raw: u16) {
        let load = raw & (1 << 11) != 0;
        let rb = ((raw >> 8) & 0x7) as usize;
        let list = raw & 0xFF;
        let mut addr = self.regs[rb];
        if list == 0 {
            if load {
                self.write_reg(15, bus.read32(addr), true);
            } else {
                bus.write32(addr, self.regs[15].wrapping_add(2));
            }
            self.regs[rb] = self.regs[rb].wrapping_add(0x40);
            return;
        }
        for reg in 0..8 {
            if list & (1 << reg) == 0 {
                continue;
            }
            if load {
                self.regs[reg] = bus.read32(addr);
            } else {
                bus.write32(addr, self.regs[reg]);
            }
            addr = addr.wrapping_add(4);
        }
        self.regs[rb] = addr;
    }

    fn branch_exchange(&mut self, target: u32) {
        if target & 1 != 0 {
            self.cpsr |= CPSR_THUMB;
            self.set_pc(target & !1);
        } else {
            self.cpsr &= !CPSR_THUMB;
            self.set_pc(target & !3);
        }
        self.next_fetch_sequential = false;
    }
}

fn align_pc(pc: u32, instruction_set: InstructionSet) -> u32 {
    match instruction_set {
        InstructionSet::Arm => pc & !3,
        InstructionSet::Thumb => pc & !1,
    }
}

fn arm_immediate_operand(raw: u32, old_carry: bool) -> (u32, bool) {
    let imm = raw & 0xFF;
    let rotate = ((raw >> 8) & 0xF) * 2;
    if rotate == 0 {
        (imm, old_carry)
    } else {
        let value = imm.rotate_right(rotate);
        (value, value & 0x8000_0000 != 0)
    }
}

fn shift_operand(
    value: u32,
    shift_type: u32,
    amount: u32,
    by_register: bool,
    old_carry: bool,
) -> (u32, bool) {
    match shift_type {
        0 => {
            if amount == 0 {
                (value, old_carry)
            } else if amount < 32 {
                (value << amount, value & (1 << (32 - amount)) != 0)
            } else if amount == 32 {
                (0, value & 1 != 0)
            } else {
                (0, false)
            }
        }
        1 => {
            let amount = if !by_register && amount == 0 {
                32
            } else {
                amount
            };
            if amount == 0 {
                (value, old_carry)
            } else if amount < 32 {
                (value >> amount, value & (1 << (amount - 1)) != 0)
            } else if amount == 32 {
                (0, value & 0x8000_0000 != 0)
            } else {
                (0, false)
            }
        }
        2 => {
            let amount = if !by_register && amount == 0 {
                32
            } else {
                amount
            };
            if amount == 0 {
                (value, old_carry)
            } else if amount < 32 {
                (
                    ((value as i32) >> amount) as u32,
                    value & (1 << (amount - 1)) != 0,
                )
            } else {
                let result = if value & 0x8000_0000 != 0 {
                    u32::MAX
                } else {
                    0
                };
                (result, value & 0x8000_0000 != 0)
            }
        }
        3 => {
            if !by_register && amount == 0 {
                let carry = value & 1 != 0;
                ((value >> 1) | (u32::from(old_carry) << 31), carry)
            } else {
                let rot = amount & 31;
                if amount == 0 {
                    (value, old_carry)
                } else if rot == 0 {
                    (value, value & 0x8000_0000 != 0)
                } else {
                    let result = value.rotate_right(rot);
                    (result, result & 0x8000_0000 != 0)
                }
            }
        }
        _ => (value, old_carry),
    }
}

fn rotate_right(value: u32, amount: u32) -> u32 {
    if amount == 0 {
        value
    } else {
        value.rotate_right(amount)
    }
}

fn add_overflow(lhs: u32, rhs: u32, result: u32) -> bool {
    ((lhs ^ result) & (rhs ^ result) & 0x8000_0000) != 0
}

fn sub_overflow(lhs: u32, rhs: u32, result: u32) -> bool {
    ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0
}

fn decode_stub(raw: u32, instruction_set: InstructionSet) -> DecodedInstruction {
    match instruction_set {
        InstructionSet::Arm => DecodedInstruction::Arm {
            condition: ((raw >> 28) & 0xF) as u8,
            class: decode_arm_class(raw),
        },
        InstructionSet::Thumb => DecodedInstruction::Thumb {
            class: decode_thumb_class(raw as u16),
        },
    }
}

fn decode_arm_class(raw: u32) -> ArmInstructionClass {
    if raw & 0x0FFF_FFF0 == 0x012F_FF10 {
        return ArmInstructionClass::BranchExchange;
    }
    if raw & 0x0E00_0090 == 0x0000_0090 && raw & 0x60 != 0 {
        return ArmInstructionClass::SingleDataTransfer;
    }
    if raw & 0x0F00_0000 == 0x0F00_0000 {
        return ArmInstructionClass::SoftwareInterrupt;
    }
    match (raw >> 25) & 0x7 {
        0b101 => ArmInstructionClass::Branch,
        0b100 => ArmInstructionClass::BlockDataTransfer,
        0b010 | 0b011 => ArmInstructionClass::SingleDataTransfer,
        0b000 | 0b001 => {
            if raw & 0x0FC0_00F0 == 0x0000_0090 {
                ArmInstructionClass::Multiply
            } else {
                ArmInstructionClass::DataProcessing
            }
        }
        0b110 | 0b111 => ArmInstructionClass::Coprocessor,
        _ => ArmInstructionClass::Unknown,
    }
}

fn decode_thumb_class(raw: u16) -> ThumbInstructionClass {
    match raw >> 11 {
        0b00000..=0b00010 => ThumbInstructionClass::MoveShiftedRegister,
        0b00011 => ThumbInstructionClass::AddSubtract,
        0b00100..=0b00111 => ThumbInstructionClass::Immediate,
        0b01000 => {
            if raw & 0x0400 != 0 {
                ThumbInstructionClass::HiRegisterBranchExchange
            } else {
                ThumbInstructionClass::Alu
            }
        }
        0b01001 => ThumbInstructionClass::PcRelativeLoad,
        0b01010..=0b01111 => ThumbInstructionClass::LoadStore,
        0b10000..=0b10001 => ThumbInstructionClass::LoadStoreHalfword,
        0b10010..=0b10011 => ThumbInstructionClass::SpRelativeLoad,
        0b10100..=0b10101 => ThumbInstructionClass::LoadAddress,
        0b10110 => {
            if raw & 0x0F00 == 0 {
                ThumbInstructionClass::AddOffsetSp
            } else {
                ThumbInstructionClass::PushPop
            }
        }
        0b10111 => ThumbInstructionClass::PushPop,
        0b11000..=0b11001 => ThumbInstructionClass::MultipleLoadStore,
        0b11010..=0b11011 => ThumbInstructionClass::ConditionalBranchOrSwi,
        0b11100 => ThumbInstructionClass::UnconditionalBranch,
        0b11110..=0b11111 => ThumbInstructionClass::LongBranchWithLink,
        _ => ThumbInstructionClass::Unknown,
    }
}

fn sign_extend(value: u32, bits: u8) -> i32 {
    debug_assert!((1..=31).contains(&bits));
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::bus::Bus;
    use crate::hardware::cartridge::Cartridge;

    fn bus_with_rom(rom_body: &[u8]) -> Bus {
        let mut rom = vec![0; 0xC0];
        rom[..rom_body.len()].copy_from_slice(rom_body);
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        Bus::new(Cartridge::load(&rom).unwrap(), 48_000)
    }

    #[test]
    fn arm_fetch_reads_32_bits_and_advances_pc() {
        let bus = bus_with_rom(&[0x78, 0x56, 0x34, 0xEA]);
        let mut cpu = Cpu::new();
        cpu.reset();
        let fetched = cpu.fetch_decode_stub(&bus);
        assert_eq!(fetched.instruction_set, InstructionSet::Arm);
        assert_eq!(fetched.width_bytes, 4);
        assert_eq!(fetched.raw, 0xEA34_5678);
        assert_eq!(cpu.pc(), RESET_VECTOR + 4);
        assert!(matches!(
            fetched.decoded,
            DecodedInstruction::Arm {
                class: ArmInstructionClass::Branch,
                ..
            }
        ));
    }

    #[test]
    fn thumb_fetch_reads_16_bits_and_advances_pc() {
        let bus = bus_with_rom(&[0x00, 0xE0]);
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr |= CPSR_THUMB;
        let fetched = cpu.fetch_decode_stub(&bus);
        assert_eq!(fetched.instruction_set, InstructionSet::Thumb);
        assert_eq!(fetched.width_bytes, 2);
        assert_eq!(fetched.raw, 0xE000);
        assert_eq!(cpu.pc(), RESET_VECTOR + 2);
        assert!(matches!(
            fetched.decoded,
            DecodedInstruction::Thumb {
                class: ThumbInstructionClass::UnconditionalBranch
            }
        ));
    }

    #[test]
    fn arm_branch_uses_pc_plus_8_base_and_refills_pipeline() {
        let mut bus = bus_with_rom(&0xEA00_0000_u32.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();

        cpu.step(&mut bus);

        assert_eq!(cpu.pc(), RESET_VECTOR + 8);
        assert!(!cpu.next_fetch_sequential);
    }

    #[test]
    fn arm_branch_with_link_sets_lr_to_following_instruction() {
        let mut bus = bus_with_rom(&0xEB00_0001_u32.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();

        cpu.step(&mut bus);

        assert_eq!(cpu.regs[14], RESET_VECTOR + 4);
        assert_eq!(cpu.pc(), RESET_VECTOR + 12);
    }

    #[test]
    fn arm_condition_can_skip_branch() {
        let mut bus = bus_with_rom(&0x0A00_0000_u32.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr &= !CPSR_ZERO;

        cpu.step(&mut bus);

        assert_eq!(cpu.pc(), RESET_VECTOR + 4);
        assert!(cpu.next_fetch_sequential);
    }

    #[test]
    fn arm_data_processing_and_load_store_execute() {
        let mut rom = Vec::new();
        for op in [
            0xE3A0_0001_u32, // mov r0, #1
            0xE280_1002,     // add r1, r0, #2
            0xE58D_1000,     // str r1, [sp]
            0xE59D_2000,     // ldr r2, [sp]
            0xE352_0003,     // cmp r2, #3
        ] {
            rom.extend_from_slice(&op.to_le_bytes());
        }
        let mut bus = bus_with_rom(&rom);
        let mut cpu = Cpu::new();
        cpu.reset();

        for _ in 0..5 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[0], 1);
        assert_eq!(cpu.regs[1], 3);
        assert_eq!(cpu.regs[2], 3);
        assert_ne!(cpu.cpsr & CPSR_ZERO, 0);
    }

    #[test]
    fn arm_branch_exchange_switches_to_thumb() {
        let mut bus = bus_with_rom(&0xE12F_FF1E_u32.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.regs[14] = RESET_VECTOR + 9;

        cpu.step(&mut bus);

        assert!(cpu.thumb_state());
        assert_eq!(cpu.pc(), RESET_VECTOR + 8);
        assert!(!cpu.next_fetch_sequential);
    }

    #[test]
    fn thumb_unconditional_branch_uses_pc_plus_4_base() {
        let mut bus = bus_with_rom(&0xE000_u16.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr |= CPSR_THUMB;

        cpu.step(&mut bus);

        assert_eq!(cpu.pc(), RESET_VECTOR + 4);
        assert!(!cpu.next_fetch_sequential);
    }

    #[test]
    fn thumb_conditional_branch_honors_cpsr_flags() {
        let mut bus = bus_with_rom(&0xD100_u16.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr |= CPSR_THUMB;
        cpu.cpsr &= !CPSR_ZERO;

        cpu.step(&mut bus);

        assert_eq!(cpu.pc(), RESET_VECTOR + 4);
        assert!(!cpu.next_fetch_sequential);
    }

    #[test]
    fn thumb_immediate_and_sp_relative_load_store_execute() {
        let mut rom = Vec::new();
        for op in [
            0x2004_u16, // mov r0, #4
            0x3001,     // add r0, #1
            0x9000,     // str r0, [sp]
            0x9900,     // ldr r1, [sp]
        ] {
            rom.extend_from_slice(&op.to_le_bytes());
        }
        let mut bus = bus_with_rom(&rom);
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr |= CPSR_THUMB;

        for _ in 0..4 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[0], 5);
        assert_eq!(cpu.regs[1], 5);
    }

    #[test]
    fn swi_cpu_set_copies_halfwords() {
        let mut bus = bus_with_rom(&[]);
        bus.write16(0x0200_0000, 0x1234);
        bus.write16(0x0200_0002, 0x5678);
        let mut cpu = Cpu::new();
        cpu.regs[0] = 0x0200_0000;
        cpu.regs[1] = 0x0300_0000;
        cpu.regs[2] = 2;

        cpu.execute_software_interrupt(&mut bus, 0x0B);

        assert_eq!(bus.read16(0x0300_0000), 0x1234);
        assert_eq!(bus.read16(0x0300_0002), 0x5678);
    }

    #[test]
    fn swi_lz77_uncomp_expands_backrefs() {
        let mut bus = bus_with_rom(&[]);
        let data = [0x10, 0x06, 0x00, 0x00, 0x10, b'A', b'B', b'C', 0x00, 0x02];
        for (i, value) in data.into_iter().enumerate() {
            bus.write8(0x0200_0000 + i as u32, value);
        }
        let mut cpu = Cpu::new();
        cpu.regs[0] = 0x0200_0000;
        cpu.regs[1] = 0x0300_0000;

        cpu.execute_software_interrupt(&mut bus, 0x11);

        let out: Vec<u8> = (0..6).map(|i| bus.read8(0x0300_0000 + i)).collect();
        assert_eq!(out, b"ABCABC");
    }

    #[test]
    fn swi_rl_uncomp_expands_runs() {
        let mut bus = bus_with_rom(&[]);
        let data = [0x30, 0x04, 0x00, 0x00, 0x81, 0x7F];
        for (i, value) in data.into_iter().enumerate() {
            bus.write8(0x0200_0000 + i as u32, value);
        }
        let mut cpu = Cpu::new();
        cpu.regs[0] = 0x0200_0000;
        cpu.regs[1] = 0x0300_0000;

        cpu.execute_software_interrupt(&mut bus, 0x14);

        let out: Vec<u8> = (0..4).map(|i| bus.read8(0x0300_0000 + i)).collect();
        assert_eq!(out, [0x7F; 4]);
    }

    #[test]
    fn thumb_long_branch_with_link_sets_target_and_return_address() {
        let mut rom = Vec::new();
        rom.extend_from_slice(&0xF000_u16.to_le_bytes());
        rom.extend_from_slice(&0xF801_u16.to_le_bytes());
        let mut bus = bus_with_rom(&rom);
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr |= CPSR_THUMB;

        cpu.step(&mut bus);
        assert_eq!(cpu.regs[14], RESET_VECTOR + 4);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs[14], RESET_VECTOR + 5);
        assert_eq!(cpu.pc(), RESET_VECTOR + 6);
        assert!(!cpu.next_fetch_sequential);
    }

    #[test]
    fn thumb_branch_exchange_can_return_to_arm() {
        let mut bus = bus_with_rom(&0x4770_u16.to_le_bytes());
        let mut cpu = Cpu::new();
        cpu.reset();
        cpu.cpsr |= CPSR_THUMB;
        cpu.regs[14] = RESET_VECTOR + 8;

        cpu.step(&mut bus);

        assert!(!cpu.thumb_state());
        assert_eq!(cpu.pc(), RESET_VECTOR + 8);
        assert!(!cpu.next_fetch_sequential);
    }
}

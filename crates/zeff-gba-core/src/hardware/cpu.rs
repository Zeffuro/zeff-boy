use super::bus::Bus;
use std::collections::VecDeque;

mod arm;
mod decode;
mod fetch;
mod memory;
mod ops;
mod swi;
mod thumb;

pub const RESET_VECTOR: u32 = 0x0800_0000;
pub const CPSR_MODE_MASK: u32 = 0x1F;
pub const CPSR_THUMB: u32 = 1 << 5;
const CPSR_NEGATIVE: u32 = 1 << 31;
const CPSR_ZERO: u32 = 1 << 30;
const CPSR_CARRY: u32 = 1 << 29;
const CPSR_OVERFLOW: u32 = 1 << 28;
const CPSR_IRQ_DISABLE: u32 = 1 << 7;
const CPSR_FIQ_DISABLE: u32 = 1 << 6;
const BANK_USER_SYSTEM: usize = 0;
const BANK_FIQ: usize = 1;
const BANK_IRQ: usize = 2;
const BANK_SUPERVISOR: usize = 3;
const BANK_ABORT: usize = 4;
const BANK_UNDEFINED: usize = 5;
const CPU_BANKS: usize = 6;
const PREFETCH_QUEUE_LEN: usize = 2;
const BIOS_END: u32 = 0x0000_3FFF;
const POST_STARTUP_BIOS_READ_LATCH: u32 = 0xE129_F000;
const POST_SWI_BIOS_READ_LATCH: u32 = 0xE3A0_2004;

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

pub use decode::{
    ArmInstructionClass, DecodedInstruction, FetchedInstruction, ThumbInstructionClass,
};

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

fn bank_index(mode: CpuMode) -> usize {
    match mode {
        CpuMode::Fiq => BANK_FIQ,
        CpuMode::Irq => BANK_IRQ,
        CpuMode::Supervisor => BANK_SUPERVISOR,
        CpuMode::Abort => BANK_ABORT,
        CpuMode::Undefined => BANK_UNDEFINED,
        CpuMode::User | CpuMode::System | CpuMode::Unknown(_) => BANK_USER_SYSTEM,
    }
}

fn mode_has_spsr(mode: CpuMode) -> bool {
    matches!(
        mode,
        CpuMode::Fiq | CpuMode::Irq | CpuMode::Supervisor | CpuMode::Abort | CpuMode::Undefined
    )
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
    bios_protected_read_latch: u32,
    prefetch_queue: VecDeque<FetchedInstruction>,
    pub(crate) banked_sp: [u32; CPU_BANKS],
    pub(crate) banked_lr: [u32; CPU_BANKS],
    pub(crate) banked_spsr: [u32; CPU_BANKS],
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
            bios_protected_read_latch: POST_STARTUP_BIOS_READ_LATCH,
            prefetch_queue: VecDeque::with_capacity(PREFETCH_QUEUE_LEN),
            banked_sp: [0; CPU_BANKS],
            banked_lr: [0; CPU_BANKS],
            banked_spsr: [0; CPU_BANKS],
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
        self.regs[15] = RESET_VECTOR;
        self.regs[13] = 0x0300_7F00;
        self.banked_sp[BANK_USER_SYSTEM] = self.regs[13];
        self.banked_sp[BANK_IRQ] = 0x0300_7FA0;
        self.banked_sp[BANK_SUPERVISOR] = 0x0300_7FE0;
    }

    pub fn pc(&self) -> u32 {
        self.regs[15]
    }

    pub fn set_pc(&mut self, value: u32) {
        self.regs[15] = value;
        self.flush_prefetch_queue();
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

    pub(crate) fn set_cpsr(&mut self, value: u32) {
        let old_thumb = self.thumb_state();
        self.sync_active_bank();
        self.cpsr = value;
        self.load_active_bank();
        if self.thumb_state() != old_thumb {
            self.flush_prefetch_queue();
        }
    }

    pub(crate) fn sync_active_bank(&mut self) {
        let bank = bank_index(self.mode());
        self.banked_sp[bank] = self.regs[13];
        self.banked_lr[bank] = self.regs[14];
        if mode_has_spsr(self.mode()) {
            self.banked_spsr[bank] = self.spsr;
        }
    }

    fn load_active_bank(&mut self) {
        let bank = bank_index(self.mode());
        self.regs[13] = self.banked_sp[bank];
        self.regs[14] = self.banked_lr[bank];
        self.spsr = if mode_has_spsr(self.mode()) {
            self.banked_spsr[bank]
        } else {
            0
        };
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

    pub(crate) fn try_service_irq(&mut self, interrupt_pending: bool) {
        if !interrupt_pending || self.cpsr & CPSR_IRQ_DISABLE != 0 {
            return;
        }

        let old_cpsr = self.cpsr;
        let return_pc = self.pc().wrapping_add(4);
        self.set_cpsr((old_cpsr & !(CPSR_MODE_MASK | CPSR_THUMB)) | CPSR_IRQ_DISABLE | 0x12);
        self.spsr = old_cpsr;
        self.regs[14] = return_pc;
        self.set_pc(0x0000_0018);
        self.next_fetch_sequential = false;
        self.state = CpuState::Running;
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
                    ArmInstructionClass::MultiplyLong => {
                        self.execute_arm_multiply_long(fetched.raw)
                    }
                    ArmInstructionClass::SingleDataSwap => {
                        self.execute_arm_single_data_swap(bus, fetched.pc, fetched.raw)
                    }
                    ArmInstructionClass::SoftwareInterrupt => {
                        self.execute_software_interrupt(bus, (fetched.raw >> 16) & 0xFF)
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
}

#[cfg(test)]
#[path = "cpu/tests.rs"]
mod tests;

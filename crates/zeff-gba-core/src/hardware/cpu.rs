use super::bus::Bus;
use super::constants::{
    BIOS_END, GAMEPAK0_START, IO_END, IO_LAST_ALIGNED_HALFWORD_ADDR, IO_START, IO_UNUSED_START,
};
use super::timing::DataAccessCursor;
#[cfg(test)]
use super::timing::{CpuInstructionTimeline, TimerIoCompletionEvent};

mod arm;
mod decode;
mod fetch;
mod memory;
mod ops;
mod swi;
mod thumb;
mod transfer;

pub const RESET_VECTOR: u32 = GAMEPAK0_START;
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
const R8_R12_USER_SYSTEM_BANK: usize = 0;
const R8_R12_FIQ_BANK: usize = 1;
const R8_R12_BANKS: usize = 2;
const PREFETCH_QUEUE_LEN: usize = 2;
const POST_BIOS_CPSR: u32 = 0x1F;
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

const EMPTY_FETCHED_INSTRUCTION: FetchedInstruction = FetchedInstruction {
    pc: 0,
    raw: 0,
    instruction_set: InstructionSet::Arm,
    width_bytes: 4,
    fetch_cycles: 0,
    decoded: DecodedInstruction::Arm {
        condition: 0,
        class: ArmInstructionClass::DataProcessing,
    },
};

#[derive(Clone, Debug)]
struct PrefetchPipeline {
    entries: [FetchedInstruction; PREFETCH_QUEUE_LEN],
    len: u8,
}

impl PrefetchPipeline {
    fn new() -> Self {
        Self {
            entries: [EMPTY_FETCHED_INSTRUCTION; PREFETCH_QUEUE_LEN],
            len: 0,
        }
    }

    #[inline]
    fn front(&self) -> Option<&FetchedInstruction> {
        (self.len != 0).then(|| &self.entries[0])
    }

    #[inline]
    fn back(&self) -> Option<&FetchedInstruction> {
        (self.len != 0).then(|| &self.entries[usize::from(self.len - 1)])
    }

    #[inline]
    fn pop_front(&mut self) -> Option<FetchedInstruction> {
        if self.len == 0 {
            return None;
        }

        let fetched = self.entries[0];
        if self.len == PREFETCH_QUEUE_LEN as u8 {
            self.entries[0] = self.entries[1];
        }
        self.len -= 1;
        Some(fetched)
    }

    #[inline]
    fn push_back(&mut self, fetched: FetchedInstruction) {
        debug_assert!(usize::from(self.len) < PREFETCH_QUEUE_LEN);
        self.entries[usize::from(self.len)] = fetched;
        self.len += 1;
    }

    #[inline]
    fn len(&self) -> usize {
        usize::from(self.len)
    }

    #[inline]
    fn clear(&mut self) {
        self.len = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuPipelineEntryState {
    pub pc: u32,
    pub raw: u32,
    pub thumb: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuPipelineState {
    pub entries: [CpuPipelineEntryState; PREFETCH_QUEUE_LEN],
    pub len: u8,
    pub pending_load_internal_cycle: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CpuExecutionPhase {
    #[default]
    Boundary,
    SequentialFetch,
    Execute,
    DataBus,
    LoadInternal,
    Writeback,
    RefillNonSequential,
    RefillSequential,
}

impl CpuExecutionPhase {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::Boundary => 0,
            Self::SequentialFetch => 1,
            Self::Execute => 2,
            Self::DataBus => 3,
            Self::LoadInternal => 4,
            Self::Writeback => 5,
            Self::RefillNonSequential => 6,
            Self::RefillSequential => 7,
        }
    }

    pub(crate) const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Boundary),
            1 => Some(Self::SequentialFetch),
            2 => Some(Self::Execute),
            3 => Some(Self::DataBus),
            4 => Some(Self::LoadInternal),
            5 => Some(Self::Writeback),
            6 => Some(Self::RefillNonSequential),
            7 => Some(Self::RefillSequential),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CpuBusOperation {
    #[default]
    None,
    Read,
    Write,
}

impl CpuBusOperation {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Read => 1,
            Self::Write => 2,
        }
    }

    pub(crate) const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::None),
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuExecutionState {
    pub phase: CpuExecutionPhase,
    pub phase_cycles_remaining: u8,
    pub instruction_active: bool,
    pub active_pc: u32,
    pub active_raw: u32,
    pub active_thumb: bool,
    pub condition_passed: bool,
    pub active_fetch_cycles: u32,
    pub bus_operation: CpuBusOperation,
    pub bus_address: u32,
    pub bus_width: u8,
    pub bus_sequential: bool,
    pub bus_value: u32,
    pub bus_read_latch: u32,
    pub transfer_original_base: u32,
    pub transfer_current_address: u32,
    pub transfer_register_mask: u16,
    pub transfer_next_register: u8,
    pub transfer_first_access: bool,
    pub transfer_force_user: bool,
    pub transfer_exception_return: bool,
    pub transfer_writeback: bool,
    pub writeback_present: bool,
    pub writeback_register: u8,
    pub writeback_value: u32,
    pub refill_target: u32,
    pub refill_thumb: bool,
    pub refill_index: u8,
    pub data_access_elapsed_cycles: u32,
    pub data_access_count: u32,
    pub data_bus_phase_cycles: u32,
}

impl CpuExecutionState {
    fn has_only_active_instruction_fields(self) -> bool {
        let expected = Self {
            phase: self.phase,
            instruction_active: self.instruction_active,
            active_pc: self.active_pc,
            active_raw: self.active_raw,
            active_thumb: self.active_thumb,
            condition_passed: self.condition_passed,
            active_fetch_cycles: self.active_fetch_cycles,
            ..Self::default()
        };
        self == expected
    }

    fn has_only_refill_fields(self) -> bool {
        let expected = Self {
            phase: self.phase,
            instruction_active: self.instruction_active,
            active_pc: self.active_pc,
            active_raw: self.active_raw,
            active_thumb: self.active_thumb,
            condition_passed: self.condition_passed,
            active_fetch_cycles: self.active_fetch_cycles,
            refill_target: self.refill_target,
            refill_thumb: self.refill_thumb,
            refill_index: self.refill_index,
            ..Self::default()
        };
        self == expected
    }
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

fn r8_r12_bank_index(mode: CpuMode) -> usize {
    if matches!(mode, CpuMode::Fiq) {
        R8_R12_FIQ_BANK
    } else {
        R8_R12_USER_SYSTEM_BANK
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
    pub(crate) swi_wait_return_pc: Option<u32>,
    pub(crate) swi_wait_mask: u16,
    pipeline: PrefetchPipeline,
    pending_load_internal_cycle: bool,
    execution_state: CpuExecutionState,
    data_access_cursor: DataAccessCursor,
    instruction_fetch_cycles: u32,
    bus_phase_cycles: u32,
    #[cfg(test)]
    timer_io_completion_events: Vec<TimerIoCompletionEvent>,
    #[cfg(test)]
    instruction_timeline: CpuInstructionTimeline,
    data_access_timing_active: bool,
    hle_data_accesses: bool,
    pub(crate) banked_sp: [u32; CPU_BANKS],
    pub(crate) banked_lr: [u32; CPU_BANKS],
    pub(crate) banked_spsr: [u32; CPU_BANKS],
    pub(crate) banked_r8_r12: [[u32; 5]; R8_R12_BANKS],
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
            swi_wait_return_pc: None,
            swi_wait_mask: 0,
            pipeline: PrefetchPipeline::new(),
            pending_load_internal_cycle: false,
            execution_state: CpuExecutionState::default(),
            data_access_cursor: DataAccessCursor::default(),
            instruction_fetch_cycles: 0,
            bus_phase_cycles: 0,
            #[cfg(test)]
            timer_io_completion_events: Vec::new(),
            #[cfg(test)]
            instruction_timeline: CpuInstructionTimeline::default(),
            data_access_timing_active: false,
            hle_data_accesses: false,
            banked_sp: [0; CPU_BANKS],
            banked_lr: [0; CPU_BANKS],
            banked_spsr: [0; CPU_BANKS],
            banked_r8_r12: [[0; 5]; R8_R12_BANKS],
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
        self.cpsr = POST_BIOS_CPSR;
        self.regs[15] = RESET_VECTOR;
        self.regs[13] = 0x0300_7F00;
        self.banked_sp[BANK_USER_SYSTEM] = self.regs[13];
        self.banked_sp[BANK_IRQ] = 0x0300_7FA0;
        self.banked_sp[BANK_SUPERVISOR] = 0x0300_7FE0;
    }

    pub(crate) fn reset_with_bios(&mut self) {
        *self = Self::new();
        self.cpsr = CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE | 0x13;
        self.regs[15] = 0;
        self.last_opcode_pc = 0;
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

    pub(crate) fn begin_guest_call(&mut self, target: u32, thumb: bool) -> (u32, u32, u32) {
        let state = (self.pc(), self.regs[14], self.cpsr);
        self.regs[14] = self.pc() | u32::from(self.thumb_state());
        self.cpsr |= CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE;
        self.cpsr = if thumb {
            self.cpsr | CPSR_THUMB
        } else {
            self.cpsr & !CPSR_THUMB
        };
        self.set_pc(if thumb { target & !1 } else { target & !3 });
        self.resume();
        state
    }

    pub(crate) fn finish_guest_call(&mut self, saved_lr: u32, saved_cpsr: u32) {
        self.regs[14] = saved_lr;
        let interrupt_mask = CPSR_IRQ_DISABLE | CPSR_FIQ_DISABLE;
        self.cpsr = (self.cpsr & !interrupt_mask) | (saved_cpsr & interrupt_mask);
        self.suspend();
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
        let r8_bank = r8_r12_bank_index(self.mode());
        self.banked_r8_r12[r8_bank].copy_from_slice(&self.regs[8..13]);
        self.banked_sp[bank] = self.regs[13];
        self.banked_lr[bank] = self.regs[14];
        if mode_has_spsr(self.mode()) {
            self.banked_spsr[bank] = self.spsr;
        }
    }

    fn load_active_bank(&mut self) {
        let bank = bank_index(self.mode());
        let r8_bank = r8_r12_bank_index(self.mode());
        self.regs[8..13].copy_from_slice(&self.banked_r8_r12[r8_bank]);
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
        loop {
            if let Some(result) = self.step_cpu_phase(bus) {
                return result;
            }
        }
    }

    fn step_cpu_phase(&mut self, bus: &mut Bus) -> Option<Option<FetchedInstruction>> {
        match self.execution_state.phase {
            CpuExecutionPhase::Boundary => self.begin_instruction_phase(bus),
            CpuExecutionPhase::SequentialFetch => self.step_sequential_fetch_phase(bus),
            CpuExecutionPhase::Execute => self.step_execute_phase(bus),
            CpuExecutionPhase::DataBus => self.step_data_bus_phase(bus),
            CpuExecutionPhase::LoadInternal => self.step_load_internal_phase(bus),
            CpuExecutionPhase::Writeback => self.step_writeback_phase(bus),
            CpuExecutionPhase::RefillNonSequential => self.step_refill_nonsequential_phase(bus),
            CpuExecutionPhase::RefillSequential => self.step_refill_sequential_phase(bus),
        }
    }

    fn begin_instruction_phase(&mut self, bus: &mut Bus) -> Option<Option<FetchedInstruction>> {
        if self.state != CpuState::Running {
            return Some(None);
        }
        if self.swi_wait_return_pc == Some(self.pc()) {
            self.complete_swi_wait(bus);
        }
        self.execution_state.phase = CpuExecutionPhase::SequentialFetch;
        None
    }

    fn step_sequential_fetch_phase(&mut self, bus: &mut Bus) -> Option<Option<FetchedInstruction>> {
        let fetched = self.fetch_decode_stub(bus);
        self.execution_state = CpuExecutionState {
            phase: CpuExecutionPhase::Execute,
            instruction_active: true,
            active_pc: fetched.pc,
            active_raw: fetched.raw,
            active_thumb: fetched.instruction_set == InstructionSet::Thumb,
            condition_passed: self.fetched_condition_passed(fetched),
            active_fetch_cycles: fetched.fetch_cycles,
            ..CpuExecutionState::default()
        };
        None
    }

    fn step_execute_phase(&mut self, bus: &mut Bus) -> Option<Option<FetchedInstruction>> {
        let fetched = self.active_fetched_instruction();
        let condition_passed = self.execution_state.condition_passed;
        let instruction_start_cycle = self.cycles.wrapping_sub(u64::from(fetched.fetch_cycles));
        let base_cycles = instruction_base_cycles(fetched, condition_passed);
        self.begin_data_access_timing(fetched.fetch_cycles);
        if condition_passed && self.begin_staged_transfer(fetched) {
            self.cycles = self.cycles.wrapping_add(u64::from(base_cycles));
            self.sync_staged_timing_state();
            return None;
        }
        self.execute_fetched(bus, fetched);
        self.cycles = self.cycles.wrapping_add(u64::from(base_cycles));
        self.finish_data_access_timing(
            bus,
            self.cycles
                .wrapping_sub(instruction_start_cycle)
                .min(u64::from(u32::MAX)) as u32,
        );
        if instruction_has_load_final_internal_cycle(fetched, condition_passed) {
            self.pending_load_internal_cycle = true;
        }
        if self.state == CpuState::Running && self.pipeline.len() == 0 {
            self.begin_refill_phases();
            return None;
        }
        self.complete_active_instruction(bus)
    }

    fn begin_refill_phases(&mut self) {
        self.pipeline.clear();
        self.execution_state.phase = CpuExecutionPhase::RefillNonSequential;
        self.execution_state.phase_cycles_remaining = 0;
        self.execution_state.refill_target = match self.instruction_set() {
            InstructionSet::Arm => self.pc() & !3,
            InstructionSet::Thumb => self.pc() & !1,
        };
        self.execution_state.refill_thumb = self.thumb_state();
        self.execution_state.refill_index = 0;
    }

    fn step_refill_nonsequential_phase(
        &mut self,
        bus: &mut Bus,
    ) -> Option<Option<FetchedInstruction>> {
        let instruction_set = if self.execution_state.refill_thumb {
            InstructionSet::Thumb
        } else {
            InstructionSet::Arm
        };
        let fetched = fetch::fetch_instruction_at(
            bus,
            self.execution_state.refill_target,
            instruction_set,
            instruction_set.width_bytes(),
            false,
        );
        self.track_bios_fetch(fetched);
        self.pipeline.push_back(fetched);
        let pending_internal_cycles = u32::from(self.take_pending_load_internal_cycle());
        let cycles = fetched.fetch_cycles.max(pending_internal_cycles);
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        bus.step_cycles(cycles);
        self.add_refill_timeline_cycles(cycles);
        self.execution_state.phase = CpuExecutionPhase::RefillSequential;
        self.execution_state.refill_index = 1;
        None
    }

    fn step_refill_sequential_phase(
        &mut self,
        bus: &mut Bus,
    ) -> Option<Option<FetchedInstruction>> {
        let instruction_set = if self.execution_state.refill_thumb {
            InstructionSet::Thumb
        } else {
            InstructionSet::Arm
        };
        let address = self
            .execution_state
            .refill_target
            .wrapping_add(u32::from(instruction_set.width_bytes()));
        let fetched = fetch::fetch_instruction_at(
            bus,
            address,
            instruction_set,
            instruction_set.width_bytes(),
            true,
        );
        self.track_bios_fetch(fetched);
        self.pipeline.push_back(fetched);
        self.cycles = self.cycles.wrapping_add(u64::from(fetched.fetch_cycles));
        bus.step_cycles(fetched.fetch_cycles);
        self.add_refill_timeline_cycles(fetched.fetch_cycles);
        self.execution_state.refill_index = 2;
        if self.execution_state.instruction_active {
            self.complete_active_instruction(bus)
        } else {
            self.execution_state = CpuExecutionState::default();
            None
        }
    }

    fn add_refill_timeline_cycles(&mut self, cycles: u32) {
        #[cfg(test)]
        if self.execution_state.instruction_active {
            self.instruction_timeline.total_cycles = self
                .instruction_timeline
                .total_cycles
                .saturating_add(cycles);
            self.instruction_timeline.required_cycles = self
                .instruction_timeline
                .required_cycles
                .saturating_add(cycles);
        }
        #[cfg(not(test))]
        let _ = cycles;
    }

    fn complete_active_instruction(&mut self, bus: &mut Bus) -> Option<Option<FetchedInstruction>> {
        let fetched = self.active_fetched_instruction();
        if bus.take_halt_request() {
            self.state = CpuState::Halted;
        }
        if self.break_after_next_stub {
            self.break_after_next_stub = false;
            self.suspend();
        }
        self.execution_state = CpuExecutionState::default();
        Some(Some(fetched))
    }

    fn active_fetched_instruction(&self) -> FetchedInstruction {
        let instruction_set = if self.execution_state.active_thumb {
            InstructionSet::Thumb
        } else {
            InstructionSet::Arm
        };
        FetchedInstruction {
            pc: self.execution_state.active_pc,
            raw: self.execution_state.active_raw,
            instruction_set,
            width_bytes: instruction_set.width_bytes(),
            fetch_cycles: self.execution_state.active_fetch_cycles,
            decoded: decode::decode_stub(self.execution_state.active_raw, instruction_set),
        }
    }

    #[cfg(test)]
    pub(crate) fn step_cpu_phase_for_test(
        &mut self,
        bus: &mut Bus,
    ) -> Option<Option<FetchedInstruction>> {
        self.step_cpu_phase(bus)
    }

    pub(crate) fn execution_state(&self) -> CpuExecutionState {
        self.execution_state
    }

    pub(crate) fn at_instruction_boundary(&self) -> bool {
        self.execution_state.phase == CpuExecutionPhase::Boundary
    }

    pub(crate) fn set_execution_state(&mut self, state: CpuExecutionState) -> bool {
        let valid = match state.phase {
            CpuExecutionPhase::Boundary => state == CpuExecutionState::default(),
            CpuExecutionPhase::SequentialFetch => {
                state
                    == CpuExecutionState {
                        phase: CpuExecutionPhase::SequentialFetch,
                        ..CpuExecutionState::default()
                    }
            }
            CpuExecutionPhase::Execute => {
                state.instruction_active
                    && state.active_fetch_cycles != 0
                    && state.active_pc & (if state.active_thumb { 1 } else { 3 }) == 0
                    && state.has_only_active_instruction_fields()
                    && self.pipeline.len() == PREFETCH_QUEUE_LEN
                    && self.pc()
                        == state
                            .active_pc
                            .wrapping_add(u32::from(if state.active_thumb { 2u8 } else { 4u8 }))
            }
            CpuExecutionPhase::RefillNonSequential | CpuExecutionPhase::RefillSequential => {
                let expected_index = u8::from(state.phase == CpuExecutionPhase::RefillSequential);
                let expected_pipeline_len = usize::from(expected_index);
                let active_valid = if state.instruction_active {
                    state.active_fetch_cycles != 0
                        && state.active_pc & (if state.active_thumb { 1 } else { 3 }) == 0
                } else {
                    state.active_pc == 0
                        && state.active_raw == 0
                        && !state.active_thumb
                        && !state.condition_passed
                        && state.active_fetch_cycles == 0
                };
                active_valid
                    && state.refill_index == expected_index
                    && state.refill_target & (if state.refill_thumb { 1 } else { 3 }) == 0
                    && state.has_only_refill_fields()
                    && self.pipeline.len() == expected_pipeline_len
                    && self.pc() == state.refill_target
            }
            CpuExecutionPhase::DataBus
            | CpuExecutionPhase::LoadInternal
            | CpuExecutionPhase::Writeback => self.valid_staged_transfer_state(state),
        };
        if !valid {
            return false;
        }
        self.execution_state = state;
        if state.instruction_active {
            self.last_fetch = Some(self.active_fetched_instruction());
        }
        if matches!(
            state.phase,
            CpuExecutionPhase::DataBus
                | CpuExecutionPhase::LoadInternal
                | CpuExecutionPhase::Writeback
        ) {
            if !self.data_access_cursor.set_state(
                state.active_fetch_cycles,
                state.data_access_elapsed_cycles,
                state.data_access_count,
            ) {
                return false;
            }
            self.instruction_fetch_cycles = state.active_fetch_cycles;
            self.bus_phase_cycles = state.data_bus_phase_cycles;
            self.data_access_timing_active = true;
            self.hle_data_accesses = false;
        }
        true
    }

    pub(crate) fn migrate_legacy_execution_state(&mut self) {
        self.execution_state = CpuExecutionState::default();
    }

    #[cfg(test)]
    pub(crate) fn data_access_cycles(&self) -> u32 {
        self.data_access_cursor.elapsed_cycles()
    }

    #[cfg(test)]
    pub(crate) fn timer_io_completion_events(&self) -> &[TimerIoCompletionEvent] {
        &self.timer_io_completion_events
    }

    #[cfg(test)]
    pub(crate) fn instruction_timeline(&self) -> CpuInstructionTimeline {
        self.instruction_timeline
    }

    fn begin_data_access_timing(&mut self, fetch_cycles: u32) {
        self.data_access_cursor.reset(fetch_cycles);
        self.instruction_fetch_cycles = fetch_cycles;
        self.bus_phase_cycles = 0;
        self.data_access_timing_active = true;
        self.hle_data_accesses = false;
        #[cfg(test)]
        {
            self.timer_io_completion_events.clear();
            self.instruction_timeline = CpuInstructionTimeline {
                fetch_cycles,
                total_cycles: fetch_cycles,
                data_access_cycles: 0,
                data_access_count: 0,
                replaced_legacy_data_cycles: 0,
                incremental_non_data_cycles: 0,
                required_cycles: fetch_cycles,
            };
        }
    }

    fn finish_data_access_timing(&mut self, bus: &mut Bus, total_cycles: u32) {
        let data_access_count = self.data_access_cursor.access_count();
        let incremental_cycles = total_cycles.saturating_sub(self.instruction_fetch_cycles);
        let replaced_legacy_data_cycles = if self.hle_data_accesses {
            0
        } else {
            data_access_count.min(incremental_cycles)
        };
        let incremental_non_data_cycles =
            incremental_cycles.saturating_sub(replaced_legacy_data_cycles);
        let data_access_cycles = self.data_access_cursor.elapsed_cycles();
        let required_cycles = self
            .instruction_fetch_cycles
            .saturating_add(incremental_non_data_cycles)
            .saturating_add(data_access_cycles);
        let charged_cycles = total_cycles.max(required_cycles);
        self.cycles = self
            .cycles
            .wrapping_add(u64::from(charged_cycles.saturating_sub(total_cycles)));
        self.advance_bus_phase(bus, charged_cycles);
        #[cfg(test)]
        {
            self.instruction_timeline.total_cycles = total_cycles;
            self.instruction_timeline.data_access_cycles = data_access_cycles;
            self.instruction_timeline.data_access_count = data_access_count;
            self.instruction_timeline.replaced_legacy_data_cycles = replaced_legacy_data_cycles;
            self.instruction_timeline.incremental_non_data_cycles = incremental_non_data_cycles;
            self.instruction_timeline.required_cycles = required_cycles;
        }
        self.data_access_timing_active = false;
    }

    fn advance_bus_phase(&mut self, bus: &mut Bus, target_cycle: u32) {
        let cycles = target_cycle.saturating_sub(self.bus_phase_cycles);
        if cycles != 0 {
            bus.step_cycles(cycles);
            self.bus_phase_cycles = target_cycle;
        }
    }

    pub(crate) fn try_service_irq(&mut self, _bus: &mut Bus, interrupt_pending: bool) -> bool {
        if !self.at_instruction_boundary()
            || !interrupt_pending
            || self.cpsr & CPSR_IRQ_DISABLE != 0
        {
            return false;
        }

        let old_cpsr = self.cpsr;
        let return_pc = self.pc().wrapping_add(4);
        self.set_cpsr((old_cpsr & !(CPSR_MODE_MASK | CPSR_THUMB)) | CPSR_IRQ_DISABLE | 0x12);
        self.spsr = old_cpsr;
        self.regs[14] = return_pc;
        self.set_pc(0x0000_0018);
        self.next_fetch_sequential = false;
        self.state = CpuState::Running;
        self.execution_state = CpuExecutionState::default();
        self.begin_refill_phases();
        true
    }

    fn fetched_condition_passed(&self, fetched: FetchedInstruction) -> bool {
        match fetched.decoded {
            DecodedInstruction::Arm { condition, .. } => {
                condition != 0xF && self.condition_passed(condition)
            }
            DecodedInstruction::Thumb { .. } => true,
        }
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

fn instruction_base_cycles(fetched: FetchedInstruction, condition_passed: bool) -> u32 {
    if !condition_passed {
        return 0;
    }

    match fetched.decoded {
        DecodedInstruction::Arm {
            class: ArmInstructionClass::DataProcessing,
            ..
        } => {
            if fetched.raw & (1 << 25) == 0 && fetched.raw & (1 << 4) != 0 {
                1
            } else {
                0
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::Branch | ArmInstructionClass::BranchExchange,
            ..
        } => 0,
        DecodedInstruction::Arm {
            class: ArmInstructionClass::SingleDataTransfer,
            ..
        } => {
            if fetched.raw & (1 << 20) != 0 {
                2
            } else {
                1
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::BlockDataTransfer,
            ..
        } => {
            let register_count = block_transfer_register_count(fetched.raw);
            if fetched.raw & (1 << 20) != 0 {
                register_count + 1
            } else {
                register_count
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::SingleDataSwap,
            ..
        } => 3,
        DecodedInstruction::Thumb {
            class:
                ThumbInstructionClass::MoveShiftedRegister
                | ThumbInstructionClass::AddSubtract
                | ThumbInstructionClass::Immediate
                | ThumbInstructionClass::LoadAddress
                | ThumbInstructionClass::AddOffsetSp
                | ThumbInstructionClass::HiRegisterBranchExchange
                | ThumbInstructionClass::UnconditionalBranch
                | ThumbInstructionClass::LongBranchWithLink,
        } => 0,
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::Alu,
        } => match (fetched.raw >> 6) & 0xF {
            0x2 | 0x3 | 0x4 | 0x7 | 0xD => 1,
            _ => 0,
        },
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::ConditionalBranchOrSwi,
        } if fetched.raw as u16 & 0x0F00 != 0x0F00 => 0,
        _ => 1,
    }
}

fn block_transfer_register_count(raw: u32) -> u32 {
    let count = (raw & 0xFFFF).count_ones();
    if count == 0 { 16 } else { count }
}

fn instruction_has_load_final_internal_cycle(
    fetched: FetchedInstruction,
    condition_passed: bool,
) -> bool {
    if !condition_passed {
        return false;
    }

    match fetched.decoded {
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::PcRelativeLoad,
        } => true,
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::LoadStore,
        } => {
            let raw = fetched.raw as u16;
            if raw & 0xF000 == 0x5000 {
                (raw >> 9) & 0x7 >= 0b011
            } else {
                raw & (1 << 11) != 0
            }
        }
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::LoadStoreHalfword | ThumbInstructionClass::SpRelativeLoad,
        } => fetched.raw & (1 << 11) != 0,
        _ => false,
    }
}

#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
#[path = "cpu/tests.rs"]
mod tests;

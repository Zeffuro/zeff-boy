use super::super::constants::{EWRAM_END, EWRAM_SIZE, IWRAM_END, IWRAM_SIZE};
use super::super::timing::{self, AccessType, BusRegion, DataAccessCursor};
use super::frame_fetch::FrameFetchWindow;
use super::transfer::{
    NO_WRITEBACK, SingleTransfer, arm_single_load_value, thumb_halfword_load_value,
};
use super::{
    ArmInstructionClass, Bus, CPSR_IRQ_DISABLE, Cpu, CpuBusOperation, CpuState, DataAccessCharge,
    DecodedInstruction, EMPTY_PREFETCHED_INSTRUCTION, FetchedInstruction, InstructionSet,
    PREFETCH_QUEUE_LEN, PrefetchedInstruction, ThumbInstructionClass, fetch,
    instruction_base_cycles,
};

#[derive(Clone, Copy)]
pub(super) struct DirectFetchWindow {
    last_fetch_address: u32,
    width: u32,
    cycles: u32,
}

enum DirectOperation {
    Noop,
    Transfer {
        transfer: SingleTransfer,
        #[cfg(test)]
        cursor: DataAccessCursor,
    },
    BlockTransfer {
        transfer: DirectBlockTransfer,
        #[cfg(test)]
        cursor: DataAccessCursor,
    },
    Branch(u32, [u32; 2], DirectFetchWindow),
}

#[derive(Clone, Copy)]
pub(super) enum DirectPure {
    ArmConditionFailed,
    ArmDataProcessing,
    ArmMultiply,
    ThumbMoveShiftedRegister,
    ThumbAddSubtract,
    ThumbImmediate,
    ThumbAlu,
    ThumbLoadAddress,
    ThumbAddOffsetSp,
}

#[cfg(not(test))]
const _: () = assert!(size_of::<Option<(DirectOperation, u32)>>() <= 36);

#[derive(Clone, Copy)]
enum DirectRam {
    Ewram,
    Iwram,
}

#[derive(Clone, Copy)]
struct DirectBlockTransfer {
    ram: DirectRam,
    start_index: u32,
    register_mask: u16,
    base_register: u8,
    writeback_value: u32,
    load: bool,
    writeback: bool,
}

#[derive(Clone, Copy)]
struct DirectOptions {
    allow_multiply: bool,
    #[cfg(test)]
    allow_mixed: bool,
    #[cfg(test)]
    allow_fast_fetch: bool,
}

impl DirectOptions {
    fn mixed_enabled(self) -> bool {
        #[cfg(test)]
        {
            self.allow_mixed
        }
        #[cfg(not(test))]
        {
            true
        }
    }

    fn fetch_window(
        self,
        bus: &Bus,
        pc: u32,
        instruction_set: InstructionSet,
        cycles: u32,
    ) -> Option<FrameFetchWindow> {
        #[cfg(test)]
        if !self.allow_fast_fetch {
            return None;
        }
        FrameFetchWindow::new(bus, pc, instruction_set, cycles)
    }
}

impl Cpu {
    pub(super) fn run_frame_direct(
        &mut self,
        bus: &mut Bus,
        guard_cycles: u64,
        allow_multiply: bool,
        #[cfg(test)] allow_mixed: bool,
    ) -> Option<(FetchedInstruction, u32)> {
        self.run_frame_direct_with_fetch(
            bus,
            guard_cycles,
            allow_multiply,
            #[cfg(test)]
            allow_mixed,
            #[cfg(test)]
            true,
        )
    }

    pub(super) fn run_frame_direct_with_fetch(
        &mut self,
        bus: &mut Bus,
        guard_cycles: u64,
        allow_multiply: bool,
        #[cfg(test)] allow_mixed: bool,
        #[cfg(test)] allow_fast_fetch: bool,
    ) -> Option<(FetchedInstruction, u32)> {
        if !self.at_instruction_boundary()
            || self.state != CpuState::Running
            || self.break_after_next_stub
            || self.swi_wait_return_pc == Some(self.pc())
            || !bus.frame_service_active()
            || bus.debug_trace_enabled
            || bus.halt_request_pending()
            || (bus.interrupt_ready() && self.cpsr & CPSR_IRQ_DISABLE == 0)
        {
            return None;
        }
        let options = DirectOptions {
            allow_multiply,
            #[cfg(test)]
            allow_mixed,
            #[cfg(test)]
            allow_fast_fetch,
        };
        let instruction_set = self.instruction_set();
        let mut window = DirectFetchWindow::new(self, bus)?;
        let mut fast_fetch = options.fetch_window(bus, self.pc(), instruction_set, window.cycles);
        let budget = bus.frame_cpu_cycle_budget().min(
            guard_cycles
                .saturating_sub(self.cycles)
                .min(u64::from(u32::MAX)) as u32,
        );
        let mut elapsed = 0;
        let mut completed = None;
        let mut instructions = 0u32;
        let mut front = self.pipeline.entries[0];
        let mut back = self.pipeline.entries[1];
        let mut next_fetch_sequential = self.next_fetch_sequential;
        while elapsed < budget {
            if self.swi_wait_return_pc == Some(front.pc) {
                break;
            }
            let lookahead = back.pc.wrapping_add(window.width);
            if lookahead > window.last_fetch_address {
                break;
            }
            let fetch_cycles = window
                .cycles
                .max(u32::from(self.pending_load_internal_cycle));
            let fetched = front.decode(instruction_set, fetch_cycles);
            let condition_passed = self.fetched_condition_passed(fetched);
            let base_cycles = instruction_base_cycles(fetched, condition_passed);
            let (charged, _kind) = if let Some(operation) =
                direct_pure_candidate(fetched, condition_passed, options)
            {
                let cycles = fetch_cycles + base_cycles;
                if cycles > budget - elapsed {
                    break;
                }
                let next = self.prepare_frame_direct_next(
                    bus,
                    fast_fetch,
                    lookahead,
                    window.cycles,
                    fetched,
                );
                front = back;
                back = next;
                next_fetch_sequential = true;
                self.execute_frame_pure(operation, fetched.pc, fetched.raw);
                #[cfg(test)]
                {
                    self.cycles = self.cycles.wrapping_add(u64::from(base_cycles));
                }
                #[cfg(test)]
                let charged = self.settle_data_access_timing(fetch_cycles + base_cycles);
                #[cfg(not(test))]
                let charged = cycles;
                debug_assert_eq!(charged, cycles);
                (charged, 0)
            } else {
                let Some((operation, cycles)) =
                    self.plan_frame_direct(bus, fetched, base_cycles, options)
                else {
                    #[cfg(feature = "profiling")]
                    if instructions != 0 {
                        self.profile_thumb_macro_run_end(fetched);
                    }
                    break;
                };
                if cycles > budget - elapsed {
                    break;
                }
                let next = self.prepare_frame_direct_next(
                    bus,
                    fast_fetch,
                    lookahead,
                    window.cycles,
                    fetched,
                );
                front = back;
                back = next;
                next_fetch_sequential = true;
                let kind = match &operation {
                    DirectOperation::Noop => 0,
                    DirectOperation::Branch(..) => {
                        self.execute_fetched(bus, fetched);
                        2
                    }
                    DirectOperation::Transfer {
                        transfer,
                        #[cfg(test)]
                        cursor,
                    } => {
                        #[cfg(test)]
                        {
                            self.data_access_cursor = *cursor;
                        }
                        self.execute_frame_transfer(bus, fetched, *transfer);
                        1
                    }
                    DirectOperation::BlockTransfer {
                        transfer,
                        #[cfg(test)]
                        cursor,
                    } => {
                        #[cfg(test)]
                        {
                            self.data_access_cursor = *cursor;
                        }
                        self.execute_frame_block_transfer(bus, *transfer);
                        1
                    }
                };
                #[cfg(test)]
                {
                    self.cycles = self.cycles.wrapping_add(u64::from(base_cycles));
                }
                #[cfg(test)]
                let mut charged = self.settle_data_access_timing(fetch_cycles + base_cycles);
                #[cfg(not(test))]
                let mut charged = {
                    let refill_cycles = match &operation {
                        DirectOperation::Branch(_, refill, _) => refill[0] + refill[1],
                        _ => 0,
                    };
                    cycles - refill_cycles
                };
                if let DirectOperation::Branch(target, refill_cycles, next_window) = operation {
                    debug_assert_eq!(self.pc(), target);
                    let entries =
                        self.refill_frame_direct(bus, target, instruction_set, refill_cycles);
                    front = entries[0];
                    back = entries[1];
                    next_fetch_sequential = false;
                    charged += refill_cycles[0] + refill_cycles[1];
                    window = next_window;
                    fast_fetch = options.fetch_window(bus, target, instruction_set, window.cycles);
                }
                debug_assert_eq!(charged, cycles);
                (charged, kind)
            };
            elapsed += charged;
            #[cfg(feature = "profiling")]
            {
                completed = Some(self.complete_instruction(bus, fetched));
            }
            #[cfg(not(feature = "profiling"))]
            {
                completed = Some(fetched);
            }
            instructions += 1;
            #[cfg(feature = "profiling")]
            {
                self.profiling.frame_direct_kinds[_kind] =
                    self.profiling.frame_direct_kinds[_kind].wrapping_add(1);
            }
            if self.state != CpuState::Running {
                break;
            }
        }
        let completed = completed?;
        #[cfg(not(test))]
        {
            self.cycles = self.cycles.wrapping_add(u64::from(elapsed));
        }
        self.pipeline.entries = [front, back];
        self.pipeline.len = PREFETCH_QUEUE_LEN as u8;
        self.pipeline.instruction_set = instruction_set;
        self.last_fetch = Some(completed);
        self.last_opcode_pc = completed.pc;
        self.next_fetch_sequential = next_fetch_sequential;
        // Only the quiet prefix is merged; the crossing instruction stays phase-owned.
        bus.step_cycles(elapsed);
        #[cfg(feature = "profiling")]
        {
            self.profiling.frame_direct_runs = self.profiling.frame_direct_runs.wrapping_add(1);
            self.profiling.frame_direct_instructions = self
                .profiling
                .frame_direct_instructions
                .wrapping_add(u64::from(instructions));
            self.profiling.frame_direct_cycles = self
                .profiling
                .frame_direct_cycles
                .wrapping_add(u64::from(elapsed));
        }
        Some((completed, instructions))
    }

    fn prepare_frame_direct_next(
        &mut self,
        bus: &Bus,
        fast_fetch: Option<FrameFetchWindow>,
        lookahead: u32,
        window_cycles: u32,
        fetched: FetchedInstruction,
    ) -> PrefetchedInstruction {
        let instruction_set = fetched.instruction_set;
        let next = if let Some(window) = fast_fetch.filter(|window| window.contains(lookahead)) {
            self.fetch_frame_direct(bus, window, lookahead)
        } else {
            #[cfg(feature = "profiling")]
            self.profile_instruction_fetch(
                bus,
                lookahead,
                instruction_set,
                instruction_set.width_bytes(),
                true,
            );
            let (next, next_fetch_cycles) = fetch::fetch_prefetched_at(
                bus,
                lookahead,
                instruction_set,
                instruction_set.width_bytes(),
                true,
            );
            self.track_bios_fetch(next.pc, next.raw, instruction_set);
            debug_assert_eq!(next_fetch_cycles, window_cycles);
            next
        };
        self.pending_load_internal_cycle = false;
        self.regs[15] = fetched.pc.wrapping_add(u32::from(fetched.width_bytes));
        #[cfg(test)]
        {
            self.cycles = self.cycles.wrapping_add(u64::from(fetched.fetch_cycles));
        }
        #[cfg(feature = "profiling")]
        self.profile_frame_kernel_instruction(bus, fetched, 0);
        #[cfg(test)]
        self.begin_data_access_timing(fetched.fetch_cycles);
        next
    }

    fn plan_frame_direct(
        &self,
        bus: &Bus,
        fetched: FetchedInstruction,
        base_cycles: u32,
        options: DirectOptions,
    ) -> Option<(DirectOperation, u32)> {
        let fetch_cycles = fetched.fetch_cycles;
        let total = fetch_cycles + base_cycles;
        if !options.mixed_enabled() {
            return None;
        }
        match fetched.decoded {
            DecodedInstruction::Arm {
                class: ArmInstructionClass::SingleDataTransfer,
                ..
            }
            | DecodedInstruction::Thumb {
                class: ThumbInstructionClass::LoadStoreHalfword,
            } => {
                let transfer = match fetched.instruction_set {
                    InstructionSet::Arm => {
                        self.plan_arm_single_transfer(fetched.pc, fetched.raw)?
                    }
                    InstructionSet::Thumb => self.plan_thumb_halfword_transfer(fetched.raw as u16),
                };
                if transfer.writeback_register == 15
                    || (transfer.operation == CpuBusOperation::Read && transfer.destination == 15)
                    || !plain_data_access(bus, transfer)
                {
                    return None;
                }
                let mut cursor = DataAccessCursor::default();
                cursor.reset(fetch_cycles);
                cursor.advance(
                    transfer.address,
                    transfer.width,
                    AccessType::NonSequential,
                    bus.waitcnt(),
                );
                let charge = DataAccessCharge::new(fetch_cycles, total, cursor, false);
                Some((
                    DirectOperation::Transfer {
                        transfer,
                        #[cfg(test)]
                        cursor,
                    },
                    charge.cycles,
                ))
            }
            DecodedInstruction::Arm {
                class: ArmInstructionClass::BlockDataTransfer,
                ..
            } => {
                let (transfer, cursor) = self.plan_arm_block_transfer(bus, fetched)?;
                let charge = DataAccessCharge::new(fetch_cycles, total, cursor, false);
                Some((
                    DirectOperation::BlockTransfer {
                        transfer,
                        #[cfg(test)]
                        cursor,
                    },
                    charge.cycles,
                ))
            }
            DecodedInstruction::Arm {
                class: ArmInstructionClass::Branch,
                ..
            } => {
                let offset = super::ops::sign_extend(fetched.raw & 0x00FF_FFFF, 24) << 2;
                let target = fetched.pc.wrapping_add(8).wrapping_add_signed(offset);
                plan_direct_branch(bus, target, fetched.instruction_set, total)
            }
            DecodedInstruction::Thumb {
                class: ThumbInstructionClass::ConditionalBranchOrSwi,
            } => {
                let condition = ((fetched.raw >> 8) & 15) as u8;
                if condition >= 14 {
                    return None;
                }
                if !self.condition_passed(condition) {
                    return Some((DirectOperation::Noop, total));
                }
                let offset = super::ops::sign_extend(fetched.raw & 0xFF, 8) << 1;
                let target = fetched.pc.wrapping_add(4).wrapping_add_signed(offset);
                plan_direct_branch(bus, target, fetched.instruction_set, total)
            }
            DecodedInstruction::Thumb {
                class: ThumbInstructionClass::UnconditionalBranch,
            } => {
                let offset = super::ops::sign_extend(fetched.raw & 0x7FF, 11) << 1;
                let target = fetched.pc.wrapping_add(4).wrapping_add_signed(offset);
                plan_direct_branch(bus, target, fetched.instruction_set, total)
            }
            _ => None,
        }
    }

    fn execute_frame_transfer(
        &mut self,
        bus: &mut Bus,
        fetched: FetchedInstruction,
        transfer: SingleTransfer,
    ) {
        match (transfer.operation, transfer.width) {
            (CpuBusOperation::Read, width) => {
                let value = match width {
                    1 => u32::from(bus.read8(transfer.address)),
                    2 => u32::from(bus.read16(transfer.address)),
                    _ => bus.read32(transfer.address),
                };
                self.regs[usize::from(transfer.destination)] = match fetched.instruction_set {
                    InstructionSet::Arm => {
                        arm_single_load_value(fetched.raw, transfer.address, value)
                    }
                    InstructionSet::Thumb => {
                        self.pending_load_internal_cycle = true;
                        thumb_halfword_load_value(transfer.address, value)
                    }
                };
            }
            (CpuBusOperation::Write, 1) => bus.write8(transfer.address, transfer.value as u8),
            (CpuBusOperation::Write, 2) => bus.write16(transfer.address, transfer.value as u16),
            (CpuBusOperation::Write, 4) => bus.write32(transfer.address, transfer.value),
            _ => unreachable!("invalid direct transfer"),
        }
        if transfer.writeback_register != NO_WRITEBACK {
            self.regs[usize::from(transfer.writeback_register)] = transfer.writeback_value;
        }
    }

    pub(super) fn execute_frame_pure(&mut self, operation: DirectPure, pc: u32, raw: u32) {
        match operation {
            DirectPure::ArmConditionFailed => {}
            DirectPure::ArmDataProcessing => self.execute_arm_data_processing(pc, raw),
            DirectPure::ArmMultiply => self.execute_arm_multiply(raw),
            DirectPure::ThumbMoveShiftedRegister => {
                self.execute_thumb_move_shifted_register(raw as u16)
            }
            DirectPure::ThumbAddSubtract => self.execute_thumb_add_subtract(raw as u16),
            DirectPure::ThumbImmediate => self.execute_thumb_immediate(raw as u16),
            DirectPure::ThumbAlu => self.execute_thumb_alu(raw as u16),
            DirectPure::ThumbLoadAddress => self.execute_thumb_load_address(pc, raw as u16),
            DirectPure::ThumbAddOffsetSp => self.execute_thumb_add_offset_sp(raw as u16),
        }
    }

    fn plan_arm_block_transfer(
        &self,
        bus: &Bus,
        fetched: FetchedInstruction,
    ) -> Option<(DirectBlockTransfer, DataAccessCursor)> {
        let raw = fetched.raw;
        let list = raw as u16;
        let base_register = ((raw >> 16) & 15) as usize;
        if list == 0 || list & (1 << 15) != 0 || raw & (1 << 22) != 0 || base_register == 15 {
            return None;
        }
        let count = list.count_ones();
        let base = self.regs[base_register];
        let pre = raw & (1 << 24) != 0;
        let up = raw & (1 << 23) != 0;
        let address = match (up, pre) {
            (true, false) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(4 * (count - 1)),
            (false, true) => base.wrapping_sub(4 * count),
        };
        let last_address = address.checked_add(4 * (count - 1))?;
        let (ram, size) = match timing::region_for_addr(address) {
            BusRegion::Ewram if timing::region_for_addr(last_address) == BusRegion::Ewram => {
                (DirectRam::Ewram, EWRAM_SIZE)
            }
            BusRegion::Iwram if timing::region_for_addr(last_address) == BusRegion::Iwram => {
                (DirectRam::Iwram, IWRAM_SIZE)
            }
            _ => return None,
        };
        let start_index = (address as usize & (size - 1)) & !3;
        let byte_count = count as usize * 4;
        let memory_len = match ram {
            DirectRam::Ewram => bus.ewram.len(),
            DirectRam::Iwram => bus.iwram.len(),
        };
        if start_index.checked_add(byte_count)? > memory_len {
            return None;
        }
        let writeback_value = if up {
            base.wrapping_add(4 * count)
        } else {
            base.wrapping_sub(4 * count)
        };
        let load = raw & (1 << 20) != 0;
        let writeback = raw & (1 << 21) != 0 && !(load && list & (1 << base_register) != 0);
        let mut cursor = DataAccessCursor::default();
        cursor.reset(fetched.fetch_cycles);
        for index in 0..count {
            cursor.advance(
                address + index * 4,
                4,
                if index == 0 {
                    AccessType::NonSequential
                } else {
                    AccessType::Sequential
                },
                bus.waitcnt(),
            );
        }
        Some((
            DirectBlockTransfer {
                ram,
                start_index: start_index as u32,
                register_mask: list,
                base_register: base_register as u8,
                writeback_value,
                load,
                writeback,
            },
            cursor,
        ))
    }

    fn execute_frame_block_transfer(&mut self, bus: &mut Bus, transfer: DirectBlockTransfer) {
        let memory = match transfer.ram {
            DirectRam::Ewram => bus.ewram.as_mut_slice(),
            DirectRam::Iwram => bus.iwram.as_mut_slice(),
        };
        let mut mask = transfer.register_mask;
        let first = mask.trailing_zeros() as usize;
        let base_register = usize::from(transfer.base_register);
        let mut offset = transfer.start_index as usize;
        while mask != 0 {
            let register = mask.trailing_zeros() as usize;
            if transfer.load {
                self.regs[register] = u32::from_le_bytes(
                    memory[offset..offset + 4]
                        .try_into()
                        .expect("validated direct block load"),
                );
            } else {
                let value = if transfer.writeback && register == base_register && register != first
                {
                    transfer.writeback_value
                } else {
                    self.regs[register]
                };
                memory[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            mask &= mask - 1;
            offset += 4;
        }
        if transfer.writeback {
            self.regs[base_register] = transfer.writeback_value;
        }
    }

    fn refill_frame_direct(
        &mut self,
        bus: &Bus,
        target: u32,
        instruction_set: InstructionSet,
        cycles: [u32; 2],
    ) -> [PrefetchedInstruction; 2] {
        let width = instruction_set.width_bytes();
        let mut entries = [EMPTY_PREFETCHED_INSTRUCTION; 2];
        for (index, expected) in cycles.into_iter().enumerate() {
            let address = target.wrapping_add(index as u32 * u32::from(width));
            #[cfg(feature = "profiling")]
            self.profile_instruction_fetch(bus, address, instruction_set, width, index != 0);
            let (fetched, fetch_cycles) =
                fetch::fetch_prefetched_at(bus, address, instruction_set, width, index != 0);
            self.track_bios_fetch(fetched.pc, fetched.raw, instruction_set);
            entries[index] = fetched;
            let charged = if index == 0 {
                fetch_cycles.max(u32::from(self.take_pending_load_internal_cycle()))
            } else {
                fetch_cycles
            };
            debug_assert_eq!(charged, expected);
            #[cfg(test)]
            {
                self.cycles = self.cycles.wrapping_add(u64::from(charged));
            }
            #[cfg(test)]
            {
                self.add_instruction_refill_cycles(charged);
            }
        }
        entries
    }
}

fn plan_direct_branch(
    bus: &Bus,
    target: u32,
    instruction_set: InstructionSet,
    total: u32,
) -> Option<(DirectOperation, u32)> {
    let window = DirectFetchWindow::for_pc(bus, target, instruction_set)?;
    let refill_cycles = [
        timing::instruction_fetch_cycles_with_waitcnt(
            target,
            instruction_set.width_bytes(),
            false,
            bus.waitcnt(),
        ),
        window.cycles,
    ];
    Some((
        DirectOperation::Branch(target, refill_cycles, window),
        total + refill_cycles[0] + refill_cycles[1],
    ))
}

impl DirectFetchWindow {
    fn new(cpu: &Cpu, bus: &Bus) -> Option<Self> {
        let instruction_set = cpu.instruction_set();
        let width = u32::from(instruction_set.width_bytes());
        let pc = cpu.pc() & !(width - 1);
        if cpu.pipeline.len() != PREFETCH_QUEUE_LEN
            || cpu.pipeline.instruction_set != instruction_set
            || !cpu
                .pipeline
                .entries
                .iter()
                .enumerate()
                .all(|(index, entry)| entry.pc == pc.wrapping_add(index as u32 * width))
        {
            return None;
        }
        Self::for_pc(bus, pc, instruction_set)
    }

    pub(super) fn for_pc(bus: &Bus, pc: u32, instruction_set: InstructionSet) -> Option<Self> {
        let width = u32::from(instruction_set.width_bytes());
        let end = match timing::region_for_addr(pc) {
            BusRegion::Ewram => EWRAM_END + 1,
            BusRegion::Iwram => IWRAM_END + 1,
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2 => {
                (pc & !0x01FF_FFFF) + bus.cartridge.rom().len().min(0x0200_0000) as u32
            }
            _ => return None,
        };
        let lookahead = pc.checked_add(PREFETCH_QUEUE_LEN as u32 * width)?;
        let last_fetch_address = end.checked_sub(width)?;
        if lookahead > last_fetch_address {
            return None;
        }
        Some(Self {
            last_fetch_address,
            width,
            cycles: timing::instruction_fetch_cycles_with_waitcnt(
                lookahead,
                instruction_set.width_bytes(),
                true,
                bus.waitcnt(),
            ),
        })
    }
}

pub(super) fn plain_data_access(bus: &Bus, transfer: SingleTransfer) -> bool {
    let address = transfer.address & !(u32::from(transfer.width) - 1);
    match timing::region_for_addr(address) {
        BusRegion::Ewram | BusRegion::Iwram => true,
        BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2 => {
            transfer.operation == CpuBusOperation::Read
                && !bus.cartridge.is_eeprom_access_addr(address)
                && !bus.cartridge.has_rtc()
                && (address & 0x01FF_FFFF) as usize + usize::from(transfer.width)
                    <= bus.cartridge.rom().len()
        }
        _ => false,
    }
}

fn direct_pure_candidate(
    fetched: FetchedInstruction,
    condition_passed: bool,
    options: DirectOptions,
) -> Option<DirectPure> {
    let operation = classify_stateless_pure(fetched, condition_passed)?;
    (options.allow_multiply || !matches!(operation, DirectPure::ArmMultiply)).then_some(operation)
}

#[cfg(any(test, feature = "profiling"))]
pub(super) fn stateless_candidate(
    fetched: FetchedInstruction,
    condition_passed: bool,
) -> Option<usize> {
    classify_stateless_pure(fetched, condition_passed).map(|operation| match operation {
        DirectPure::ArmConditionFailed => 0,
        DirectPure::ArmDataProcessing
            if fetched.raw & (1 << 25) == 0 && fetched.raw & (1 << 4) != 0 =>
        {
            2
        }
        DirectPure::ArmDataProcessing | DirectPure::ArmMultiply => 1,
        DirectPure::ThumbMoveShiftedRegister
        | DirectPure::ThumbAddSubtract
        | DirectPure::ThumbImmediate
        | DirectPure::ThumbAlu
        | DirectPure::ThumbLoadAddress
        | DirectPure::ThumbAddOffsetSp => 3,
    })
}

pub(super) fn classify_stateless_pure(
    fetched: FetchedInstruction,
    condition_passed: bool,
) -> Option<DirectPure> {
    match fetched.decoded {
        DecodedInstruction::Arm { .. } if !condition_passed => Some(DirectPure::ArmConditionFailed),
        DecodedInstruction::Arm {
            class: ArmInstructionClass::DataProcessing,
            ..
        } => {
            let raw = fetched.raw;
            if raw & 0x0FBF_0FFF == 0x010F_0000
                || raw & 0x0FB0_FFF0 == 0x0120_F000
                || raw & 0x0FB0_F000 == 0x0320_F000
                || (raw >> 12) & 0xF == 15
            {
                None
            } else {
                Some(DirectPure::ArmDataProcessing)
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::Multiply,
            ..
        } => {
            let raw = fetched.raw;
            [raw >> 16, raw >> 12, raw >> 8, raw]
                .into_iter()
                .all(|register| register & 0xF != 15)
                .then_some(DirectPure::ArmMultiply)
        }
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::MoveShiftedRegister,
        } => Some(DirectPure::ThumbMoveShiftedRegister),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::AddSubtract,
        } => Some(DirectPure::ThumbAddSubtract),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::Immediate,
        } => Some(DirectPure::ThumbImmediate),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::Alu,
        } => Some(DirectPure::ThumbAlu),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::LoadAddress,
        } => Some(DirectPure::ThumbLoadAddress),
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::AddOffsetSp,
        } => Some(DirectPure::ThumbAddOffsetSp),
        _ => None,
    }
}

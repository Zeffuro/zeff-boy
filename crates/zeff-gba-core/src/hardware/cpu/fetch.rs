#[cfg(feature = "profiling")]
use super::super::timing::BusRegion;
use super::super::{bus::Bus, timing};
use super::decode::decode_stub;
use super::*;

impl Cpu {
    #[cfg(feature = "profiling")]
    pub(super) fn profile_instruction_fetch(
        &mut self,
        bus: &Bus,
        pc: u32,
        instruction_set: InstructionSet,
        width_bytes: u8,
        sequential: bool,
    ) {
        let profiling = &mut self.profiling;
        profiling.instruction_fetches = profiling.instruction_fetches.wrapping_add(1);
        profiling.instruction_fetch_modes[usize::from(instruction_set == InstructionSet::Thumb)] =
            profiling.instruction_fetch_modes
                [usize::from(instruction_set == InstructionSet::Thumb)]
            .wrapping_add(1);
        profiling.instruction_fetch_accesses[usize::from(sequential)] =
            profiling.instruction_fetch_accesses[usize::from(sequential)].wrapping_add(1);

        let region = timing::region_for_addr(pc);
        let region_index = match region {
            BusRegion::Bios => 0,
            BusRegion::Ewram => 1,
            BusRegion::Iwram => 2,
            BusRegion::GamePak0 => 3,
            BusRegion::GamePak1 => 4,
            BusRegion::GamePak2 => 5,
            _ => 6,
        };
        profiling.instruction_fetch_regions[region_index] =
            profiling.instruction_fetch_regions[region_index].wrapping_add(1);

        let waitcnt = bus.waitcnt() & 0x07FC;
        if profiling
            .last_instruction_fetch_waitcnt
            .is_some_and(|previous| previous != waitcnt)
        {
            profiling.instruction_fetch_waitcnt_changes =
                profiling.instruction_fetch_waitcnt_changes.wrapping_add(1);
        }
        profiling.last_instruction_fetch_waitcnt = Some(waitcnt);

        let fallback = if bus.debug_trace_enabled && bus.debug_trace_reads {
            Some(4)
        } else if !matches!(
            region,
            BusRegion::Ewram
                | BusRegion::Iwram
                | BusRegion::GamePak0
                | BusRegion::GamePak1
                | BusRegion::GamePak2
        ) {
            Some(3)
        } else if instruction_set == InstructionSet::Thumb
            && bus.cartridge.is_eeprom_access_addr(pc)
        {
            Some(0)
        } else if bus
            .cartridge
            .instruction_fetch_overlaps_rtc_gpio(pc, width_bytes)
        {
            Some(1)
        } else if matches!(
            region,
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2
        ) && bus
            .cartridge
            .instruction_fetch_uses_open_bus(pc, width_bytes)
        {
            Some(2)
        } else {
            None
        };

        if let Some(index) = fallback {
            profiling.instruction_fetch_fallbacks[index] =
                profiling.instruction_fetch_fallbacks[index].wrapping_add(1);
        } else {
            profiling.instruction_fetch_descriptor_compatible = profiling
                .instruction_fetch_descriptor_compatible
                .wrapping_add(1);
        }
    }

    pub(crate) fn fetch_decode_stub(&mut self, bus: &Bus) -> FetchedInstruction {
        let pending_internal_cycles = u32::from(self.take_pending_load_internal_cycle());
        let instruction_set = self.instruction_set();
        let width_bytes = instruction_set.width_bytes();
        let pc = align_pc(self.pc(), instruction_set);
        self.last_opcode_pc = pc;

        let queued_front_matches = matches!(
            self.pipeline.front(),
            Some(fetched) if fetched.pc == pc && fetched.instruction_set == instruction_set
        );
        let mut fetched = if queued_front_matches {
            self.pipeline.pop_front().expect("pipeline front existed")
        } else {
            self.pipeline.clear();
            #[cfg(feature = "profiling")]
            self.profile_instruction_fetch(
                bus,
                pc,
                instruction_set,
                width_bytes,
                self.next_fetch_sequential,
            );
            fetch_instruction_at(
                bus,
                pc,
                instruction_set,
                width_bytes,
                self.next_fetch_sequential,
            )
        };

        if self.state == CpuState::Running {
            self.regs[15] = pc.wrapping_add(u32::from(width_bytes));
            if self.swi_wait_return_pc == Some(pc) {
                self.bios_protected_read_latch = POST_SWI_BIOS_READ_LATCH;
                self.swi_wait_return_pc = None;
            }
            self.track_bios_fetch(fetched);
            let fetch_cycles = if queued_front_matches {
                self.fetch_next_sequential(bus, instruction_set, width_bytes)
            } else {
                self.fill_prefetch_pipeline(bus, instruction_set, width_bytes);
                fetched.fetch_cycles
            }
            .max(pending_internal_cycles);
            fetched.fetch_cycles = fetch_cycles;
            self.cycles = self.cycles.wrapping_add(u64::from(fetch_cycles));
            self.next_fetch_sequential = true;
            self.last_fetch = Some(fetched);
        }

        fetched
    }

    fn fetch_next_sequential(
        &mut self,
        bus: &Bus,
        instruction_set: InstructionSet,
        width_bytes: u8,
    ) -> u32 {
        let width = u32::from(width_bytes);
        let pc = self.pipeline.back().map_or_else(
            || align_pc(self.pc(), instruction_set),
            |fetched| fetched.pc.wrapping_add(width),
        );
        let pc = align_pc(pc, instruction_set);
        #[cfg(feature = "profiling")]
        self.profile_instruction_fetch(bus, pc, instruction_set, width_bytes, true);
        let fetched = fetch_instruction_at(bus, pc, instruction_set, width_bytes, true);
        self.track_bios_fetch(fetched);
        self.pipeline.push_back(fetched);
        fetched.fetch_cycles
    }

    fn fill_prefetch_pipeline(
        &mut self,
        bus: &Bus,
        instruction_set: InstructionSet,
        width_bytes: u8,
    ) {
        let width = u32::from(width_bytes);
        let mut pc = self.pipeline.back().map_or_else(
            || align_pc(self.pc(), instruction_set),
            |fetched| fetched.pc.wrapping_add(width),
        );
        while self.pipeline.len() < PREFETCH_QUEUE_LEN {
            let aligned_pc = align_pc(pc, instruction_set);
            #[cfg(feature = "profiling")]
            self.profile_instruction_fetch(bus, aligned_pc, instruction_set, width_bytes, true);
            let fetched = fetch_instruction_at(bus, aligned_pc, instruction_set, width_bytes, true);
            self.track_bios_fetch(fetched);
            self.pipeline.push_back(fetched);
            pc = aligned_pc.wrapping_add(width);
        }
    }

    pub(crate) fn pipeline_state(&self) -> CpuPipelineState {
        let mut state = CpuPipelineState {
            len: self.pipeline.len,
            pending_load_internal_cycle: self.pending_load_internal_cycle,
            ..CpuPipelineState::default()
        };
        for (entry, fetched) in state
            .entries
            .iter_mut()
            .zip(self.pipeline.entries.iter())
            .take(self.pipeline.len())
        {
            *entry = CpuPipelineEntryState {
                pc: fetched.pc,
                raw: fetched.raw,
                thumb: fetched.instruction_set == InstructionSet::Thumb,
            };
        }
        state
    }

    pub(crate) fn set_pipeline_state(&mut self, state: CpuPipelineState) -> bool {
        if usize::from(state.len) > PREFETCH_QUEUE_LEN {
            return false;
        }
        self.pipeline.clear();
        let expected_instruction_set = self.instruction_set();
        let width = u32::from(expected_instruction_set.width_bytes());
        let expected_first_pc = align_pc(self.pc(), expected_instruction_set);
        for (index, entry) in state
            .entries
            .into_iter()
            .take(usize::from(state.len))
            .enumerate()
        {
            let instruction_set = if entry.thumb {
                InstructionSet::Thumb
            } else {
                InstructionSet::Arm
            };
            let expected_pc = expected_first_pc.wrapping_add(width.saturating_mul(index as u32));
            if instruction_set != expected_instruction_set || entry.pc != expected_pc {
                self.pipeline.clear();
                return false;
            }
            self.pipeline.push_back(FetchedInstruction {
                pc: entry.pc,
                raw: entry.raw,
                instruction_set,
                width_bytes: instruction_set.width_bytes(),
                fetch_cycles: 0,
                decoded: decode_stub(entry.raw, instruction_set),
            });
        }
        self.pending_load_internal_cycle = state.pending_load_internal_cycle;
        true
    }

    pub(crate) fn migrate_legacy_pipeline(&mut self) {
        self.pipeline.clear();
        self.pending_load_internal_cycle = false;
    }

    pub(super) fn flush_prefetch_queue(&mut self) {
        self.pipeline.clear();
        self.pending_load_internal_cycle = false;
    }

    pub(super) fn take_pending_load_internal_cycle(&mut self) -> bool {
        std::mem::take(&mut self.pending_load_internal_cycle)
    }
}
fn align_pc(pc: u32, instruction_set: InstructionSet) -> u32 {
    match instruction_set {
        InstructionSet::Arm => pc & !3,
        InstructionSet::Thumb => pc & !1,
    }
}

pub(super) fn fetch_instruction_at(
    bus: &Bus,
    pc: u32,
    instruction_set: InstructionSet,
    width_bytes: u8,
    sequential: bool,
) -> FetchedInstruction {
    let raw = match instruction_set {
        InstructionSet::Arm => bus.read32(pc),
        InstructionSet::Thumb => u32::from(bus.read16(pc)),
    };
    let fetch_cycles =
        timing::instruction_fetch_cycles_with_waitcnt(pc, width_bytes, sequential, bus.waitcnt());

    FetchedInstruction {
        pc,
        raw,
        instruction_set,
        width_bytes,
        fetch_cycles,
        decoded: decode_stub(raw, instruction_set),
    }
}

use super::super::timing::{self, BusRegion};
use super::frame_direct::stateless_candidate;
use super::{
    Bus, Cpu, CpuState, DecodedInstruction, FetchedInstruction, InstructionSet, PREFETCH_QUEUE_LEN,
    instruction_base_cycles,
};

impl Cpu {
    pub(super) fn profile_frame_kernel_fetch_gate(&self, bus: &Bus) -> usize {
        if !bus.frame_service_active() {
            return 1;
        }
        let instruction_set = self.instruction_set();
        let width = u32::from(instruction_set.width_bytes());
        let pc = self.pc() & !(width - 1);
        if self.pipeline.len() != PREFETCH_QUEUE_LEN
            || self.pipeline.instruction_set != instruction_set
            || !self
                .pipeline
                .front()
                .is_some_and(|fetched| fetched.pc == pc)
        {
            return 2;
        }
        let lookahead_pc = self
            .pipeline
            .back()
            .expect("full pipeline")
            .pc
            .wrapping_add(width);
        if !self
            .pipeline
            .entries
            .iter()
            .all(|entry| descriptor_safe(bus, entry.pc, instruction_set))
            || !descriptor_safe(bus, lookahead_pc, instruction_set)
        {
            return 3;
        }
        0
    }

    pub(super) fn profile_frame_kernel_instruction(
        &mut self,
        bus: &Bus,
        fetched: FetchedInstruction,
        fetch_gate: usize,
    ) {
        self.profile_thumb_macro_instruction(bus, fetched, fetch_gate);
        self.profiling.frame_kernel_pending_eligible = false;
        self.profiling.frame_kernel_entry_service_entries = bus.profiling.service_entries;
        let class_counter = match fetched.decoded {
            DecodedInstruction::Arm { class, .. } => {
                &mut self.profiling.instruction_classes_arm[class as usize]
            }
            DecodedInstruction::Thumb { class } => {
                &mut self.profiling.instruction_classes_thumb[class as usize]
            }
        };
        *class_counter = class_counter.wrapping_add(1);
        let condition_passed = self.fetched_condition_passed(fetched);
        let Some(candidate) = stateless_candidate(fetched, condition_passed) else {
            return;
        };
        let cycles = u64::from(fetched.fetch_cycles)
            + u64::from(instruction_base_cycles(fetched, condition_passed));
        self.profiling.frame_kernel_candidates[candidate] =
            self.profiling.frame_kernel_candidates[candidate].wrapping_add(1);
        self.profiling.frame_kernel_candidate_cycles[candidate] =
            self.profiling.frame_kernel_candidate_cycles[candidate].wrapping_add(cycles);
        self.profiling.frame_kernel_fetch_gates[fetch_gate] =
            self.profiling.frame_kernel_fetch_gates[fetch_gate].wrapping_add(1);
        if fetch_gate == 0 {
            self.profiling.frame_kernel_pending_eligible = true;
            self.profiling.frame_kernel_fetch_eligible[candidate] =
                self.profiling.frame_kernel_fetch_eligible[candidate].wrapping_add(1);
            self.profiling.frame_kernel_fetch_eligible_cycles = self
                .profiling
                .frame_kernel_fetch_eligible_cycles
                .wrapping_add(cycles);
        }
    }

    pub(super) fn profile_frame_kernel_completion(&mut self, bus: &Bus) {
        self.profile_thumb_macro_completion(bus);
        let service_entries = bus.profiling.service_entries;
        let profiling = &mut self.profiling;
        if profiling.frame_kernel_pending_eligible
            && profiling.frame_kernel_entry_service_entries == service_entries
            && self.state == CpuState::Running
        {
            if profiling.frame_kernel_last_service_entries != service_entries {
                profiling.frame_kernel_quiet_run_length = 0;
            }
            profiling.frame_kernel_quiet_instructions =
                profiling.frame_kernel_quiet_instructions.wrapping_add(1);
            if profiling.frame_kernel_quiet_run_length == 0 {
                profiling.frame_kernel_quiet_runs =
                    profiling.frame_kernel_quiet_runs.wrapping_add(1);
            }
            profiling.frame_kernel_quiet_run_length =
                profiling.frame_kernel_quiet_run_length.saturating_add(1);
            profiling.frame_kernel_quiet_longest_run = profiling
                .frame_kernel_quiet_longest_run
                .max(profiling.frame_kernel_quiet_run_length);
        } else {
            profiling.frame_kernel_quiet_run_length = 0;
        }
        profiling.frame_kernel_pending_eligible = false;
        profiling.frame_kernel_last_service_entries = service_entries;
    }
}

fn descriptor_safe(bus: &Bus, pc: u32, instruction_set: InstructionSet) -> bool {
    let region = timing::region_for_addr(pc);
    let width = instruction_set.width_bytes();
    matches!(
        region,
        BusRegion::Ewram
            | BusRegion::Iwram
            | BusRegion::GamePak0
            | BusRegion::GamePak1
            | BusRegion::GamePak2
    ) && !(bus.debug_trace_enabled && bus.debug_trace_reads)
        && !(instruction_set == InstructionSet::Thumb && bus.cartridge.is_eeprom_access_addr(pc))
        && !bus.cartridge.instruction_fetch_overlaps_rtc_gpio(pc, width)
        && (!matches!(
            region,
            BusRegion::GamePak0 | BusRegion::GamePak1 | BusRegion::GamePak2
        ) || !bus.cartridge.instruction_fetch_uses_open_bus(pc, width))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cpu::decode::decode_stub;

    fn fetched(raw: u32, instruction_set: InstructionSet) -> FetchedInstruction {
        FetchedInstruction {
            pc: 0x0800_0000,
            raw,
            instruction_set,
            width_bytes: instruction_set.width_bytes(),
            fetch_cycles: 1,
            decoded: decode_stub(raw, instruction_set),
        }
    }

    #[test]
    fn stateless_arm_profile_excludes_psr_pc_and_memory_effects() {
        for (raw, expected) in [
            (0xE280_0001, Some(1)),
            (0xE1A0_0110, Some(2)),
            (0xE10F_0000, None),
            (0xE12F_F000, None),
            (0xE328_F001, None),
            (0xE1A0_F000, None),
            (0xE350_F000, None),
            (0xE000_0291, Some(1)),
            (0xE00F_0291, None),
            (0xE000_F291, None),
            (0xE000_0F91, None),
            (0xE000_029F, None),
            (0xE0C0_2091, None),
            (0xE590_0000, None),
            (0xEA00_0000, None),
            (0xE100_0090, None),
        ] {
            let instruction = fetched(raw, InstructionSet::Arm);
            assert_eq!(
                stateless_candidate(instruction, true),
                expected,
                "{raw:08X}"
            );
            assert_eq!(
                stateless_candidate(instruction, false),
                Some(0),
                "{raw:08X}"
            );
        }
    }

    #[test]
    fn stateless_thumb_profile_only_admits_pure_low_register_classes() {
        for raw in [0x0001, 0x1801, 0x2001, 0x4001, 0xA001, 0xB001] {
            assert_eq!(
                stateless_candidate(fetched(raw, InstructionSet::Thumb), true),
                Some(3),
                "{raw:04X}",
            );
        }
        for raw in [0x4400, 0x4800, 0x6000, 0xB401, 0xD001, 0xE000, 0xF000] {
            assert_eq!(
                stateless_candidate(fetched(raw, InstructionSet::Thumb), true),
                None,
                "{raw:04X}",
            );
        }
    }

    #[test]
    fn stateless_fetch_profile_requires_a_safe_warm_lookahead() {
        use crate::hardware::cartridge::Cartridge;

        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        let mut bus = Bus::new(Cartridge::load(&rom).unwrap(), 48_000);
        let mut cpu = Cpu::new();
        cpu.set_pc(0x0800_0000);
        assert_eq!(cpu.profile_frame_kernel_fetch_gate(&bus), 1);
        bus.begin_frame_service();
        assert_eq!(cpu.profile_frame_kernel_fetch_gate(&bus), 2);
        cpu.fetch_decode_stub(&bus);
        assert_eq!(cpu.profile_frame_kernel_fetch_gate(&bus), 0);

        cpu.set_pc(0x0800_00B4);
        cpu.fetch_decode_stub(&bus);
        assert_eq!(cpu.profile_frame_kernel_fetch_gate(&bus), 3);
    }
}

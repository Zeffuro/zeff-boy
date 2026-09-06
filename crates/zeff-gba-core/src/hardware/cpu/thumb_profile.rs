use super::frame_direct::{DirectFetchWindow, plain_data_access, stateless_candidate};
use super::{Bus, CPSR_IRQ_DISABLE, Cpu, CpuState, FetchedInstruction, InstructionSet};

const COND_TAKEN: usize = 0;
const COND_UNTAKEN: usize = 1;
const SWI: usize = 2;
const RESERVED: usize = 3;
const BRANCH: usize = 4;
const LDRH: usize = 5;
const STRH: usize = 6;

impl Cpu {
    fn thumb_macro_kind(&self, fetched: FetchedInstruction) -> Option<usize> {
        if fetched.instruction_set != InstructionSet::Thumb {
            return None;
        }
        match fetched.raw & 0xF000 {
            0xD000 => Some(match (fetched.raw >> 8) & 15 {
                15 => SWI,
                14 => RESERVED,
                condition if self.condition_passed(condition as u8) => COND_TAKEN,
                _ => COND_UNTAKEN,
            }),
            0xE000 if fetched.raw & 0x0800 == 0 => Some(BRANCH),
            0x8000 => Some(if fetched.raw & 0x0800 != 0 {
                LDRH
            } else {
                STRH
            }),
            _ => None,
        }
    }

    fn thumb_macro_plain_halfword(&self, bus: &Bus, fetched: FetchedInstruction) -> bool {
        plain_data_access(bus, self.plan_thumb_halfword_transfer(fetched.raw as u16))
    }

    pub(super) fn profile_thumb_macro_instruction(
        &mut self,
        bus: &Bus,
        fetched: FetchedInstruction,
        fetch_gate: usize,
    ) {
        let kind = self.thumb_macro_kind(fetched);
        let plain_halfword =
            kind.is_some_and(|kind| kind >= LDRH) && self.thumb_macro_plain_halfword(bus, fetched);
        let region_safe = match kind {
            Some(COND_TAKEN | BRANCH) => {
                let (mask, bits) = if kind == Some(BRANCH) {
                    (0x7FF, 11)
                } else {
                    (0xFF, 8)
                };
                let offset = super::ops::sign_extend(fetched.raw & mask, bits) << 1;
                let target = fetched.pc.wrapping_add(4).wrapping_add_signed(offset);
                DirectFetchWindow::for_pc(bus, target, InstructionSet::Thumb).is_some()
            }
            Some(COND_UNTAKEN) => true,
            Some(LDRH | STRH) => plain_halfword,
            _ => stateless_candidate(fetched, true) == Some(3),
        };
        let eligible = fetch_gate == 0
            && bus.frame_service_active()
            && !bus.debug_trace_enabled
            && !self.break_after_next_stub
            && !(bus.interrupt_ready() && self.cpsr & CPSR_IRQ_DISABLE == 0)
            && DirectFetchWindow::for_pc(bus, fetched.pc, InstructionSet::Thumb).is_some()
            && region_safe;
        let profiling = &mut self.profiling;
        if let Some(kind) = kind {
            profiling.thumb_macro_counts[kind] = profiling.thumb_macro_counts[kind].wrapping_add(1);
            if plain_halfword {
                profiling.thumb_macro_plain_halfwords[kind - LDRH] =
                    profiling.thumb_macro_plain_halfwords[kind - LDRH].wrapping_add(1);
            }
            if eligible {
                profiling.thumb_macro_eligible[kind] =
                    profiling.thumb_macro_eligible[kind].wrapping_add(1);
            }
        }
        profiling.thumb_macro_pending_kind = kind;
        profiling.thumb_macro_pending_eligible = eligible;
        profiling.thumb_macro_entry_service_entries = bus.profiling.service_entries;
    }

    pub(super) fn profile_thumb_macro_completion(&mut self, bus: &Bus) {
        let profiling = &mut self.profiling;
        let entry = profiling.thumb_macro_entry_service_entries;
        let quiet = profiling.thumb_macro_pending_eligible
            && entry == bus.profiling.service_entries
            && self.state == CpuState::Running;
        let left = quiet
            && profiling.thumb_macro_previous_quiet
            && profiling.thumb_macro_previous_service_entries == entry;
        if let Some((kind, neighbors)) = profiling.thumb_macro_pending_neighbor.take()
            && left
        {
            profiling.thumb_macro_neighbors[kind][neighbors] =
                profiling.thumb_macro_neighbors[kind][neighbors].wrapping_sub(1);
            profiling.thumb_macro_neighbors[kind][neighbors | 2] =
                profiling.thumb_macro_neighbors[kind][neighbors | 2].wrapping_add(1);
        }
        if let Some(kind) = profiling.thumb_macro_pending_kind.take()
            && quiet
        {
            let neighbors = usize::from(left);
            profiling.thumb_macro_quiet[kind] = profiling.thumb_macro_quiet[kind].wrapping_add(1);
            // The final instruction has no right neighbor until one completes.
            profiling.thumb_macro_neighbors[kind][neighbors] =
                profiling.thumb_macro_neighbors[kind][neighbors].wrapping_add(1);
            profiling.thumb_macro_pending_neighbor = Some((kind, neighbors));
        }
        profiling.thumb_macro_previous_quiet = quiet;
        profiling.thumb_macro_previous_service_entries = bus.profiling.service_entries;
        profiling.thumb_macro_pending_eligible = false;
    }

    pub(super) fn profile_thumb_macro_run_end(&mut self, fetched: FetchedInstruction) {
        if let Some(kind) = self.thumb_macro_kind(fetched) {
            self.profiling.thumb_macro_run_ends[kind] =
                self.profiling.thumb_macro_run_ends[kind].wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::Emulator;
    use crate::hardware::cpu::{CPSR_THUMB, decode::decode_stub};

    fn fixture() -> Emulator {
        let mut rom = vec![0; 0x4000];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        emu.cpu.cpsr |= CPSR_THUMB;
        emu.cpu.regs[0] = 0x0200_1000;
        emu.bus.begin_frame_service();
        emu
    }

    fn fetched(raw: u32, pc: u32) -> FetchedInstruction {
        FetchedInstruction {
            pc,
            raw,
            instruction_set: InstructionSet::Thumb,
            width_bytes: 2,
            fetch_cycles: 1,
            decoded: decode_stub(raw, InstructionSet::Thumb),
        }
    }

    #[test]
    fn thumb_macro_profile_classifies_conditions_and_only_immediate_halfwords() {
        let mut emu = fixture();
        for flags in 0..16 {
            emu.cpu.cpsr = (emu.cpu.cpsr & 0x0FFF_FFFF) | flags << 28;
            for condition in 0..16 {
                let raw = 0xD000 | condition << 8;
                let expected = match condition {
                    15 => SWI,
                    14 => RESERVED,
                    c if emu.cpu.condition_passed(c as u8) => COND_TAKEN,
                    _ => COND_UNTAKEN,
                };
                assert_eq!(
                    emu.cpu.thumb_macro_kind(fetched(raw, 0x0300_0200)),
                    Some(expected)
                );
                emu.cpu
                    .profile_thumb_macro_instruction(&emu.bus, fetched(raw, 0x0300_0200), 0);
                emu.cpu.profile_thumb_macro_completion(&emu.bus);
            }
        }
        for (raw, expected) in [
            (0xE7FF, Some(BRANCH)),
            (0x8000, Some(STRH)),
            (0x8FFF, Some(LDRH)),
            (0x5A00, None),
            (0xE800, None),
            (0xF800, None),
        ] {
            assert_eq!(
                emu.cpu.thumb_macro_kind(fetched(raw, 0x0300_0200)),
                expected
            );
        }
        assert_eq!(
            emu.cpu.profiling.thumb_macro_counts,
            [112, 112, 16, 16, 0, 0, 0]
        );
        assert_eq!(
            &emu.cpu.profiling.thumb_macro_eligible[SWI..=RESERVED],
            &[0, 0]
        );
    }

    #[test]
    fn thumb_macro_profile_reuses_plain_data_and_refill_window_fences() {
        let mut emu = fixture();
        for (address, read, write) in [
            (0x0200_1001, true, true),
            (0x0300_1001, true, true),
            (0x0800_0201, true, false),
            (0x0A00_0201, true, false),
            (0x0C00_0201, true, false),
            (0x0800_3FFF, true, false),
            (0x0800_4000, false, false),
            (0x0000_0010, false, false),
            (0x0400_0100, false, false),
            (0x0500_0000, false, false),
            (0x0600_0000, false, false),
            (0x0700_0000, false, false),
            (0x0E00_0000, false, false),
        ] {
            emu.cpu.regs[0] = address;
            assert_eq!(
                emu.cpu
                    .thumb_macro_plain_halfword(&emu.bus, fetched(0x8801, 0x0300_0200)),
                read,
                "{address:08X}"
            );
            assert_eq!(
                emu.cpu
                    .thumb_macro_plain_halfword(&emu.bus, fetched(0x8001, 0x0300_0200)),
                write,
                "{address:08X}"
            );
        }
        for (pc, raw, eligible) in [
            (0x0800_3FF4, 0xE000, true),
            (0x0800_3FF8, 0xE000, false),
            (0x0A00_0200, 0xE7FE, true),
            (0x0C00_0200, 0xE7FE, true),
            (0x0300_7FF8, 0xE000, true),
            (0x03FF_FFF8, 0xE000, false),
        ] {
            let before = emu.cpu.profiling.thumb_macro_eligible[BRANCH];
            emu.cpu
                .profile_thumb_macro_instruction(&emu.bus, fetched(raw, pc), 0);
            assert_eq!(
                emu.cpu.profiling.thumb_macro_eligible[BRANCH] - before,
                u64::from(eligible),
                "{pc:08X} {raw:04X}"
            );
        }
        emu.cpu.regs[0] = 0x0200_1000;
        emu.cpu
            .profile_thumb_macro_instruction(&emu.bus, fetched(0x8801, 0x0300_0200), 3);
        assert_eq!(emu.cpu.profiling.thumb_macro_plain_halfwords, [1, 0]);
        assert_eq!(emu.cpu.profiling.thumb_macro_eligible[LDRH], 0);
    }

    #[test]
    fn thumb_macro_profile_neighbors_require_eligible_quiet_completions() {
        let mut emu = fixture();
        for (raw, service_before, service_during) in [
            (0x8801, false, false),
            (0x2001, false, false),
            (0x8001, false, false),
            (0xD100, false, false),
            (0x2001, false, false),
            (0x8801, true, false),
            (0x2001, true, false),
            (0x8801, false, true),
            (0x8801, false, false),
            (0xDF00, false, false),
        ] {
            emu.bus.profiling.service_entries += u64::from(service_before);
            emu.cpu
                .profile_thumb_macro_instruction(&emu.bus, fetched(raw, 0x0300_0200), 0);
            emu.bus.profiling.service_entries += u64::from(service_during);
            emu.cpu.profile_thumb_macro_completion(&emu.bus);
        }
        let snapshot = emu.profiling_snapshot();
        assert_eq!(snapshot.thumb_macro_neighbors[LDRH], [2, 0, 1, 0]);
        assert_eq!(snapshot.thumb_macro_neighbors[STRH], [0, 0, 0, 1]);
        assert_eq!(snapshot.thumb_macro_neighbors[COND_TAKEN], [0, 0, 0, 1]);
        assert_eq!(snapshot.thumb_macro_quiet[LDRH], 3);
        assert_eq!(snapshot.thumb_macro_eligible[LDRH], 4);
        for (quiet, neighbors) in snapshot
            .thumb_macro_quiet
            .into_iter()
            .zip(snapshot.thumb_macro_neighbors)
        {
            assert_eq!(quiet, neighbors.iter().sum());
        }
        emu.reset_profiling();
        assert_eq!(emu.profiling_snapshot().thumb_macro_counts, [0; 7]);
        assert!(emu.cpu.profiling.thumb_macro_pending_neighbor.is_none());
    }

    #[test]
    fn thumb_macro_profile_counts_real_direct_stops_without_fusing_transfers() {
        let mut emu = fixture();
        let base = 0x0300_0200;
        for (index, raw) in [0x2101, 0x3101, 0x8001, 0x8802, 0xE7FB, 0x2101]
            .into_iter()
            .enumerate()
        {
            emu.bus.write16(base + index as u32 * 2, raw);
        }
        emu.cpu.set_pc(base);
        emu.cpu.step(&mut emu.bus).unwrap();
        emu.reset_profiling();
        let (_, direct_count) = emu
            .cpu
            .run_frame_direct(&mut emu.bus, u64::MAX, true, false)
            .unwrap();
        assert_eq!(direct_count, 1);
        assert_eq!(emu.cpu.profiling.thumb_macro_run_ends[STRH], 1);
        assert!(
            emu.cpu
                .run_frame_direct(&mut emu.bus, u64::MAX, true, false)
                .is_none()
        );
        assert_eq!(emu.cpu.profiling.thumb_macro_run_ends[STRH], 1);
        for _ in 0..3 {
            emu.cpu.step(&mut emu.bus).unwrap();
        }
        let snapshot = emu.profiling_snapshot();
        assert_eq!(snapshot.thumb_macro_counts, [0, 0, 0, 0, 1, 1, 1]);
        assert_eq!(snapshot.frame_cpu_direct_kinds, [1, 0, 0]);
        assert!(snapshot.cpu_phase_visits.iter().sum::<u64>() > 0);
        assert_eq!(snapshot.thumb_macro_neighbors[STRH], [0, 0, 0, 1]);
    }
}

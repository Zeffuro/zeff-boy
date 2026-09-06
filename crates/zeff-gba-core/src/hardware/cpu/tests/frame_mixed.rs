use super::frame_direct::{assert_cpu_equal, assert_projected_equal};
use super::*;
use crate::emulator::Emulator;

pub(super) const CODE_BASES: [u32; 5] = [
    0x0200_0200,
    0x0300_0200,
    0x0800_0200,
    0x0A00_0200,
    0x0C00_0200,
];

pub(super) fn fixture(base: u32, words: &[u32]) -> Emulator {
    let mut rom = vec![0; 0x4000];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    for (index, byte) in rom[0x3000..].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(37).wrapping_add(11);
    }
    let code: Vec<_> = std::iter::once(0xE1A0_0000)
        .chain(words.iter().copied())
        .chain([0xE1A0_0000; 3])
        .collect();
    if base >= 0x0800_0000 {
        let offset = (base & 0x01FF_FFFF) as usize;
        for (index, word) in code.iter().enumerate() {
            rom[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
    }
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    if base < 0x0800_0000 {
        for (index, word) in code.into_iter().enumerate() {
            emu.bus.write32(base + index as u32 * 4, word);
        }
    }
    for region in [0x0200_1000, 0x0300_1000] {
        for index in 0..64u32 {
            emu.bus.write32(
                region + index * 4,
                0xA517_293B ^ index.wrapping_mul(0x9E37_79B9),
            );
        }
    }
    emu.cpu.set_pc(base);
    emu.cpu.regs[0] = 0x0200_1080;
    emu.cpu.regs[1] = 0x81F2_7354;
    emu.cpu.regs[3] = 1;
    emu.cpu.step(&mut emu.bus).unwrap();
    emu
}

pub(super) fn oracle_step(phase: &mut Emulator, direct: &mut Emulator) -> bool {
    let expected = phase.cpu.step(&mut phase.bus);
    let accelerated = direct
        .cpu
        .run_frame_direct(&mut direct.bus, phase.cpu.cycles, true, true);
    let actual = if let Some((fetched, count)) = accelerated {
        assert_eq!(count, 1);
        Some(fetched)
    } else {
        direct.cpu.step(&mut direct.bus)
    };
    assert_eq!(actual, expected);
    assert_projected_equal(phase, direct);
    accelerated.is_some()
}

#[test]
fn frame_mixed_single_transfer_payload_preserves_register_and_writeback_domain() {
    use super::super::transfer::{NO_WRITEBACK, SingleTransfer};

    assert_eq!(size_of::<SingleTransfer>(), 16);
    let mut cpu = Cpu::new();
    cpu.regs = std::array::from_fn(|register| 0x0200_1000 + register as u32 * 16);
    let pc = 0x0800_0200;
    for rn in 0..16u32 {
        for rd in 0..16u32 {
            for flags in 0..32u32 {
                let pre = flags & 1 != 0;
                let up = flags & 2 != 0;
                let byte = flags & 4 != 0;
                let writeback = flags & 8 != 0;
                let load = flags & 16 != 0;
                let raw = 0xE400_0004
                    | rn << 16
                    | rd << 12
                    | u32::from(pre) << 24
                    | u32::from(up) << 23
                    | u32::from(byte) << 22
                    | u32::from(writeback) << 21
                    | u32::from(load) << 20;
                let plan = cpu.plan_arm_single_transfer(pc, raw).unwrap();
                let base = if rn == 15 {
                    pc + 8
                } else {
                    cpu.regs[rn as usize]
                };
                let indexed = if up { base + 4 } else { base - 4 };
                let has_writeback = (!pre || writeback) && !(load && rn == rd);
                assert_eq!(plan.destination, rd as u8);
                assert_eq!(plan.width, if byte { 1 } else { 4 });
                assert_eq!(plan.address, if pre { indexed } else { base });
                assert_eq!(
                    plan.writeback_register,
                    if has_writeback {
                        rn as u8
                    } else {
                        NO_WRITEBACK
                    }
                );
                assert_eq!(
                    plan.writeback_value,
                    if has_writeback { indexed } else { 0 }
                );
            }
        }
    }
    for raw in 0x8000..=0x8FFF {
        let plan = cpu.plan_thumb_halfword_transfer(raw);
        assert_eq!(plan.destination, (raw & 7) as u8);
        assert_eq!(plan.writeback_register, NO_WRITEBACK);
        assert_eq!(plan.writeback_value, 0);
    }
}

#[test]
fn frame_mixed_single_transfers_match_staged_address_value_and_writeback_rules() {
    for base in CODE_BASES {
        for index in 0..32u32 {
            let load = index & 16 != 0;
            let raw = 0xE400_0000
                | ((index & 1) << 24)
                | (((index >> 1) & 1) << 23)
                | (((index >> 2) & 1) << 22)
                | (((index >> 3) & 1) << 21)
                | (u32::from(load) << 20)
                | (1 << 12)
                | (index & 3);
            for data in [0x0200_1081, 0x0300_1081] {
                let mut phase = fixture(base, &[raw]);
                phase.cpu.regs[0] = data;
                phase.cpu.cpsr |= CPSR_CARRY;
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                assert!(oracle_step(&mut phase, &mut direct), "{base:08X} {raw:08X}");
            }
        }
        for raw in [
            0xE580_F000,
            0xE5C0_F000,
            0xE490_0004,
            0xE7A0_1083,
            0xE7B0_1063,
            0xE59F_1000,
            0xE5CF_1000,
        ] {
            let mut phase = fixture(base, &[raw]);
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            let accelerated = oracle_step(&mut phase, &mut direct);
            assert_eq!(accelerated, raw != 0xE5CF_1000 || base < 0x0800_0000);
        }
    }
}

#[test]
fn frame_mixed_rom_reads_and_refills_preserve_waitcnt_and_mirrors() {
    for base in CODE_BASES {
        for waitcnt in [0, 0x0417, 0x03FF] {
            for data in [0x0800_3001, 0x0A00_3002, 0x0C00_3003] {
                for raw in [0xE590_1000, 0xE5D0_1000, 0xEAFF_FFFE, 0xEBFF_FFFE] {
                    let mut phase = fixture(base, &[raw]);
                    phase.cpu.regs[0] = data;
                    phase.bus.write16(0x0400_0204, waitcnt);
                    let mut direct = phase.clone();
                    direct.bus.begin_frame_service();
                    assert!(oracle_step(&mut phase, &mut direct));
                }
            }
        }
    }
}

#[test]
fn frame_mixed_loops_collapse_cycle_submissions_and_phase_visits() {
    for base in CODE_BASES {
        let mut phase = fixture(base, &[0xE580_1000, 0xE281_1001, 0xEAFF_FFFC]);
        let mut direct = phase.clone();
        #[cfg(feature = "profiling")]
        {
            phase.reset_profiling();
            direct.reset_profiling();
        }
        phase.bus.begin_frame_service();
        direct.bus.begin_frame_service();
        let (last, count) = direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .unwrap();
        assert!(count >= 6, "mixed block must span loop iterations: {count}");
        let mut expected = None;
        for _ in 0..count {
            expected = phase.cpu.step(&mut phase.bus);
        }
        assert_eq!(expected, Some(last));
        let phase_calls = phase.bus.frame_service_stats_for_test().2;
        let direct_calls = direct.bus.frame_service_stats_for_test().2;
        assert_eq!(direct_calls, 1);
        assert!(phase_calls >= 10);
        eprintln!(
            "mixed {base:08X}: {count} instructions, {phase_calls} -> {direct_calls} cycle submissions"
        );
        #[cfg(feature = "profiling")]
        {
            let slow = phase.profiling_snapshot();
            let fast = direct.profiling_snapshot();
            assert_eq!(fast.cpu_phase_visits, [0; 8]);
            assert!(slow.cpu_phase_visits.iter().sum::<u64>() >= u64::from(count) * 4);
            assert_eq!(fast.frame_cpu_direct_instructions, u64::from(count));
            assert!(fast.frame_cpu_direct_kinds.iter().all(|count| *count >= 2));
            assert_eq!(fast.bus_step_calls, 1);
            assert_eq!(slow.bus_step_calls, phase_calls);
            assert_eq!(slow.instruction_fetches, fast.instruction_fetches);
            assert_eq!(
                slow.instruction_fetch_accesses,
                fast.instruction_fetch_accesses
            );
            eprintln!(
                "mixed phases: {} -> 0",
                slow.cpu_phase_visits.iter().sum::<u64>()
            );
        }
        phase.bus.end_frame_service();
        assert_projected_equal(&phase, &direct);
    }
}

#[test]
fn frame_mixed_self_modifying_ram_preserves_prefetched_and_future_words() {
    for base in [0x0200_0200, 0x0300_0200] {
        for target in [8, 12, 16] {
            let mut phase = fixture(
                base,
                &[
                    0xE580_7000,
                    0xE283_3001,
                    0xE283_3002,
                    0xE283_3004,
                    0xEAFF_FFFA,
                ],
            );
            phase.cpu.regs[0] = base + target;
            phase.cpu.regs[3] = 0;
            phase.cpu.regs[7] = 0xE283_3007;
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            let mut accelerated = 0;
            for index in 0..40 {
                accelerated += u32::from(oracle_step(&mut phase, &mut direct));
                if index == 3 {
                    assert_eq!(direct.cpu.regs[3], if target == 16 { 10 } else { 7 });
                }
            }
            assert!(accelerated > 30);
        }
    }
}

#[test]
fn frame_mixed_rejects_observable_transfers_and_pc_writeback_before_mutation() {
    for (raw, address) in [
        (0xE590_F000, 0x0200_1080),
        (0xE48F_1004, 0x0200_1080),
        (0xE590_1000, 0x0400_0100),
        (0xE580_1000, 0x0400_0100),
        (0xE5C0_1000, 0x0400_0301),
        (0xE580_1000, 0x0500_0000),
        (0xE590_1000, 0x0000_0000),
        (0xE590_1000, 0x0100_0000),
        (0xE590_1000, 0x0800_4000),
        (0xE590_1000, 0x0A00_4000),
        (0xE590_1000, 0x0C00_4000),
        (0xE580_1000, 0x0800_3000),
        (0xE590_1000, 0x0E00_0000),
        (0xE1D0_10B0, 0x0200_1080),
    ] {
        let mut phase = fixture(0x0300_0200, &[raw]);
        phase.cpu.regs[0] = address;
        phase.bus.write32(0x0200_1080, 0x0300_020C);
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(
            direct
                .cpu
                .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                .is_none()
        );
        assert_cpu_equal(&phase.cpu, &direct.cpu);
        assert_eq!(direct.bus.frame_service_stats_for_test().1, 0);
        assert!(!oracle_step(&mut phase, &mut direct));
    }
}

#[test]
fn frame_mixed_restored_transfer_and_refill_phases_resume_through_the_staged_engine() {
    for phases in 0..14 {
        let mut source = fixture(0x0300_0200, &[0xE580_1000, 0xE281_1001, 0xEAFF_FFFC]);
        for _ in 0..phases {
            source.cpu.step_cpu_phase_for_test(&mut source.bus);
        }
        let saved = source.encode_state().unwrap();
        let mut phase = source.clone();
        phase.load_state(&saved).unwrap();
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        if !direct.cpu.at_instruction_boundary() {
            assert!(
                direct
                    .cpu
                    .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                    .is_none()
            );
            assert_cpu_equal(&phase.cpu, &direct.cpu);
        }
        let mut accelerated = 0;
        for _ in 0..90 {
            accelerated += u32::from(oracle_step(&mut phase, &mut direct));
        }
        assert!(accelerated > 60, "restored phase {phases}");
    }
}

#[test]
fn frame_mixed_conditions_preserve_flags_and_suppress_memory_effects() {
    for flags in 0..16u32 {
        for condition in 0..16u32 {
            for body in [0x0580_1000, 0x0590_1000, 0x0AFF_FFFE, 0x0BFF_FFFE] {
                let raw = (condition << 28) | body;
                let mut phase = fixture(0x0300_0200, &[raw]);
                phase.cpu.cpsr = (phase.cpu.cpsr & 0x0FFF_FFFF) | (flags << 28);
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                assert!(oracle_step(&mut phase, &mut direct));
            }
        }
    }
}

#[test]
fn frame_mixed_crossing_transfers_and_refills_keep_the_original_phase_chunks() {
    for base in [0x0300_0200, 0x0800_0200, 0x0C00_0200] {
        for raw in [
            0xE580_1000,
            0xE590_1000,
            0xE8A0_0006,
            0xEAFF_FFFE,
            0xEBFF_FFFE,
        ] {
            for cut in 1..64u16 {
                let mut phase = fixture(base, &[raw]);
                phase.bus.write16(0x0400_0100, 0u16.wrapping_sub(cut));
                phase.bus.write16(0x0400_0102, 0xC0);
                let before = phase.cpu.clone();
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let budget = direct.bus.frame_cpu_cycle_budget();
                let expected = phase.cpu.step(&mut phase.bus).unwrap();
                let cycles = (phase.cpu.cycles - before.cycles) as u32;
                let result =
                    direct
                        .cpu
                        .run_frame_direct(&mut direct.bus, phase.cpu.cycles, true, true);
                assert_eq!(
                    result.is_some(),
                    cycles <= budget,
                    "{base:08X} {raw:08X} cut={cut}"
                );
                let actual = if let Some((fetched, count)) = result {
                    assert_eq!(count, 1);
                    fetched
                } else {
                    assert_cpu_equal(&before, &direct.cpu);
                    direct.cpu.step(&mut direct.bus).unwrap()
                };
                assert_eq!(actual, expected);
                assert_projected_equal(&phase, &direct);
            }
            for guard_adjustment in [-1i64, 0, 1] {
                let mut phase = fixture(base, &[raw]);
                let before = phase.cpu.clone();
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let expected = phase.cpu.step(&mut phase.bus).unwrap();
                let guard = phase.cpu.cycles.wrapping_add_signed(guard_adjustment);
                let result = direct
                    .cpu
                    .run_frame_direct(&mut direct.bus, guard, true, true);
                assert_eq!(result.is_some(), guard_adjustment >= 0);
                if let Some((fetched, count)) = result {
                    if guard_adjustment == 0 || count == 1 {
                        assert_eq!(count, 1);
                        assert_eq!(fetched, expected);
                        assert_projected_equal(&phase, &direct);
                    }
                } else {
                    assert_cpu_equal(&before, &direct.cpu);
                }
            }
        }
    }
}

#[test]
fn frame_mixed_prefixes_flush_at_original_timer_dma_and_halt_completion_points() {
    for (raw, address, value) in [
        (0xE594_6000, 0x0400_0100, 0),
        (0xE584_5000, 0x0400_0100, 0x0000_FE00),
        (0xE584_5000, 0x0400_0100, 0x0080_FE00),
        (0xE584_5000, 0x0400_00B8, 0x8400_0001),
        (0xE5C4_5000, 0x0400_0301, 0),
    ] {
        let mut phase = fixture(0x0300_0200, &[0xE580_1000, 0xE281_1001, raw]);
        phase.bus.write16(0x0400_0100, 0xFE00);
        phase.bus.write16(0x0400_0102, 0x80);
        phase.bus.write32(0x0400_00B0, 0x0200_1080);
        phase.bus.write32(0x0400_00B4, 0x0300_1080);
        phase.cpu.regs[4] = address;
        phase.cpu.regs[5] = value;
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        let (last, count) = direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .unwrap();
        assert_eq!(count, 2);
        phase.step_instruction();
        assert_eq!(phase.step_instruction(), Some(last));
        assert_projected_equal(&phase, &direct);
        assert_eq!(phase.step_instruction(), direct.step_instruction());
        assert_projected_equal(&phase, &direct);
        if address == 0x0400_00B8 {
            assert_eq!(direct.bus.read32(0x0300_1080), 0x81F2_7354);
        }
        if address == 0x0400_0301 {
            assert_eq!(direct.cpu.state, CpuState::Halted);
        }
    }
}

#[test]
fn frame_mixed_branch_to_pending_swi_return_exits_before_return_processing() {
    let base = 0x0300_0200;
    let mut phase = fixture(base, &[0xEA00_0000, 0xE1A0_0000, 0xE281_1001, 0xEAFF_FFFD]);
    phase.cpu.swi_wait_return_pc = Some(base + 12);
    let mut direct = phase.clone();
    direct.bus.begin_frame_service();
    let expected = phase.cpu.step(&mut phase.bus).unwrap();
    let (actual, count) = direct
        .cpu
        .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
        .unwrap();
    assert_eq!((actual, count), (expected, 1));
    assert_eq!(direct.cpu.pc(), base + 12);
    assert_eq!(direct.cpu.swi_wait_return_pc, Some(base + 12));
    assert_projected_equal(&phase, &direct);
    assert!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .is_none()
    );
    assert_cpu_equal(&phase.cpu, &direct.cpu);
}

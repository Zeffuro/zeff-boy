use super::frame_direct::{assert_cpu_equal, assert_projected_equal};
use super::frame_mixed::{CODE_BASES, oracle_step};
use super::*;
use crate::emulator::Emulator;

fn fixture(base: u32, instructions: &[u16]) -> Emulator {
    let mut rom = vec![0; 0x4000];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    rom[0x3000..].fill(0xA5);
    let code: Vec<_> = std::iter::once(0x46C0u16)
        .chain(instructions.iter().copied())
        .chain([0x46C0; 3])
        .collect();
    if base >= 0x0800_0000 {
        let offset = (base & 0x01FF_FFFF) as usize;
        for (index, raw) in code.iter().enumerate() {
            rom[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&raw.to_le_bytes());
        }
    }
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    if base < 0x0800_0000 {
        for (index, raw) in code.into_iter().enumerate() {
            emu.bus.write16(base + index as u32 * 2, raw);
        }
    }
    for region in [0x0200_1000, 0x0300_1000] {
        for offset in (0..128).step_by(2) {
            emu.bus.write16(region + offset, 0x81F2 ^ offset as u16);
        }
    }
    emu.cpu.cpsr |= CPSR_THUMB;
    emu.cpu.regs[0] = 0x0200_1000;
    emu.cpu.regs[1] = 0xB5E7_293B;
    emu.cpu.set_pc(base);
    emu.cpu.step(&mut emu.bus).unwrap();
    emu
}

#[test]
fn frame_thumb_halfwords_match_staged_alignment_offsets_and_register_overlap() {
    for base in CODE_BASES {
        for data in [
            0x0200_1000,
            0x0300_1001,
            0x0800_3001,
            0x0A00_3000,
            0x0C00_3001,
        ] {
            for offset in [0, 1, 31] {
                for (load, destination) in [(false, 1), (true, 1), (true, 0)] {
                    if !load && data >= 0x0800_0000 {
                        continue;
                    }
                    let raw = 0x8000 | u16::from(load) << 11 | offset << 6 | destination;
                    let mut phase = fixture(base, &[raw, 0x3201]);
                    phase.cpu.regs[0] = data;
                    let mut direct = phase.clone();
                    direct.bus.begin_frame_service();
                    assert!(
                        oracle_step(&mut phase, &mut direct),
                        "{base:08X} {data:08X} {raw:04X}"
                    );
                    assert_eq!(direct.cpu.pending_load_internal_cycle, load);
                    assert!(oracle_step(&mut phase, &mut direct));
                    assert!(!direct.cpu.pending_load_internal_cycle);
                }
            }
        }
    }
}

#[test]
fn frame_thumb_conditions_and_refills_match_flags_waitcnt_and_rom_mirrors() {
    for flags in 0..16 {
        for condition in 0..14 {
            let mut phase = fixture(0x0300_0200, &[0xD0FE | condition << 8]);
            phase.cpu.cpsr = (phase.cpu.cpsr & 0x0FFF_FFFF) | flags << 28;
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert!(oracle_step(&mut phase, &mut direct));
        }
    }
    for base in CODE_BASES {
        for waitcnt in [0, 0x0417, 0x03FF] {
            for raw in [0xD180, 0xD17F, 0xE000, 0xE400, 0xE3FF, 0xE7FE] {
                let mut phase = fixture(base, &[raw]);
                phase.bus.write16(0x0400_0204, waitcnt);
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let target = if raw == 0xE400 {
                    base.wrapping_sub(0x7FA)
                } else {
                    base
                };
                let expected_direct = raw != 0xE400 || base == 0x0300_0200;
                assert_eq!(
                    oracle_step(&mut phase, &mut direct),
                    expected_direct,
                    "{base:08X} {raw:04X} target {target:08X}"
                );
            }
        }
    }
}

#[test]
fn frame_thumb_mixed_loops_collapse_submissions_and_phase_visits() {
    for base in CODE_BASES {
        let mut phase = fixture(base, &[0x8001, 0x8802, 0x3101, 0x2900, 0xD001, 0xE7F9]);
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
        assert!(count >= 12, "{base:08X}: only {count} instructions");
        let mut expected = None;
        for _ in 0..count {
            expected = phase.cpu.step(&mut phase.bus);
        }
        assert_eq!(expected, Some(last));
        let phase_calls = phase.bus.frame_service_stats_for_test().2;
        assert_eq!(direct.bus.frame_service_stats_for_test().2, 1);
        eprintln!("Thumb {base:08X}: {count} instructions, {phase_calls} -> 1 cycle submissions");
        #[cfg(feature = "profiling")]
        {
            let slow = phase.profiling_snapshot();
            let fast = direct.profiling_snapshot();
            assert_eq!(fast.cpu_phase_visits, [0; 8]);
            assert_eq!(fast.bus_step_calls, 1);
            assert_eq!(fast.frame_cpu_direct_instructions, u64::from(count));
            assert!(fast.frame_cpu_direct_kinds.iter().all(|count| *count >= 2));
            assert_eq!(slow.instruction_fetches, fast.instruction_fetches);
            assert_eq!(
                slow.instruction_fetch_accesses,
                fast.instruction_fetch_accesses
            );
            assert_eq!(slow.thumb_macro_counts, fast.thumb_macro_counts);
            eprintln!(
                "Thumb phases: {} -> 0",
                slow.cpu_phase_visits.iter().sum::<u64>()
            );
        }
        phase.bus.end_frame_service();
        assert_projected_equal(&phase, &direct);
    }
}

#[test]
fn frame_thumb_horizon_and_guard_cutoffs_leave_crossing_instructions_unchanged() {
    for base in [0x0300_0200, 0x0800_0200, 0x0C00_0200] {
        for raw in [0x8001, 0x8801, 0xD0FE, 0xD1FE, 0xE7FE] {
            for cut in 1..24u16 {
                let mut phase = fixture(base, &[raw]);
                phase.bus.write16(0x0400_0100, 0u16.wrapping_sub(cut));
                phase.bus.write16(0x0400_0102, 0xC0);
                let before = phase.cpu.clone();
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let budget = direct.bus.frame_cpu_cycle_budget();
                let expected = phase.cpu.step(&mut phase.bus).unwrap();
                let cycles = phase.cpu.cycles - before.cycles;
                let result =
                    direct
                        .cpu
                        .run_frame_direct(&mut direct.bus, phase.cpu.cycles, true, true);
                assert_eq!(result.is_some(), cycles <= u64::from(budget));
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
            let mut phase = fixture(base, &[raw]);
            let before = phase.clone();
            phase.cpu.step(&mut phase.bus).unwrap();
            for adjustment in [-1i64, 0] {
                let mut direct = before.clone();
                direct.bus.begin_frame_service();
                let result = direct.cpu.run_frame_direct(
                    &mut direct.bus,
                    phase.cpu.cycles.wrapping_add_signed(adjustment),
                    true,
                    true,
                );
                assert_eq!(result.is_some(), adjustment == 0);
                if result.is_some() {
                    assert_projected_equal(&phase, &direct);
                } else {
                    assert_cpu_equal(&before.cpu, &direct.cpu);
                }
            }
        }
    }
}

#[test]
fn frame_thumb_rejects_unsupported_classes_and_observable_data_before_mutation() {
    for (raw, address) in [
        (0xDE00, 0x0200_1000),
        (0xDF00, 0x0200_1000),
        (0x4700, 0x0200_1000),
        (0xF000, 0x0200_1000),
        (0xF800, 0x0200_1000),
        (0x5801, 0x0200_1000),
        (0x5A01, 0x0200_1000),
        (0x6001, 0x0200_1000),
        (0xB401, 0x0200_1000),
        (0x8801, 0),
        (0x8801, 0x0100_0000),
        (0x8801, 0x0400_0100),
        (0x8001, 0x0500_0000),
        (0x8001, 0x0600_0000),
        (0x8001, 0x0700_0000),
        (0x8001, 0x0800_3000),
        (0x8801, 0x0800_4000),
        (0x8801, 0x0A00_4000),
        (0x8801, 0x0C00_4000),
        (0x8801, 0x0E00_0000),
    ] {
        let mut phase = fixture(0x0300_0200, &[raw]);
        phase.cpu.regs[0] = address;
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(
            direct
                .cpu
                .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                .is_none(),
            "{raw:04X} {address:08X}"
        );
        assert_cpu_equal(&phase.cpu, &direct.cpu);
        assert_eq!(direct.bus.frame_service_stats_for_test().1, 0);
    }
}

#[test]
fn frame_thumb_self_modifying_halfwords_preserve_queued_and_future_fetches() {
    for base in [0x0200_0200, 0x0300_0200] {
        for target in [4, 6, 8] {
            let mut phase = fixture(base, &[0x8007, 0x3301, 0x3302, 0x3304, 0xE7FA]);
            phase.cpu.regs[0] = base + target;
            phase.cpu.regs[7] = 0x3307;
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            let mut accelerated = 0;
            for index in 0..24 {
                accelerated += u32::from(oracle_step(&mut phase, &mut direct));
                if index == 3 {
                    assert_eq!(direct.cpu.regs[3], if target == 8 { 10 } else { 7 });
                }
            }
            assert!(accelerated > 16);
        }
    }
}

#[test]
fn frame_thumb_restored_transfer_and_refill_phases_stay_phase_owned_until_boundary() {
    for phases in 0..17 {
        let mut source = fixture(0x0300_0200, &[0x8001, 0x8802, 0x3101, 0xE7FB]);
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
        for _ in 0..24 {
            accelerated += u32::from(oracle_step(&mut phase, &mut direct));
        }
        assert!(accelerated > 16, "restored phase {phases}");
    }
}

#[test]
fn frame_thumb_prefixes_preserve_mmio_dma_halt_and_swi_return_boundaries() {
    for (address, value) in [
        (0x0400_0102, 0),
        (0x0400_0102, 0x80),
        (0x0400_00BA, 0x8400),
        (0x0400_0301, 0),
    ] {
        let terminal = if address == 0x0400_0301 {
            0x7025
        } else {
            0x8025
        };
        let mut phase = fixture(0x0300_0200, &[0x8001, 0x8802, 0x3101, terminal]);
        phase.bus.write16(0x0400_0100, 0xFE00);
        phase.bus.write16(0x0400_0102, 0x80);
        phase.bus.write32(0x0400_00B0, 0x0200_1000);
        phase.bus.write32(0x0400_00B4, 0x0300_1000);
        phase.bus.write16(0x0400_00B8, 1);
        phase.cpu.regs[4] = address;
        phase.cpu.regs[5] = value;
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        let (_, count) = direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .unwrap();
        assert_eq!(count, 3);
        for _ in 0..count {
            phase.step_instruction();
        }
        assert_projected_equal(&phase, &direct);
        assert_eq!(phase.step_instruction(), direct.step_instruction());
        assert_projected_equal(&phase, &direct);
        if address == 0x0400_00BA {
            assert_eq!(direct.bus.read16(0x0300_1000), 0x293B);
        }
        if address == 0x0400_0301 {
            assert_eq!(direct.cpu.state, CpuState::Halted);
        }
    }
    let base = 0x0300_0200;
    let mut phase = fixture(base, &[0xE000, 0x3101, 0x3102]);
    phase.cpu.swi_wait_return_pc = Some(base + 6);
    phase.cpu.swi_wait_mask = 8;
    phase.bus.write16(0x0300_7FF8, 8);
    let mut direct = phase.clone();
    direct.bus.begin_frame_service();
    assert_eq!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .unwrap()
            .1,
        1
    );
    phase.cpu.step(&mut phase.bus);
    assert_projected_equal(&phase, &direct);
    assert!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .is_none()
    );
    assert_eq!(
        phase.cpu.step(&mut phase.bus),
        direct.cpu.step(&mut direct.bus)
    );
    assert_eq!(direct.cpu.swi_wait_mask, 0);
    assert_projected_equal(&phase, &direct);
    direct.bus.write16(0x0400_0200, 8);
    direct.bus.write16(0x0400_0208, 1);
    direct.bus.request_interrupt(8);
    direct.bus.step_cycles(7);
    direct.cpu.cycles += 7;
    direct.bus.materialize_frame_service();
    direct.cpu.cpsr &= !CPSR_IRQ_DISABLE;
    assert!(direct.bus.interrupt_ready());
    assert!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .is_none()
    );
}

use super::frame_direct::{assert_cpu_equal, assert_projected_equal};
use super::frame_mixed;
use super::*;
use crate::emulator::Emulator;
use crate::hardware::cpu::frame_fetch::FrameFetchWindow;

pub(super) fn fixture(instruction_set: InstructionSet, base: u32) -> Emulator {
    if instruction_set == InstructionSet::Arm {
        return frame_mixed::fixture(base, &[0xE580_1000, 0xE281_1001, 0xE590_2000, 0xEAFF_FFFB]);
    }
    let mut rom = vec![0; 0x4000];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    let code = [0x2000u16, 0x8001, 0x3101, 0x8802, 0xE7FB, 0x2000, 0x2000];
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
    emu.cpu.cpsr |= CPSR_THUMB;
    emu.cpu.set_pc(base);
    emu.cpu.step(&mut emu.bus).unwrap();
    emu.cpu.regs[0] = 0x0200_1081;
    emu.cpu.regs[1] = 0x81F2_7354;
    emu
}

pub(super) fn selected_step(
    emu: &mut Emulator,
    guard: u64,
    enabled: bool,
) -> Option<FetchedInstruction> {
    match emu
        .cpu
        .run_frame_direct_with_fetch(&mut emu.bus, guard, true, true, enabled)
    {
        Some((fetched, count)) => {
            assert_eq!(count, 1);
            Some(fetched)
        }
        None => emu.cpu.step(&mut emu.bus),
    }
}

#[test]
fn frame_fetch_matches_each_instruction_and_live_waitcnt_in_all_regions() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for base in frame_mixed::CODE_BASES.into_iter().chain([
            0x0204_0200,
            0x0300_8200,
            0x0203_FFFC,
            0x0300_7FFC,
        ]) {
            let mut phase = fixture(instruction_set, base);
            let mut generic = phase.clone();
            let mut fast = phase.clone();
            generic.bus.begin_frame_service();
            fast.bus.begin_frame_service();
            for index in 0..160 {
                if index % 32 == 0 {
                    let waitcnt = [0, 0x0417, 0x03FF, 0x47FF, 0x7FFC][index / 32];
                    phase.bus.write16(0x0400_0204, waitcnt);
                    generic.bus.write16(0x0400_0204, waitcnt);
                    fast.bus.write16(0x0400_0204, waitcnt);
                }
                let expected = phase.cpu.step(&mut phase.bus);
                assert_eq!(
                    selected_step(&mut generic, phase.cpu.cycles, false),
                    expected
                );
                assert_eq!(selected_step(&mut fast, phase.cpu.cycles, true), expected);
                assert_projected_equal(&phase, &generic);
                assert_projected_equal(&phase, &fast);
            }
            assert_eq!(generic.cpu.gamepak_block_fetches, 0);
            assert_eq!(generic.cpu.ram_block_fetches, [0; 2]);
            if base >= 0x0800_0000 {
                assert!(fast.cpu.gamepak_block_fetches > 100);
                assert_eq!(fast.cpu.ram_block_fetches, [0; 2]);
            } else {
                assert_eq!(fast.cpu.gamepak_block_fetches, 0);
                assert!(fast.cpu.ram_block_fetches[usize::from(base >= 0x0300_0000)] > 100);
            }
        }
    }
}

#[test]
fn frame_fetch_keeps_restored_pipeline_raw_and_metadata_authoritative() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for base in frame_mixed::CODE_BASES {
            let mut phase = fixture(instruction_set, base);
            let mut pipeline = phase.cpu.pipeline_state();
            pipeline.entries[0].raw = match instruction_set {
                InstructionSet::Arm => 0xE3A0_3055,
                InstructionSet::Thumb => 0x2355,
            };
            pipeline.entries[1].raw = match instruction_set {
                InstructionSet::Arm => 0xE283_3011,
                InstructionSet::Thumb => 0x3311,
            };
            pipeline.pending_load_internal_cycle = true;
            assert!(phase.cpu.set_pipeline_state(pipeline));
            let saved = phase.encode_state().unwrap();
            phase.load_state(&saved).unwrap();
            let mut fast = phase.clone();
            fast.bus.begin_frame_service();
            for index in 0..12 {
                let expected = phase.cpu.step(&mut phase.bus);
                assert_eq!(selected_step(&mut fast, phase.cpu.cycles, true), expected);
                assert_projected_equal(&phase, &fast);
                if index < 2 {
                    assert_eq!(fast.cpu.regs[3], [0x55, 0x66][index]);
                    assert!(!fast.cpu.pending_load_internal_cycle);
                }
            }
            assert!(
                fast.cpu.gamepak_block_fetches + fast.cpu.ram_block_fetches.iter().sum::<u64>() > 8
            );
        }
    }
}

#[test]
fn frame_fetch_merges_blocks_without_crossing_horizon_or_guard() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for base in frame_mixed::CODE_BASES {
            for guard_offset in [0, 1, 2, 3, 5, 6, 7, 11, 31, 127, 1000, u32::MAX] {
                let mut generic = fixture(instruction_set, base);
                let mut fast = generic.clone();
                generic.bus.begin_frame_service();
                fast.bus.begin_frame_service();
                #[cfg(feature = "profiling")]
                {
                    generic.reset_profiling();
                    fast.reset_profiling();
                }
                let guard = fast.cpu.cycles + u64::from(guard_offset);
                let expected = generic.cpu.run_frame_direct_with_fetch(
                    &mut generic.bus,
                    guard,
                    true,
                    true,
                    false,
                );
                let actual =
                    fast.cpu
                        .run_frame_direct_with_fetch(&mut fast.bus, guard, true, true, true);
                assert_eq!(actual, expected);
                assert_cpu_equal(&generic.cpu, &fast.cpu);
                assert_eq!(
                    generic.encode_state().unwrap(),
                    fast.encode_state().unwrap()
                );
                if let Some((_, count)) = actual {
                    assert_eq!(
                        fast.cpu.gamepak_block_fetches
                            + fast.cpu.ram_block_fetches.iter().sum::<u64>(),
                        u64::from(count)
                    );
                    assert_eq!(fast.bus.frame_service_stats_for_test().2, 1);
                    #[cfg(feature = "profiling")]
                    {
                        let before = generic.profiling_snapshot();
                        let after = fast.profiling_snapshot();
                        assert_eq!(before.cpu_generic_fetch_decode_calls, 0);
                        assert_eq!(after.cpu_generic_fetch_decode_calls, 0);
                        assert_eq!(
                            after.cpu_gamepak_block_fetches
                                + after.cpu_ram_block_fetches.iter().sum::<u64>(),
                            u64::from(count)
                        );
                        assert_eq!(before.instruction_fetches, after.instruction_fetches);
                        assert_eq!(
                            before.instruction_fetch_accesses,
                            after.instruction_fetch_accesses
                        );
                        assert_eq!(before.cpu_phase_visits, after.cpu_phase_visits);
                    }
                } else {
                    assert_eq!(fast.cpu.gamepak_block_fetches, 0);
                    assert_eq!(fast.cpu.ram_block_fetches, [0; 2]);
                }
            }
        }
    }
}

#[test]
fn frame_fetch_preserves_cross_mirror_refill_and_swi_return_fence() {
    for (base, target) in [
        (0x0800_0200u32, 0x0A00_0200u32),
        (0x0A00_0200, 0x0800_0210),
        (0x0200_0200, 0x0300_8200),
        (0x0300_8200, 0x0204_0200),
    ] {
        let branch = 0xEA00_0000 | ((target.wrapping_sub(base + 12) >> 2) & 0x00FF_FFFF);
        let mut generic = frame_mixed::fixture(base, &[branch]);
        generic.cpu.swi_wait_return_pc = Some(target);
        let mut fast = generic.clone();
        generic.bus.begin_frame_service();
        fast.bus.begin_frame_service();
        let expected =
            generic
                .cpu
                .run_frame_direct_with_fetch(&mut generic.bus, u64::MAX, true, true, false);
        let actual =
            fast.cpu
                .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true);
        assert_eq!(actual, expected);
        assert_eq!(actual.unwrap().1, 1);
        assert_eq!(
            fast.cpu.gamepak_block_fetches + fast.cpu.ram_block_fetches.iter().sum::<u64>(),
            1
        );
        assert_cpu_equal(&generic.cpu, &fast.cpu);
        assert_eq!(fast.cpu.pc(), target);
        assert!(
            fast.cpu
                .run_frame_direct(&mut fast.bus, u64::MAX, true, true)
                .is_none()
        );
        assert_eq!(
            generic.cpu.step(&mut generic.bus),
            fast.cpu.step(&mut fast.bus)
        );
        assert_cpu_equal(&generic.cpu, &fast.cpu);
        assert_eq!(
            generic.encode_state().unwrap(),
            fast.encode_state().unwrap()
        );
    }
}

#[test]
fn frame_fetch_excludes_rtc_eeprom_and_open_bus_windows() {
    let mut rom = vec![0; 0x0200_0000];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    rom[0x180..0x188].copy_from_slice(b"EEPROM_V");
    let bus = Bus::new(Cartridge::load(&rom).unwrap(), 48_000);
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for pc in [0x0800_0200, 0x0DFF_FEFA, 0x0DFF_FEFC, 0x0DFF_FF00] {
            assert!(FrameFetchWindow::new(&bus, pc, instruction_set, 4).is_none());
        }
    }
    drop(bus);
    rom.truncate(0x0201);
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    let rtc = Bus::new(Cartridge::load(&rom).unwrap(), 48_000);
    for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
        assert!(FrameFetchWindow::new(&rtc, base + 0x100, InstructionSet::Arm, 8).is_none());
    }
    rom[0xAC..0xB0].fill(0);
    rom[0x180..0x188].fill(0);
    for length in 0x0201..0x0209 {
        rom.resize(length, 0);
        for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
            for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
                let mut phase = Emulator::new(&rom, 48_000).unwrap();
                if instruction_set == InstructionSet::Thumb {
                    phase.cpu.cpsr |= CPSR_THUMB;
                }
                phase.cpu.set_pc(base + 0x1F0);
                phase.cpu.step(&mut phase.bus).unwrap();
                let mut fast = phase.clone();
                fast.bus.begin_frame_service();
                for _ in 0..12 {
                    let expected = phase.cpu.step(&mut phase.bus);
                    assert_eq!(selected_step(&mut fast, phase.cpu.cycles, true), expected);
                    assert_projected_equal(&phase, &fast);
                }
                assert!(
                    fast.cpu.gamepak_block_fetches > 0,
                    "{instruction_set:?} {base:08X} length {length:X}"
                );
            }
        }
    }
}

#[test]
fn frame_fetch_preserves_frame_audio_state_tas_and_phase_restore() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for base in [0x0204_0200, 0x0300_8200, 0x0800_0200] {
            let mut phase = fixture(instruction_set, base);
            for (address, value) in [
                (0x0400_0084, 0x80),
                (0x0400_0080, 0xFF77),
                (0x0400_0068, 0xF080),
                (0x0400_006C, 0x87C3),
                (0x0400_0100, 0xFF00),
                (0x0400_0102, 0x80),
            ] {
                phase.bus.write16(address, value);
            }
            let mut fast = phase.clone();
            for frame in 0..4 {
                if frame == 2 {
                    phase.cpu.step_cpu_phase_for_test(&mut phase.bus);
                    let saved = phase.encode_state().unwrap();
                    phase.load_state(&saved).unwrap();
                    fast.load_state(&saved).unwrap();
                }
                phase.eager_service_step_frame();
                fast.step_frame();
                let expected = phase.encode_state().unwrap();
                let actual = fast.encode_state().unwrap();
                assert_eq!(actual, expected);
                assert_eq!(
                    crate::save_state::inspect_current_native_gba_tas_state(&phase, &expected)
                        .unwrap(),
                    crate::save_state::inspect_current_native_gba_tas_state(&fast, &actual)
                        .unwrap(),
                );
                assert_eq!(phase.framebuffer(), fast.framebuffer());
                let mut first_audio = Vec::new();
                let mut second_audio = Vec::new();
                phase.drain_audio_samples_into(&mut first_audio);
                fast.drain_audio_samples_into(&mut second_audio);
                assert_eq!(
                    first_audio
                        .iter()
                        .map(|sample| sample.to_bits())
                        .collect::<Vec<_>>(),
                    second_audio
                        .iter()
                        .map(|sample| sample.to_bits())
                        .collect::<Vec<_>>()
                );
            }
            assert!(
                fast.cpu.gamepak_block_fetches + fast.cpu.ram_block_fetches.iter().sum::<u64>()
                    > 10_000
            );
        }
    }
}

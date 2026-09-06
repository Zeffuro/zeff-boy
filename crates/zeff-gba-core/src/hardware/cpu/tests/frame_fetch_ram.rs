use super::frame_direct::{assert_cpu_equal, assert_projected_equal};
use super::frame_fetch::{fixture, selected_step};
use super::frame_mixed;
use super::*;
use crate::emulator::Emulator;
use crate::hardware::cpu::frame_fetch::FrameFetchWindow;

fn program(instruction_set: InstructionSet, base: u32, words: &[u32]) -> Emulator {
    if instruction_set == InstructionSet::Arm {
        return frame_mixed::fixture(base, words);
    }
    let mut emu = fixture(InstructionSet::Thumb, base);
    for (index, raw) in std::iter::once(0x2000)
        .chain(words.iter().copied())
        .chain([0x46C0; 3])
        .enumerate()
    {
        emu.bus.write16(base + index as u32 * 2, raw as u16);
    }
    emu.cpu.set_pc(base);
    emu.cpu.step(&mut emu.bus).unwrap();
    emu
}

fn oracle_step(phase: &mut Emulator, fast: &mut Emulator) {
    let expected = phase.step_instruction();
    let actual = selected_step(fast, phase.cpu.cycles, true);
    let dma_cycles = fast.bus.take_pending_dma_cycles();
    if dma_cycles != 0 {
        fast.cpu.cycles = fast.cpu.cycles.wrapping_add(u64::from(dma_cycles));
        fast.bus.step_cycles(dma_cycles);
    }
    assert_eq!(actual, expected);
    assert_projected_equal(phase, fast);
}

#[test]
fn frame_fetch_ram_cpu_stores_keep_queued_and_future_mirror_words_distinct() {
    for (base, mirror) in [
        (0x0200_0200, 0x0004_0000),
        (0x0300_0200, 0x0000_8000),
        (0x0203_FFF8, 0x0004_0000),
        (0x0300_7FF8, 0x0000_8000),
    ] {
        for (instruction_set, store, replacement) in [
            (InstructionSet::Arm, 0xE580_7000, 0xE283_3007),
            (InstructionSet::Arm, 0xE5C0_7000, 7),
            (InstructionSet::Thumb, 0x8007, 0x3307),
        ] {
            for target in [2, 3, 4] {
                let words = match instruction_set {
                    InstructionSet::Arm => {
                        [store, 0xE283_3001, 0xE283_3002, 0xE283_3004, 0xEAFF_FFFA]
                    }
                    InstructionSet::Thumb => [store, 0x3301, 0x3302, 0x3304, 0xE7FA],
                };
                let mut phase = program(instruction_set, base, &words);
                phase.cpu.regs[0] =
                    (base + target * u32::from(instruction_set.width_bytes())) ^ mirror;
                phase.cpu.regs[3] = 0;
                phase.cpu.regs[7] = replacement;
                let mut fast = phase.clone();
                fast.bus.begin_frame_service();
                for index in 0..20 {
                    oracle_step(&mut phase, &mut fast);
                    if index == 3 {
                        assert_eq!(fast.cpu.regs[3], if target == 4 { 10 } else { 7 });
                    }
                }
                assert!(fast.cpu.ram_block_fetches.iter().sum::<u64>() > 15);
            }
        }
    }
}

#[test]
fn frame_fetch_ram_region_edges_fall_back_before_crossing() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        let width = u32::from(instruction_set.width_bytes());
        for end in [0x0300_0000, 0x0400_0000] {
            let base = end - 32;
            let mut generic = fixture(instruction_set, base);
            let first = end - 6 * width;
            for address in (first..end).step_by(width as usize) {
                if instruction_set == InstructionSet::Arm {
                    generic.bus.write32(address, 0xE283_3001);
                } else {
                    generic.bus.write16(address, 0x3301);
                }
            }
            generic.cpu.set_pc(first);
            generic.cpu.step(&mut generic.bus).unwrap();
            let mut fast = generic.clone();
            generic.bus.begin_frame_service();
            fast.bus.begin_frame_service();
            let expected = generic.cpu.run_frame_direct_with_fetch(
                &mut generic.bus,
                u64::MAX,
                true,
                true,
                false,
            );
            let actual =
                fast.cpu
                    .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true);
            assert_eq!(actual, expected);
            assert!(actual.unwrap().1 >= 2);
            assert_cpu_equal(&generic.cpu, &fast.cpu);
            assert_eq!(
                generic.encode_state().unwrap(),
                fast.encode_state().unwrap()
            );
            assert!(
                FrameFetchWindow::new(&fast.bus, end - 2 * width, instruction_set, 1).is_none()
            );
            assert!(
                fast.cpu
                    .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true)
                    .is_none()
            );
            for _ in 0..3 {
                let expected = generic.cpu.step(&mut generic.bus);
                assert_eq!(fast.cpu.step(&mut fast.bus), expected);
                assert_cpu_equal(&generic.cpu, &fast.cpu);
                assert_eq!(
                    generic.encode_state().unwrap(),
                    fast.encode_state().unwrap()
                );
            }
        }
    }
}

#[test]
fn frame_fetch_ram_hblank_dma_stays_beyond_the_quiet_prefix() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for (base, mirror) in [(0x0200_0200, 0x0004_0000), (0x0300_0200, 0x0000_8000)] {
            for target in [1, 3, 4] {
                let words = match instruction_set {
                    InstructionSet::Arm => [0xE283_3001; 16],
                    InstructionSet::Thumb => [0x3301; 16],
                };
                let mut phase = program(instruction_set, base, &words);
                phase.cpu.cpsr |= CPSR_IRQ_DISABLE;
                phase.set_apu_sample_generation_enabled(false);
                let mut probe = phase.clone();
                probe.cpu.step(&mut probe.bus).unwrap();
                let instruction_cycles = (probe.cpu.cycles - phase.cpu.cycles) as u32;
                let until_dma = instruction_cycles * 4;
                let patch = if instruction_set == InstructionSet::Arm {
                    0xE283_3007
                } else {
                    0x3307
                };
                let destination =
                    (base + (3 + target) * u32::from(instruction_set.width_bytes())) ^ mirror;
                phase.bus.write32(0x0200_1000, patch);
                phase.bus.write32(0x0400_00B0, 0x0200_1000);
                phase.bus.write32(0x0400_00B4, destination);
                phase.bus.write16(0x0400_00B8, 1);
                phase.bus.write16(
                    0x0400_00BA,
                    if instruction_set == InstructionSet::Arm {
                        0xE400
                    } else {
                        0xE000
                    },
                );
                let advance = phase.bus.ppu.cycles_until_next_status_event() - until_dma;
                phase.bus.step_cycles(advance);
                phase.cpu.cycles += u64::from(advance);
                let mut fast = phase.clone();
                fast.bus.begin_frame_service();
                let budget = fast.bus.frame_cpu_cycle_budget();
                assert_eq!(budget, until_dma - 1);
                let (_, count) = fast
                    .cpu
                    .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true)
                    .unwrap();
                assert_eq!(count, 3);
                assert_eq!(
                    fast.bus.frame_service_stats_for_test().1,
                    instruction_cycles * 3
                );
                assert_ne!(fast.bus.peek16(destination), patch as u16);
                for _ in 0..count {
                    phase.step_instruction();
                }
                assert_projected_equal(&phase, &fast);
                assert_eq!(fast.bus.frame_cpu_cycle_budget(), instruction_cycles - 1);
                assert!(
                    fast.cpu
                        .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true)
                        .is_none()
                );
                for _ in 0..20 {
                    oracle_step(&mut phase, &mut fast);
                }
                assert_eq!(fast.bus.peek16(destination), patch as u16);
                assert_ne!(fast.bus.peek16(0x0400_0202) & (1 << 8), 0);
                assert!(fast.cpu.ram_block_fetches.iter().sum::<u64>() > 10);
            }
        }
    }
}

#[test]
fn frame_fetch_ram_rom_exchange_uses_generic_refill_then_live_region() {
    for source_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for target_set in [InstructionSet::Arm, InstructionSet::Thumb] {
            for (source, target) in [
                (0x0204_0200, 0x0800_0200),
                (0x0300_8200, 0x0800_0200),
                (0x0800_0200, 0x0204_0200),
                (0x0800_0200, 0x0300_8200),
            ] {
                let source_words = match source_set {
                    InstructionSet::Arm => [0xE1A0_0000, 0xE283_3001, 0xE12F_FF14, 0xE283_3002],
                    InstructionSet::Thumb => [0x46C0, 0x3301, 0x4720, 0x3302],
                };
                let target_words = match target_set {
                    InstructionSet::Arm => [0xE283_3004, 0xE283_3008, 0xE283_3010, 0xEAFF_FFFE],
                    InstructionSet::Thumb => [0x3304, 0x3308, 0x3310, 0xE7FE],
                };
                let mut rom = vec![0; 0x4000];
                rom[0xA0..0xA4].copy_from_slice(b"TEST");
                rom[0xB2] = 0x96;
                let code = [
                    (source, source_set, source_words),
                    (target, target_set, target_words),
                ];
                for (base, instruction_set, words) in code {
                    if base >= 0x0800_0000 {
                        let width = usize::from(instruction_set.width_bytes());
                        for (index, raw) in words.iter().enumerate() {
                            let offset = (base & 0x01FF_FFFF) as usize + index * width;
                            rom[offset..offset + width]
                                .copy_from_slice(&u32::to_le_bytes(*raw)[..width]);
                        }
                    }
                }
                let mut phase = Emulator::new(&rom, 48_000).unwrap();
                for (base, instruction_set, words) in code {
                    if base < 0x0800_0000 {
                        for (index, raw) in words.into_iter().enumerate() {
                            let address =
                                base + index as u32 * u32::from(instruction_set.width_bytes());
                            if instruction_set == InstructionSet::Arm {
                                phase.bus.write32(address, raw);
                            } else {
                                phase.bus.write16(address, raw as u16);
                            }
                        }
                    }
                }
                if source_set == InstructionSet::Thumb {
                    phase.cpu.cpsr |= CPSR_THUMB;
                }
                phase.cpu.regs[4] = target | u32::from(target_set == InstructionSet::Thumb);
                phase.cpu.set_pc(source);
                phase.cpu.step(&mut phase.bus).unwrap();
                let mut fast = phase.clone();
                fast.bus.begin_frame_service();
                for index in 0..12 {
                    let before = fast.cpu.gamepak_block_fetches
                        + fast.cpu.ram_block_fetches.iter().sum::<u64>();
                    oracle_step(&mut phase, &mut fast);
                    if index == 1 {
                        assert_eq!(fast.cpu.pc(), target);
                        assert_eq!(fast.cpu.instruction_set(), target_set);
                        assert_eq!(
                            fast.cpu.gamepak_block_fetches
                                + fast.cpu.ram_block_fetches.iter().sum::<u64>(),
                            before
                        );
                    }
                }
                assert!(fast.cpu.gamepak_block_fetches > 0);
                assert!(fast.cpu.ram_block_fetches.iter().sum::<u64>() > 0);
            }
        }
    }
}

#[test]
fn frame_fetch_ram_debug_reads_stay_generic_and_traced() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for base in [0x0204_0200, 0x0300_8200] {
            let mut phase = fixture(instruction_set, base);
            phase.bus.debug_trace_enabled = true;
            phase.bus.debug_trace_reads = true;
            let mut fast = phase.clone();
            fast.bus.begin_frame_service();
            assert!(
                fast.cpu
                    .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true)
                    .is_none()
            );
            assert_eq!(fast.cpu.ram_block_fetches, [0; 2]);
            assert_eq!(phase.cpu.step(&mut phase.bus), fast.cpu.step(&mut fast.bus));
            assert_projected_equal(&phase, &fast);
            assert!(!fast.bus.debug_trace_events.borrow().is_empty());
            assert_eq!(
                *phase.bus.debug_trace_events.borrow(),
                *fast.bus.debug_trace_events.borrow()
            );
        }
    }
}

#[test]
fn frame_fetch_ram_immediate_dma_keeps_prefetch_order_and_charges_before_resume() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for (base, mirror) in [(0x0200_0200, 0x0004_0000), (0x0300_0200, 0x0000_8000)] {
            for target in [4, 5, 6] {
                let words = match instruction_set {
                    InstructionSet::Arm => [
                        0xE283_3001,
                        0xE283_3002,
                        0xE580_1000,
                        0xE283_3004,
                        0xE283_3008,
                        0xEAFF_FFF9,
                    ],
                    InstructionSet::Thumb => [0x3301, 0x3302, 0x6001, 0x3304, 0x3308, 0xE7F9],
                };
                let mut phase = program(instruction_set, base, &words);
                phase.cpu.cpsr |= CPSR_IRQ_DISABLE;
                let patch = if instruction_set == InstructionSet::Arm {
                    0xE283_3007
                } else {
                    0x3307
                };
                let destination =
                    (base + target * u32::from(instruction_set.width_bytes())) ^ mirror;
                phase.bus.write32(0x0200_1000, patch);
                phase.bus.write32(0x0400_00B0, 0x0200_1000);
                phase.bus.write32(0x0400_00B4, destination);
                phase.cpu.regs[0] = 0x0400_00B8;
                phase.cpu.regs[1] = if instruction_set == InstructionSet::Arm {
                    0xC400_0001
                } else {
                    0xC000_0001
                };
                let mut fast = phase.clone();
                fast.bus.begin_frame_service();
                let (_, count) = fast
                    .cpu
                    .run_frame_direct_with_fetch(&mut fast.bus, u64::MAX, true, true, true)
                    .unwrap();
                assert_eq!(count, 2);
                assert_ne!(fast.bus.peek16(destination), patch as u16);
                phase.step_instruction();
                phase.step_instruction();
                assert_projected_equal(&phase, &fast);
                for index in 0..12 {
                    oracle_step(&mut phase, &mut fast);
                    if index == 0 {
                        assert_eq!(fast.bus.peek16(destination), patch as u16);
                        assert_ne!(fast.bus.peek16(0x0400_0202) & (1 << 8), 0);
                        assert_eq!(fast.bus.take_pending_dma_cycles(), 0);
                    }
                }
                assert!(fast.cpu.ram_block_fetches.iter().sum::<u64>() > 5);
            }
        }
    }
}

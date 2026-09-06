use super::*;
use crate::emulator::Emulator;

fn fixture(instruction_set: InstructionSet, base: u32) -> Emulator {
    let mut rom = vec![0; 0x4000];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    let mut words = Vec::new();
    let mut seed = 0x4C91_752Bu32;
    for index in 0..64u32 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let raw = match instruction_set {
            InstructionSet::Arm => {
                let operand = if index % 3 == 0 {
                    (1 << 25) | (seed & 0xFFF)
                } else if index % 3 == 1 {
                    (seed & 0xF) | (((seed >> 8) & 0xF) << 8) | (1 << 4) | (((seed >> 16) & 3) << 5)
                } else {
                    (seed & 0xF) | (((seed >> 8) & 31) << 7) | (((seed >> 16) & 3) << 5)
                };
                ((index % 16) << 28)
                    | ((index % 16) << 21)
                    | (((index >> 4) & 1) << 20)
                    | (((seed >> 20) & 15) << 16)
                    | (((seed >> 24) % 15) << 12)
                    | operand
            }
            InstructionSet::Thumb => match index % 6 {
                0 => (seed & 0x7FF) | ((index % 3) << 11),
                1 => 0x1800 | (seed & 0x7FF),
                2 => 0x2000 | (seed & 0x1FFF),
                3 => 0x4000 | (seed & 0x3FF),
                4 => 0xA000 | (seed & 0xFFF),
                _ => 0xB000 | (seed & 0xFF),
            },
        };
        words.push(raw);
    }
    words.push(match instruction_set {
        InstructionSet::Arm => 0xEAFF_FFBE,
        InstructionSet::Thumb => 0xE7BE,
    });
    let width = usize::from(instruction_set.width_bytes());
    if base >= 0x0800_0000 {
        let offset = (base & 0x01FF_FFFF) as usize;
        for (index, raw) in words.iter().enumerate() {
            rom[offset + index * width..offset + (index + 1) * width]
                .copy_from_slice(&raw.to_le_bytes()[..width]);
        }
    }
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    if base < 0x0800_0000 {
        for (index, raw) in words.into_iter().enumerate() {
            if instruction_set == InstructionSet::Arm {
                emu.bus.write32(base + index as u32 * 4, raw);
            } else {
                emu.bus.write16(base + index as u32 * 2, raw as u16);
            }
        }
    }
    emu.cpu.set_pc(base);
    if instruction_set == InstructionSet::Thumb {
        emu.cpu.cpsr |= CPSR_THUMB;
    }
    for register in 0..15 {
        emu.cpu.regs[register] = (register as u32).wrapping_mul(0x71B6_D943);
    }
    for (address, value) in [
        (0x0400_0084, 0x80),
        (0x0400_0080, 0xFF77),
        (0x0400_0068, 0xF080),
        (0x0400_006C, 0x87C3),
        (0x0400_0100, 0xFF00),
        (0x0400_0102, 0x80),
    ] {
        emu.bus.write16(address, value);
    }
    emu
}

pub(super) fn assert_cpu_equal(first: &Cpu, second: &Cpu) {
    assert_eq!(first.regs, second.regs);
    assert_eq!(first.cpsr, second.cpsr);
    assert_eq!(first.spsr, second.spsr);
    assert_eq!(first.cycles, second.cycles);
    assert_eq!(first.state, second.state);
    assert_eq!(first.execution_state(), second.execution_state());
    assert_eq!(first.active_decoded, second.active_decoded);
    assert_eq!(first.pipeline.entries, second.pipeline.entries);
    assert_eq!(first.pipeline_state(), second.pipeline_state());
    assert_eq!(first.last_fetch, second.last_fetch);
    assert_eq!(first.last_opcode_pc, second.last_opcode_pc);
    assert_eq!(first.next_fetch_sequential, second.next_fetch_sequential);
    assert_eq!(
        first.bios_protected_read_latch,
        second.bios_protected_read_latch
    );
    assert_eq!(first.swi_wait_return_pc, second.swi_wait_return_pc);
    assert_eq!(first.swi_wait_mask, second.swi_wait_mask);
    assert_eq!(first.break_after_next_stub, second.break_after_next_stub);
    assert_eq!(first.banked_sp, second.banked_sp);
    assert_eq!(first.banked_lr, second.banked_lr);
    assert_eq!(first.banked_spsr, second.banked_spsr);
    assert_eq!(first.banked_r8_r12, second.banked_r8_r12);
    assert_eq!(
        first.instruction_fetch_cycles,
        second.instruction_fetch_cycles
    );
    assert_eq!(first.bus_phase_cycles, second.bus_phase_cycles);
    assert_eq!(
        first.data_access_timing_active,
        second.data_access_timing_active
    );
    assert_eq!(first.hle_data_accesses, second.hle_data_accesses);
    assert_eq!(
        first.data_access_cursor.elapsed_cycles(),
        second.data_access_cursor.elapsed_cycles()
    );
    assert_eq!(
        first.data_access_cursor.access_count(),
        second.data_access_cursor.access_count()
    );
    assert_eq!(first.instruction_timeline(), second.instruction_timeline());
    assert_eq!(
        first.timer_io_completion_events(),
        second.timer_io_completion_events()
    );
}

pub(super) fn assert_projected_equal(first: &Emulator, second: &Emulator) {
    assert_cpu_equal(&first.cpu, &second.cpu);
    let mut projected = second.clone();
    projected.bus.end_frame_service();
    assert_eq!(
        first.encode_state().unwrap(),
        projected.encode_state().unwrap()
    );
    assert_eq!(first.bus.apu.save_state(), projected.bus.apu.save_state());
}

#[test]
fn frame_direct_matches_every_instruction_in_rom_and_writable_ram() {
    for instruction_set in [InstructionSet::Arm, InstructionSet::Thumb] {
        for base in [
            0x0200_0200,
            0x0300_0200,
            0x0800_0200,
            0x0A00_0200,
            0x0C00_0200,
        ] {
            let mut phase = fixture(instruction_set, base);
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            let mut direct_classes = [0u32; 4];
            for _ in 0..520 {
                let candidate = direct.cpu.pipeline.front().copied().and_then(|prefetched| {
                    let fetched = prefetched.decode(direct.cpu.instruction_set(), 0);
                    super::super::frame_direct::stateless_candidate(
                        fetched,
                        direct.cpu.fetched_condition_passed(fetched),
                    )
                });
                let expected = phase.cpu.step(&mut phase.bus);
                let actual = if let Some((fetched, count)) =
                    direct
                        .cpu
                        .run_frame_direct(&mut direct.bus, phase.cpu.cycles, true, false)
                {
                    assert_eq!(count, 1);
                    direct_classes[candidate.expect("direct candidate")] += count;
                    Some(fetched)
                } else {
                    direct.cpu.step(&mut direct.bus)
                };
                assert_eq!(actual, expected);
                assert_projected_equal(&phase, &direct);
            }
            assert!(
                direct_classes.iter().sum::<u32>() > 400,
                "{instruction_set:?} {base:08X}: {direct_classes:?}"
            );
            if instruction_set == InstructionSet::Arm {
                assert!(direct_classes[..3].iter().all(|count| *count != 0));
            } else {
                assert!(direct_classes[3] > 400);
            }
        }
    }
}

#[test]
fn frame_direct_merges_only_the_prefix_before_horizon_and_guard() {
    for guard_offset in [1, 2, 3, 7, 31, 511] {
        let mut phase = fixture(InstructionSet::Arm, 0x0300_0200);
        for index in 0..128 {
            phase.bus.write32(0x0300_0200 + index * 4, 0xE280_0001);
        }
        phase.cpu.step(&mut phase.bus);
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        let budget = direct.bus.frame_cpu_cycle_budget();
        let guard = direct.cpu.cycles + u64::from(guard_offset);
        let (_, count) = direct
            .cpu
            .run_frame_direct(&mut direct.bus, guard, true, false)
            .unwrap();
        assert_eq!(count, guard_offset.min(budget));
        for _ in 0..count {
            phase.cpu.step(&mut phase.bus);
        }
        assert_projected_equal(&phase, &direct);
        if count == budget {
            assert!(
                direct
                    .cpu
                    .run_frame_direct(&mut direct.bus, u64::MAX, true, false)
                    .is_none()
            );
            assert_eq!(
                phase.cpu.step(&mut phase.bus),
                direct.cpu.step(&mut direct.bus)
            );
            assert_projected_equal(&phase, &direct);
        }
    }
}

#[test]
fn frame_direct_fences_unsafe_lookahead_observers_and_swi_return() {
    let mut phase = fixture(InstructionSet::Arm, 0x0300_0200);
    phase.cpu.step(&mut phase.bus);
    for fallback in 0..6 {
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        match fallback {
            0 => direct.cpu.break_after_next_stub = true,
            1 => direct.bus.debug_trace_enabled = true,
            2 => direct.cpu.swi_wait_return_pc = Some(direct.cpu.pc()),
            3 => {
                direct.cpu.set_pc(0x0800_3FF4);
                direct.cpu.fetch_decode_stub(&direct.bus);
            }
            4 => {
                direct.cpu.set_pc(0x03FF_FFF4);
                direct.cpu.fetch_decode_stub(&direct.bus);
            }
            _ => {
                direct.cpu.step_cpu_phase_for_test(&mut direct.bus);
            }
        }
        let before = direct.encode_state().unwrap();
        assert!(
            direct
                .cpu
                .run_frame_direct(&mut direct.bus, u64::MAX, true, false)
                .is_none()
        );
        assert_eq!(direct.encode_state().unwrap(), before);
    }
}

#[test]
fn frame_direct_completes_a_latched_halt_and_respects_ready_irq() {
    let mut phase = fixture(InstructionSet::Arm, 0x0300_0200);
    for index in 0..32 {
        phase.bus.write32(0x0300_0200 + index * 4, 0xE280_0001);
    }
    phase.cpu.step(&mut phase.bus);
    phase.bus.write8(0x0400_0301, 0);
    let mut direct = phase.clone();
    direct.bus.begin_frame_service();
    let expected = phase.cpu.step(&mut phase.bus).unwrap();
    assert!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, false)
            .is_none()
    );
    let actual = direct.cpu.step(&mut direct.bus).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(direct.cpu.state, CpuState::Halted);
    assert_projected_equal(&phase, &direct);

    direct.cpu.resume();
    direct.bus.write16(0x0400_0200, 1 << 3);
    direct.bus.write16(0x0400_0208, 1);
    direct.bus.request_interrupt(1 << 3);
    direct.bus.step_cycles(7);
    direct.cpu.cycles += 7;
    direct.bus.materialize_frame_service();
    assert!(direct.bus.interrupt_ready());
    direct.cpu.cpsr &= !CPSR_IRQ_DISABLE;
    assert!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, false)
            .is_none()
    );
}

use super::*;

fn frame_fixture() -> Emulator {
    let mut emu = Emulator::new(&minimal_rom(), 48_000).unwrap();
    for (index, instruction) in [0xE590_1000, 0xE281_1001, 0xE580_1000, 0xEAFF_FFFB]
        .into_iter()
        .enumerate()
    {
        emu.bus.write32(0x0300_0000 + index as u32 * 4, instruction);
    }
    emu.cpu.set_pc(0x0300_0000);
    emu.cpu.regs[0] = 0x0200_0000;
    for (address, value) in [
        (0x0400_0084, 0x80),
        (0x0400_0080, 0xFF77),
        (0x0400_0068, 0xF080),
        (0x0400_006C, 0x87C3),
        (0x0400_0082, 0x330F),
        (0x0400_0100, 0xFF00),
        (0x0400_0102, 0x80),
    ] {
        emu.bus.write16(address, value);
    }
    emu
}

fn assert_frame_equal(first: &mut Emulator, second: &mut Emulator) {
    assert_eq!(
        first.encode_state().unwrap(),
        second.encode_state().unwrap()
    );
    assert_eq!(first.bus.ppu.framebuffer(), second.bus.ppu.framebuffer());
    assert_eq!(first.bus.apu.save_state(), second.bus.apu.save_state());
    let mut first_audio = Vec::new();
    let mut second_audio = Vec::new();
    first.drain_audio_samples_into(&mut first_audio);
    second.drain_audio_samples_into(&mut second_audio);
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
    assert!(!second.bus.frame_service_stats_for_test().0);
    assert_eq!(second.bus.frame_service_stats_for_test().1, 0);
}

#[test]
fn frame_service_preserves_frames_native_state_and_restored_continuation() {
    let mut eager = frame_fixture();
    let mut deferred = eager.clone();
    for index in 0..8 {
        if index == 3 {
            let saved = eager.encode_state().unwrap();
            eager.load_state(&saved).unwrap();
            deferred.load_state(&saved).unwrap();
        }
        eager.eager_service_step_frame();
        deferred.selected_frame_service_step_frame(true);
        assert_frame_equal(&mut eager, &mut deferred);
    }
    assert!(deferred.bus.frame_service_stats_for_test().2 > 0);
    #[cfg(feature = "profiling")]
    {
        let snapshot = deferred.profiling_snapshot();
        assert!(snapshot.frame_cpu_direct_instructions > 100_000);
        assert!(
            snapshot
                .frame_cpu_direct_kinds
                .iter()
                .all(|count| *count > 0)
        );
    }
}

#[test]
fn frame_service_keeps_single_step_and_observed_frames_eager() {
    let mut stepped = frame_fixture();
    for _ in 0..64 {
        stepped.step_instruction();
    }
    assert_eq!(stepped.bus.frame_service_stats_for_test(), (false, 0, 0));
    for observer in 0..6 {
        let mut eager = frame_fixture();
        match observer {
            0 => eager.set_opcode_log_enabled(true),
            1 => eager.set_instruction_trace_enabled(true),
            2 => eager.set_apu_debug_capture_enabled(true),
            3 => eager.debug.break_on_next = true,
            4 => eager.debug_step(),
            _ => eager.debug.set_event_breakpoint(DebugEvent::Dma, true),
        }
        let mut deferred = eager.clone();
        eager.eager_service_step_frame();
        deferred.step_frame();
        assert_frame_equal(&mut eager, &mut deferred);
        assert_eq!(deferred.bus.frame_service_stats_for_test(), (false, 0, 0));
    }
}

#[test]
fn frame_service_resumes_every_cpu_phase_with_exact_state_and_tas_projection() {
    let mut source = frame_fixture();
    let mut visited = 0u16;
    for _ in 0..64 {
        let bit = 1 << source.cpu.execution_state().phase.tag();
        if visited & bit == 0 {
            visited |= bit;
            let state = source.encode_state().unwrap();
            let mut eager = source.clone();
            let mut deferred = source.clone();
            eager.load_state(&state).unwrap();
            deferred.load_state(&state).unwrap();
            eager.eager_service_step_frame();
            deferred.step_frame();
            let first_state = eager.encode_state().unwrap();
            let second_state = deferred.encode_state().unwrap();
            assert_eq!(
                crate::save_state::inspect_current_native_gba_tas_state(&eager, &first_state)
                    .unwrap()
                    .projection,
                crate::save_state::inspect_current_native_gba_tas_state(&deferred, &second_state)
                    .unwrap()
                    .projection,
            );
            assert_frame_equal(&mut eager, &mut deferred);
        }
        source.cpu.step_cpu_phase_for_test(&mut source.bus);
    }
    assert_eq!(visited, 0xFF);
}

#[test]
fn frame_service_keeps_thumb_self_modifying_ram_halt_and_hle_order() {
    for workload in 0..5 {
        let mut eager = frame_fixture();
        match workload {
            0 => {
                for (index, opcode) in [0x6801, 0x3101, 0x6001, 0xE7FB].into_iter().enumerate() {
                    eager.bus.write16(0x0300_0000 + index as u32 * 2, opcode);
                }
                eager.cpu.cpsr |= 1 << 5;
            }
            1 => {
                for (address, opcode) in [
                    (0x0300_0000, 0xE580_1000),
                    (0x0300_0004, 0xE221_1001),
                    (0x0300_0008, 0xEA00_000C),
                    (0x0300_0040, 0xE3A0_2000),
                    (0x0300_0044, 0xEAFF_FFED),
                ] {
                    eager.bus.write32(address, opcode);
                }
                eager.cpu.regs[0] = 0x0300_0040;
                eager.cpu.regs[1] = 0xE3A0_2001;
            }
            2 => {
                eager.cpu.state = crate::hardware::cpu::CpuState::Halted;
                eager.bus.write16(0x0400_0200, 1 << 3);
                eager.bus.write16(0x0400_0208, 1);
                eager.bus.write16(0x0400_0102, 0xC0);
            }
            _ => {
                eager.bus.write32(
                    0x0300_0000,
                    if workload == 3 {
                        0xEF00_0019
                    } else {
                        0xEF00_0001
                    },
                );
                eager.bus.write32(0x0300_0004, 0xEAFF_FFFD);
                eager.cpu.regs[0] = if workload == 3 { 1 } else { 0x40 };
            }
        }
        let mut deferred = eager.clone();
        for _ in 0..3 {
            eager.eager_service_step_frame();
            deferred.step_frame();
            assert_frame_equal(&mut eager, &mut deferred);
        }
    }
}

#[test]
fn frame_cpu_run_returns_after_midphase_completion_with_an_eligible_irq() {
    use crate::hardware::cpu::CpuExecutionPhase;

    let mut eager = frame_fixture();
    while eager.cpu.execution_state().phase != CpuExecutionPhase::Writeback {
        eager.cpu.step_cpu_phase_for_test(&mut eager.bus);
    }
    eager.bus.write16(0x0400_0200, 1 << 3);
    eager.bus.write16(0x0400_0208, 1);
    eager.bus.request_interrupt(1 << 3);
    eager.bus.step_cycles(7);
    eager.cpu.cycles += 7;
    assert!(eager.bus.interrupt_ready());
    let mut deferred = eager.clone();
    deferred.bus.begin_frame_service();
    assert_eq!(
        eager.cpu.step(&mut eager.bus),
        deferred
            .cpu
            .run_frame_service(&mut deferred.bus, u64::MAX, true, true),
    );
    deferred.bus.end_frame_service();
    assert_frame_equal(&mut eager, &mut deferred);
}

#[test]
fn frame_cpu_run_preserves_immediate_dma_charging_before_the_next_instruction() {
    let mut eager = frame_fixture();
    for (address, value) in [
        (0x0300_0000, 0xE1C0_10B0),
        (0x0300_0004, 0xE283_3001),
        (0x0300_0008, 0xEAFF_FFFD),
        (0x0200_0100, 0xE283_3003),
        (0x0400_00B0, 0x0200_0100),
        (0x0400_00B4, 0x0300_0004),
    ] {
        eager.bus.write32(address, value);
    }
    eager.bus.write16(0x0400_00B8, 1);
    eager.cpu.regs[0] = 0x0400_00BA;
    eager.cpu.regs[1] = 0x8400;
    let mut deferred = eager.clone();
    eager.eager_service_step_frame();
    deferred.step_frame();
    assert_eq!(deferred.bus.read32(0x0300_0004), 0xE283_3003);
    assert_frame_equal(&mut eager, &mut deferred);
}

#[test]
fn frame_direct_preserves_phase_runner_frames_and_restored_continuation() {
    let mut phase = frame_fixture();
    for index in 0..24 {
        phase.bus.write32(0x0300_0000 + index * 4, 0xE281_1001);
    }
    phase.bus.write32(0x0300_0060, 0xEAFF_FFE6);
    let mut direct = phase.clone();
    for frame in 0..4 {
        phase.selected_frame_service_step_frame(false);
        direct.selected_frame_service_step_frame(true);
        assert_frame_equal(&mut phase, &mut direct);
        if frame == 1 {
            let state = phase.encode_state().unwrap();
            phase.load_state(&state).unwrap();
            direct.load_state(&state).unwrap();
        }
    }
    #[cfg(feature = "profiling")]
    assert!(direct.profiling_snapshot().frame_cpu_direct_instructions > 100_000);
}

#[test]
fn frame_thumb_preserves_frame_audio_tas_and_restored_continuation() {
    let mut phase = frame_fixture();
    phase.cpu.cpsr |= 1 << 5;
    for (index, raw) in [0x8001, 0x8802, 0x3101, 0x2900, 0xD001, 0xE7F9]
        .into_iter()
        .enumerate()
    {
        phase.bus.write16(0x0300_0000 + index as u32 * 2, raw);
    }
    let mut direct = phase.clone();
    for frame in 0..3 {
        phase.selected_frame_service_step_frame(false);
        direct.selected_frame_service_step_frame(true);
        let first = phase.encode_state().unwrap();
        let second = direct.encode_state().unwrap();
        assert_eq!(
            crate::save_state::inspect_current_native_gba_tas_state(&phase, &first)
                .unwrap()
                .projection,
            crate::save_state::inspect_current_native_gba_tas_state(&direct, &second)
                .unwrap()
                .projection,
        );
        assert_frame_equal(&mut phase, &mut direct);
        if frame == 1 {
            phase.load_state(&first).unwrap();
            direct.load_state(&first).unwrap();
        }
    }
    #[cfg(feature = "profiling")]
    assert!(
        direct
            .profiling_snapshot()
            .frame_cpu_direct_kinds
            .iter()
            .all(|count| *count > 1_000)
    );
}

#[test]
fn frame_multiply_preserves_frame_audio_tas_and_restored_continuation() {
    let mut phase = frame_fixture();
    for (index, raw) in [0xE014_0291, 0xE281_1001, 0xE025_3291, 0x1AFF_FFFB]
        .into_iter()
        .enumerate()
    {
        phase.bus.write32(0x0300_0000 + index as u32 * 4, raw);
    }
    phase.cpu.regs[1] = 3;
    phase.cpu.regs[2] = 5;
    phase.cpu.regs[3] = 7;
    let mut direct = phase.clone();
    for frame in 0..3 {
        phase.selected_frame_multiply_step_frame(false);
        direct.selected_frame_multiply_step_frame(true);
        let first = phase.encode_state().unwrap();
        let second = direct.encode_state().unwrap();
        assert_eq!(
            crate::save_state::inspect_current_native_gba_tas_state(&phase, &first)
                .unwrap()
                .projection,
            crate::save_state::inspect_current_native_gba_tas_state(&direct, &second)
                .unwrap()
                .projection,
        );
        assert_frame_equal(&mut phase, &mut direct);
        if frame == 1 {
            phase.load_state(&first).unwrap();
            direct.load_state(&first).unwrap();
        }
    }
    #[cfg(feature = "profiling")]
    {
        let snapshot = direct.profiling_snapshot();
        assert!(snapshot.frame_cpu_direct_instructions > 100_000);
        assert!(
            snapshot.instruction_classes_arm
                [crate::hardware::cpu::ArmInstructionClass::Multiply as usize]
                > 10_000
        );
    }
}

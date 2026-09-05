use super::*;
use crate::hardware::bus::{Bus, DebugTraceEvent};

fn minimal_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    rom
}

fn emerald_rom() -> Vec<u8> {
    let mut rom = minimal_rom();
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom
}

fn seed_host_audio_output(emu: &mut Emulator) {
    emu.bus.apu.seed_host_output_for_state_load_test();
    assert_ne!(emu.apu_debug_snapshot().sample_buffer_len, 0);
    for channel in 0..2 {
        assert!(
            emu.apu_direct_debug_samples_ordered(channel)
                .iter()
                .any(|&sample| sample != 0.0)
        );
    }
    for channel in 0..4 {
        assert!(
            emu.apu_psg_channel_debug_samples_ordered(channel)
                .iter()
                .any(|&sample| sample != 0.0)
        );
    }
    assert!(
        emu.apu_master_debug_samples_ordered()
            .iter()
            .any(|&sample| sample != 0.0)
    );
    assert!(
        emu.apu_psg_master_debug_samples_ordered()
            .iter()
            .any(|&sample| sample != 0.0)
    );
    let (sample_count, sample_remainder, capture_remainder) =
        emu.bus.apu.psg_host_output_state_for_test();
    assert_ne!(sample_count, 0);
    assert_ne!(sample_remainder, 0.0);
    assert_ne!(capture_remainder, 0);
}

fn assert_host_audio_output_cleared(emu: &Emulator) {
    assert_eq!(emu.apu_debug_snapshot().sample_buffer_len, 0);
    for channel in 0..2 {
        assert!(
            emu.apu_direct_debug_samples_ordered(channel)
                .iter()
                .all(|&sample| sample == 0.0)
        );
    }
    for channel in 0..4 {
        assert!(
            emu.apu_psg_channel_debug_samples_ordered(channel)
                .iter()
                .all(|&sample| sample == 0.0)
        );
    }
    assert!(
        emu.apu_master_debug_samples_ordered()
            .iter()
            .all(|&sample| sample == 0.0)
    );
    assert!(
        emu.apu_psg_master_debug_samples_ordered()
            .iter()
            .all(|&sample| sample == 0.0)
    );
    assert_eq!(emu.bus.apu.psg_host_output_state_for_test(), (0, 0.0, 0));
}

fn assert_host_audio_output_eq(actual: &Emulator, expected: &Emulator) {
    assert_eq!(actual.apu_debug_snapshot(), expected.apu_debug_snapshot());
    for channel in 0..2 {
        assert_eq!(
            actual.apu_direct_debug_samples_ordered(channel),
            expected.apu_direct_debug_samples_ordered(channel)
        );
    }
    for channel in 0..4 {
        assert_eq!(
            actual.apu_psg_channel_debug_samples_ordered(channel),
            expected.apu_psg_channel_debug_samples_ordered(channel)
        );
    }
    assert_eq!(
        actual.apu_master_debug_samples_ordered(),
        expected.apu_master_debug_samples_ordered()
    );
    assert_eq!(
        actual.apu_psg_master_debug_samples_ordered(),
        expected.apu_psg_master_debug_samples_ordered()
    );
    assert_eq!(
        actual.bus.apu.psg_host_output_state_for_test(),
        expected.bus.apu.psg_host_output_state_for_test()
    );
}

fn assert_timers_eq(actual: &Bus, expected: &Bus) {
    for (actual, expected) in actual
        .timer_registers_snapshot()
        .iter()
        .zip(expected.timer_registers_snapshot().iter())
    {
        assert_eq!(actual.reload, expected.reload);
        assert_eq!(actual.counter, expected.counter);
        assert_eq!(actual.control, expected.control);
    }
}

#[test]
fn roundtrips_state() {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    emu.cpu_write8(0x0200_0000, 0x55);
    emu.step_frame();
    let bytes = encode_state(&emu).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();
    assert_eq!(restored.cpu_peek8(0x0200_0000), 0x55);
    assert_eq!(restored.frame_count(), 1);
}

#[test]
fn save_state_allows_post_boot_bios_mode_changes() {
    let rom = minimal_rom();
    let bios = vec![0; crate::hardware::constants::BIOS_SIZE];
    let hle_state = encode_state(&Emulator::new(&rom, 48_000).unwrap()).unwrap();
    let mut external = Emulator::new_with_bios(&rom, &bios, 48_000).unwrap();
    external.cpu.set_pc(0x0800_0000);
    let external_state = encode_state(&external).unwrap();

    let mut hle = Emulator::new(&rom, 48_000).unwrap();
    let mut external = Emulator::new_with_bios(&rom, &bios, 48_000).unwrap();
    decode_state(&mut hle, &external_state).unwrap();
    decode_state(&mut external, &hle_state).unwrap();
    decode_state(&mut external, &external_state).unwrap();
}

#[test]
fn save_state_requires_matching_bios_while_executing_in_it() {
    let rom = minimal_rom();
    let bios = vec![0; crate::hardware::constants::BIOS_SIZE];
    let external_state =
        encode_state(&Emulator::new_with_bios(&rom, &bios, 48_000).unwrap()).unwrap();

    let mut hle = Emulator::new(&rom, 48_000).unwrap();
    assert!(decode_state(&mut hle, &external_state).is_err());
}

#[test]
fn roundtrips_fiq_banked_r8_state() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.cpu.regs[8] = 0x1111_2222;
    saved.cpu.set_cpsr(0xC0 | 0x11);
    saved.cpu.regs[8] = 0x3333_4444;
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();

    assert_eq!(restored.cpu.regs[8], 0x3333_4444);
    restored.cpu.set_cpsr(0xC0 | 0x1F);
    assert_eq!(restored.cpu.regs[8], 0x1111_2222);
    restored.cpu.set_cpsr(0xC0 | 0x11);
    assert_eq!(restored.cpu.regs[8], 0x3333_4444);
}

#[test]
fn rejects_unsupported_state_version() {
    let rom = minimal_rom();
    let emu = Emulator::new(&rom, 48_000).unwrap();
    let mut bytes = encode_state(&emu).unwrap();
    bytes[8..12].copy_from_slice(&(TILT_VERSION + 1).to_le_bytes());

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    let err = decode_state(&mut restored, &bytes).unwrap_err();

    assert!(err.to_string().contains(&format!(
        "unsupported GBA save-state version {}",
        TILT_VERSION + 1
    )));
}

#[test]
fn decode_preserves_runtime_audio_config() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 96_000).unwrap();
    saved.set_apu_sample_generation_enabled(true);
    saved.set_apu_channel_mutes([true, false, true, false, true, false]);
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    restored.set_apu_sample_generation_enabled(false);
    restored.set_apu_channel_mutes([false, true, false, true, false, true]);

    decode_state(&mut restored, &bytes).unwrap();

    let apu = restored.apu_debug_snapshot();
    assert_eq!(apu.sample_rate, 48_000);
    assert!(!apu.sample_generation_enabled);
    assert_eq!(apu.channel_mutes, [false, true, false, true, false, true]);
}

#[test]
fn decode_restores_serialized_keypad_state() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.set_input(0x31, 0x05);
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    restored.set_input(0x02, 0x0A);
    decode_state(&mut restored, &bytes).unwrap();

    assert_eq!(
        restored.bus.keypad.read_keyinput(),
        saved.bus.keypad.read_keyinput()
    );
    assert_eq!(encode_state(&restored).unwrap(), bytes);
}

#[test]
fn public_load_clears_host_audio_output_and_preserves_runtime_policy() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 96_000).unwrap();
    saved.bus.apu.write_fifo_halfword(0, 0x2211);
    saved.bus.apu.write_fifo_halfword(0, 0x4433);
    saved.bus.apu.write_fifo_halfword(1, 0x6655);
    saved.bus.apu.write_fifo_halfword(1, 0x0877);
    saved
        .bus
        .apu
        .on_timer_overflows([1, 0, 0, 0], (1 << 8) | (1 << 12));
    let saved_apu = saved.apu_debug_snapshot();
    assert_eq!(saved_apu.fifo_len, [3, 3]);
    assert_eq!(saved_apu.current_sample, [0x11, 0x55]);
    let state = saved.encode_state().unwrap();

    let mut restored = Emulator::new(&rom, 44_100).unwrap();
    let mutes = [true, false, true, false, true, false];
    restored.set_apu_sample_generation_enabled(false);
    restored.set_apu_channel_mutes(mutes);
    restored.set_apu_debug_capture_enabled(true);
    seed_host_audio_output(&mut restored);

    restored.load_state(&state).unwrap();

    let restored_apu = restored.apu_debug_snapshot();
    assert_eq!(restored_apu.sample_rate, 44_100);
    assert_eq!(restored_apu.psg_sample_rate, 44_100);
    assert!(!restored_apu.sample_generation_enabled);
    assert!(restored_apu.debug_capture_enabled);
    assert_eq!(restored_apu.channel_mutes, mutes);
    assert_eq!(restored_apu.fifo_len, saved_apu.fifo_len);
    assert_eq!(restored_apu.current_sample, saved_apu.current_sample);
    assert_host_audio_output_cleared(&restored);
    let mut audio = Vec::new();
    restored.drain_audio_samples_into(&mut audio);
    assert!(audio.is_empty());
}

#[test]
fn roundtrips_rtc_gpio_state_and_v4_defaults() {
    let rom = emerald_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.bus.write16(0x0800_00C8, 1);
    saved.bus.write32(0x0800_00C4, 0x0007_0005);
    let state = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &state).unwrap();
    assert_eq!(restored.bus.read16(0x0800_00C8), 1);
    assert_eq!(restored.bus.read16(0x0800_00C6), 7);

    let mut v4 = state[..state.len()
        - 34
        - VERSION_7_RUNTIME_STATE_SIZE
        - VERSION_8_EXECUTION_STATE_SIZE
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_10_BACKUP_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE]
        .to_vec();
    v4[8..12].copy_from_slice(&4u32.to_le_bytes());
    let mut legacy = Emulator::new(&rom, 48_000).unwrap();
    legacy.set_rtc_date_time(
        crate::hardware::cartridge::RtcDateTime::new(2031, 7, 8, 2, [12, 34, 56]).unwrap(),
    );
    let default_control = legacy.bus.read16(0x0800_00C8);
    decode_state(&mut legacy, &v4).unwrap();
    assert_eq!(legacy.bus.read16(0x0800_00C8), default_control);
    assert_eq!(legacy.rtc_date_time().unwrap().year(), 2000);
}

#[test]
fn roundtrips_timer_scheduler_phase_and_irq_timing() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.bus.step_cycles(37);
    saved.bus.write16(0x0400_0200, 1 << 3);
    saved.bus.write16(0x0400_0208, 1);
    saved.bus.write16(0x0400_0100, 0xFFFC);
    saved.bus.write16(0x0400_0102, 0x00C1);
    saved.bus.step_cycles(19);
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    restored.bus.step_cycles(1);
    assert!(!restored.bus.event_deadline_is_invalid_for_test());
    decode_state(&mut restored, &bytes).unwrap();
    assert!(restored.bus.event_deadline_is_invalid_for_test());
    assert_timers_eq(&restored.bus, &saved.bus);
    assert_eq!(
        restored.bus.timer_timing_state(),
        saved.bus.timer_timing_state()
    );
    assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
    for cycles in [1, 7, 63, 64, 211] {
        saved.bus.step_cycles(cycles);
        restored.bus.step_cycles(cycles);
        assert_timers_eq(&restored.bus, &saved.bus);
        assert_eq!(
            restored.bus.timer_timing_state(),
            saved.bus.timer_timing_state()
        );
        assert_eq!(
            restored.bus.read16(0x0400_0202),
            saved.bus.read16(0x0400_0202)
        );
        assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
    }
}

#[test]
fn roundtrips_timer_global_divider_phase() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.bus.step_cycles(37);
    saved.bus.write16(0x0400_0100, 0xFFFF);
    saved.bus.write16(0x0400_0102, 0x0081);
    assert_eq!(saved.bus.timers.cycles_until_overflow(0), Some(27));
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();
    assert_eq!(
        restored.bus.timer_timing_state(),
        saved.bus.timer_timing_state()
    );

    for cycles in [26, 1] {
        saved.bus.step_cycles(cycles);
        restored.bus.step_cycles(cycles);
        assert_timers_eq(&restored.bus, &saved.bus);
        assert_eq!(
            restored.bus.timer_timing_state(),
            saved.bus.timer_timing_state()
        );
    }
}

#[test]
fn roundtrips_pending_timer_start_delay() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.bus.step_cycles(16);
    saved.bus.write16(0x0400_0100, 0xFFFF);
    saved.bus.write16(0x0400_0102, 0x0080);
    assert_eq!(saved.bus.timer_timing_state().start_delay_cycles[0], 1);
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();
    assert_eq!(
        restored.bus.timer_timing_state(),
        saved.bus.timer_timing_state()
    );

    for cycles in [1, 1] {
        saved.bus.step_cycles(cycles);
        restored.bus.step_cycles(cycles);
        assert_timers_eq(&restored.bus, &saved.bus);
        assert_eq!(
            restored.bus.timer_timing_state(),
            saved.bus.timer_timing_state()
        );
    }
}

#[test]
fn lazy_and_eager_timer_service_encode_identical_state_bytes() {
    let rom = minimal_rom();
    let mut lazy = Emulator::new(&rom, 48_000).unwrap();
    lazy.bus.write16(0x0400_0100, 0xF123);
    lazy.bus.write16(0x0400_0102, 0x0081);

    let mut eager = lazy.clone();
    eager.bus.set_eager_timer_materialization_for_test(true);
    for cycles in [3, 11, 29, 7] {
        lazy.bus.step_cycles(cycles);
        eager.bus.step_cycles(cycles);
    }
    assert!(lazy.bus.timer_materialization_is_pending_for_test());

    let lazy_bytes = encode_state(&lazy).unwrap();
    let eager_bytes = encode_state(&eager).unwrap();
    assert_eq!(lazy_bytes, eager_bytes);
    assert!(lazy.bus.timer_materialization_is_pending_for_test());

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &lazy_bytes).unwrap();
    assert_eq!(encode_state(&restored).unwrap(), lazy_bytes);

    for cycles in [13, 64, 127] {
        lazy.bus.step_cycles(cycles);
        eager.bus.step_cycles(cycles);
        restored.bus.step_cycles(cycles);
        assert_eq!(encode_state(&lazy).unwrap(), encode_state(&eager).unwrap());
        assert_eq!(
            encode_state(&restored).unwrap(),
            encode_state(&lazy).unwrap()
        );
    }
}

#[test]
fn roundtrips_interrupt_wait_mask() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.cpu.swi_wait_return_pc = Some(0x0800_1234);
    saved.cpu.swi_wait_mask = 1 << 3;
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();

    assert_eq!(restored.cpu.swi_wait_return_pc, Some(0x0800_1234));
    assert_eq!(restored.cpu.swi_wait_mask, 1 << 3);
}

#[test]
fn roundtrips_prefetched_pipeline_contents() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    let code = 0x0300_0000;
    saved.bus.write32(code, 0xE3A0_0001);
    saved.bus.write32(code + 4, 0xE3A0_1002);
    saved.bus.write32(code + 8, 0xE3A0_2003);
    saved.cpu.set_pc(code);
    let _ = saved.step_instruction();
    saved.bus.write32(code + 4, 0xE3A0_1009);
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();

    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    assert_eq!(restored.cpu.execution_state(), CpuExecutionState::default());
    let _ = saved.step_instruction();
    let _ = restored.step_instruction();
    assert_eq!(saved.cpu.regs, restored.cpu.regs);
    assert_eq!(restored.cpu.regs[1], 2);
    assert_eq!(saved.cpu.cycles, restored.cpu.cycles);
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
}

#[test]
fn roundtrips_pending_load_internal_cycle() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    let code = 0x0300_0000;
    saved.bus.write16(code, 0x880A);
    saved.bus.write16(code + 2, 0x2307);
    saved.bus.write16(0x0300_0100, 0xABCD);
    saved.cpu.cpsr |= crate::hardware::cpu::CPSR_THUMB;
    saved.cpu.set_pc(code);
    saved.cpu.regs[1] = 0x0300_0100;
    let _ = saved.step_instruction();
    assert!(saved.cpu.pipeline_state().pending_load_internal_cycle);
    let bytes = encode_state(&saved).unwrap();

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    let _ = saved.step_instruction();
    let _ = restored.step_instruction();
    assert_eq!(restored.cpu.regs, saved.cpu.regs);
    assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
}

#[test]
fn migrates_version_7_at_an_instruction_boundary() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    let code = 0x0300_0000;
    saved.bus.write32(code, 0xE3A0_0001);
    saved.bus.write32(code + 4, 0xE280_1002);
    saved.bus.write32(code + 8, 0xE281_2003);
    saved.cpu.set_pc(code);
    let _ = saved.step_instruction();
    let state = encode_state(&saved).unwrap();
    let mut v7 = state[..state.len()
        - VERSION_8_EXECUTION_STATE_SIZE
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_10_BACKUP_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE]
        .to_vec();
    v7[8..12].copy_from_slice(&7u32.to_le_bytes());

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &v7).unwrap();

    assert_eq!(restored.cpu.execution_state(), CpuExecutionState::default());
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    let _ = saved.step_instruction();
    let _ = restored.step_instruction();
    assert_eq!(restored.cpu.regs, saved.cpu.regs);
    assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
}

fn branch_emulator_after_phase_steps(steps: usize) -> Emulator {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    let code = 0x0300_0000;
    emu.bus.write32(code, 0xEA00_0002);
    emu.bus.write32(code + 4, 0xE1A0_0000);
    emu.bus.write32(code + 8, 0xE1A0_0000);
    emu.bus.write32(code + 0x10, 0xE3A0_0007);
    emu.bus.write32(code + 0x14, 0xE1A0_0000);
    emu.bus.write32(code + 0x18, 0xE1A0_0000);
    emu.cpu.set_pc(code);
    for _ in 0..steps {
        let _ = emu.cpu.step_cpu_phase_for_test(&mut emu.bus);
    }
    emu
}

fn assert_midphase_continuation(steps: usize, expected_phase: CpuExecutionPhase) {
    let rom = minimal_rom();
    let mut saved = branch_emulator_after_phase_steps(steps);
    assert_eq!(saved.cpu.execution_state().phase, expected_phase);
    let state = encode_state(&saved).unwrap();
    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &state).unwrap();

    assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    assert_eq!(restored.cpu.regs, saved.cpu.regs);
    assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
    assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    assert_eq!(
        restored.bus.timer_timing_state(),
        saved.bus.timer_timing_state()
    );
    assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());

    let saved_result = saved.step_instruction();
    let restored_result = restored.step_instruction();
    assert_eq!(restored_result, saved_result);
    assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    assert_eq!(restored.cpu.regs, saved.cpu.regs);
    assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
    assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    assert_eq!(
        restored.bus.timer_timing_state(),
        saved.bus.timer_timing_state()
    );
    assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
}

#[test]
fn roundtrips_sequential_fetch_and_both_refill_boundaries() {
    assert_midphase_continuation(1, CpuExecutionPhase::SequentialFetch);
    assert_midphase_continuation(2, CpuExecutionPhase::Execute);
    assert_midphase_continuation(3, CpuExecutionPhase::RefillNonSequential);
    assert_midphase_continuation(4, CpuExecutionPhase::RefillSequential);
    assert_midphase_continuation(5, CpuExecutionPhase::Boundary);
}

#[test]
fn roundtrips_both_irq_refill_boundaries() {
    let rom = minimal_rom();
    for completed_phases in [0, 1] {
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.cpu.cpsr &= !(1 << 7);
        assert!(saved.cpu.try_service_irq(&mut saved.bus, true));
        for _ in 0..completed_phases {
            let _ = saved.cpu.step_cpu_phase_for_test(&mut saved.bus);
        }
        let expected_phase = if completed_phases == 0 {
            CpuExecutionPhase::RefillNonSequential
        } else {
            CpuExecutionPhase::RefillSequential
        };
        assert_eq!(saved.cpu.execution_state().phase, expected_phase);
        let state = encode_state(&saved).unwrap();
        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &state).unwrap();

        assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        let saved_result = saved.step_instruction();
        let restored_result = restored.step_instruction();
        assert_eq!(restored_result, saved_result);
        assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    }
}

fn staged_transfer_emulator(instruction: u32, registers: &[(usize, u32)]) -> Emulator {
    let rom = minimal_rom();
    let mut emu = Emulator::new(&rom, 48_000).unwrap();
    let code = 0x0300_0000;
    emu.bus.write32(code, instruction);
    emu.bus.write32(code + 4, 0xE1A0_0000);
    emu.bus.write32(code + 8, 0xE1A0_0000);
    emu.cpu.set_pc(code);
    for &(register, value) in registers {
        emu.cpu.regs[register] = value;
    }
    emu
}

fn assert_staged_transfer_continuation(
    instruction: u32,
    registers: &[(usize, u32)],
    phase_steps: usize,
    expected_phase: CpuExecutionPhase,
) {
    let rom = minimal_rom();
    let mut saved = staged_transfer_emulator(instruction, registers);
    saved.bus.write32(0x0200_0000, 0x1122_3344);
    saved.bus.write32(0x0200_0004, 0x5566_7788);
    for _ in 0..phase_steps {
        let _ = saved.cpu.step_cpu_phase_for_test(&mut saved.bus);
    }
    assert_eq!(saved.cpu.execution_state().phase, expected_phase);
    let state = encode_state(&saved).unwrap();
    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &state).unwrap();

    assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    assert_eq!(restored.cpu.regs, saved.cpu.regs);
    assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    assert_eq!(restored.bus.ewram, saved.bus.ewram);
    let saved_result = saved.step_instruction();
    let restored_result = restored.step_instruction();
    assert_eq!(restored_result, saved_result);
    assert_eq!(restored.cpu.regs, saved.cpu.regs);
    assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
    assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    assert_eq!(restored.bus.ewram, saved.bus.ewram);
    assert_eq!(
        restored.bus.timer_timing_state(),
        saved.bus.timer_timing_state()
    );
    assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
}

#[test]
fn roundtrips_every_single_transfer_phase() {
    for (steps, phase) in [
        (3, CpuExecutionPhase::DataBus),
        (4, CpuExecutionPhase::LoadInternal),
        (5, CpuExecutionPhase::Writeback),
    ] {
        assert_staged_transfer_continuation(0xE590_1000, &[(0, 0x0200_0000)], steps, phase);
    }
}

#[test]
fn roundtrips_every_two_register_block_transfer_boundary() {
    for (steps, phase) in [
        (3, CpuExecutionPhase::DataBus),
        (4, CpuExecutionPhase::LoadInternal),
        (5, CpuExecutionPhase::DataBus),
        (6, CpuExecutionPhase::LoadInternal),
        (7, CpuExecutionPhase::Writeback),
    ] {
        assert_staged_transfer_continuation(0xE8B2_0005, &[(2, 0x0200_0000)], steps, phase);
    }
}

#[test]
fn completed_block_store_is_not_replayed_after_restore() {
    let rom = minimal_rom();
    let mut saved = staged_transfer_emulator(0xE8A2_0005, &[(0, 0x1111_2222), (2, 0x0200_0000)]);
    for _ in 0..4 {
        let _ = saved.cpu.step_cpu_phase_for_test(&mut saved.bus);
    }
    assert_eq!(
        saved.cpu.execution_state().phase,
        CpuExecutionPhase::DataBus
    );
    assert_eq!(saved.cpu.execution_state().bus_address, 0x0200_0004);
    let bytes = encode_state(&saved).unwrap();
    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &bytes).unwrap();

    let (_, events) = restored.step_instruction_with_bus_trace(false, true);
    assert!(events.iter().any(|event| matches!(
        event,
        DebugTraceEvent::Write {
            addr: 0x0200_0004,
            ..
        }
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        DebugTraceEvent::Write {
            addr: 0x0200_0000,
            ..
        }
    )));
    assert_eq!(restored.bus.read32(0x0200_0000), 0x1111_2222);
    assert_eq!(restored.bus.read32(0x0200_0004), 0x0200_0008);
}

#[test]
fn rejects_invalid_or_noncanonical_execution_state() {
    let rom = minimal_rom();
    let saved = Emulator::new(&rom, 48_000).unwrap();
    let bytes = encode_state(&saved).unwrap();
    let execution_offset = bytes.len()
        - VERSION_8_EXECUTION_STATE_SIZE
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_10_BACKUP_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE;

    let mut invalid_phase = bytes.clone();
    invalid_phase[execution_offset] = 8;
    assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_phase).is_err());

    let mut active_phase = bytes.clone();
    active_phase[execution_offset] = CpuExecutionPhase::Execute.tag();
    assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &active_phase).is_err());

    let mut orphaned_phase_cycle = bytes.clone();
    orphaned_phase_cycle[execution_offset + 1] = 1;
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &orphaned_phase_cycle
        )
        .is_err()
    );

    let mut invalid_bus_operation = bytes.clone();
    invalid_bus_operation[execution_offset + 17] = 3;
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &invalid_bus_operation
        )
        .is_err()
    );

    let mut orphaned_bus_width = bytes;
    orphaned_bus_width[execution_offset + 22] = 4;
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &orphaned_bus_width
        )
        .is_err()
    );

    let mut invalid_cursor = encode_state(&saved).unwrap();
    invalid_cursor[execution_offset + 59..execution_offset + 63]
        .copy_from_slice(&1025u32.to_le_bytes());
    assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_cursor).is_err());
}

#[test]
fn rejects_invalid_timer_and_irq_scheduler_state() {
    let rom = minimal_rom();
    let saved = Emulator::new(&rom, 48_000).unwrap();
    let bytes = encode_state(&saved).unwrap();
    let runtime_offset = bytes.len()
        - VERSION_8_EXECUTION_STATE_SIZE
        - VERSION_7_RUNTIME_STATE_SIZE
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_10_BACKUP_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE;

    let mut invalid_accum = bytes.clone();
    invalid_accum[runtime_offset..runtime_offset + 4].copy_from_slice(&0x400u32.to_le_bytes());
    assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_accum).is_err());

    let mut invalid_start_delay = bytes.clone();
    invalid_start_delay[runtime_offset + 16] = 2;
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &invalid_start_delay
        )
        .is_err()
    );

    let mut invalid_phase = bytes.clone();
    let phase_offset = runtime_offset + 20;
    invalid_phase[phase_offset..phase_offset + 2].copy_from_slice(&0x400u16.to_le_bytes());
    assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_phase).is_err());

    let mut invalid_irq_delay = bytes.clone();
    let irq_present_offset = runtime_offset + 22;
    invalid_irq_delay[irq_present_offset] = 1;
    let irq_delay_offset = runtime_offset + 23;
    invalid_irq_delay[irq_delay_offset..irq_delay_offset + 4].copy_from_slice(&8u32.to_le_bytes());
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &invalid_irq_delay
        )
        .is_err()
    );

    let mut invalid_wait_mask = encode_state(&saved).unwrap();
    let wait_mask_offset = runtime_offset + 27;
    invalid_wait_mask[wait_mask_offset..wait_mask_offset + 2]
        .copy_from_slice(&0x4000u16.to_le_bytes());
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &invalid_wait_mask
        )
        .is_err()
    );

    let mut orphaned_wait_mask = encode_state(&saved).unwrap();
    orphaned_wait_mask[wait_mask_offset..wait_mask_offset + 2].copy_from_slice(&8u16.to_le_bytes());
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &orphaned_wait_mask
        )
        .is_err()
    );

    let mut invalid_pipeline = bytes.clone();
    invalid_pipeline[runtime_offset + 29] = 1;
    invalid_pipeline[runtime_offset + 30..runtime_offset + 34]
        .copy_from_slice(&0x0800_0004u32.to_le_bytes());
    assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_pipeline).is_err());

    let mut invalid_pending_load = bytes.clone();
    invalid_pending_load[runtime_offset + 48] = 2;
    assert!(
        decode_state(
            &mut Emulator::new(&rom, 48_000).unwrap(),
            &invalid_pending_load
        )
        .is_err()
    );
}

#[test]
fn migrates_version_6_timer_and_irq_scheduler_state() {
    let rom = minimal_rom();
    let mut saved = Emulator::new(&rom, 48_000).unwrap();
    saved.bus.write16(0x0400_0200, 1 << 3);
    saved.bus.write16(0x0400_0208, 1);
    saved.bus.write16(0x0400_0100, 0xFFFF);
    saved.bus.write16(0x0400_0102, 0x00C1);
    saved.bus.step_cycles(64);
    saved.cpu.cycles = 321;
    let state = encode_state(&saved).unwrap();
    let mut v6 = state[..state.len()
        - VERSION_8_EXECUTION_STATE_SIZE
        - VERSION_7_RUNTIME_STATE_SIZE
        - VERSION_9_ROM_HASH_SIZE
        - VERSION_10_BACKUP_EXECUTION_STATE_SIZE
        - VERSION_12_PSG_STATE_SIZE]
        .to_vec();
    v6[8..12].copy_from_slice(&6u32.to_le_bytes());

    let mut restored = Emulator::new(&rom, 48_000).unwrap();
    decode_state(&mut restored, &v6).unwrap();

    let timing = restored.bus.timer_timing_state();
    assert_eq!(timing.clock_phase, 321);
    assert_eq!(timing.cycle_accum[0], 1);
    assert_eq!(timing.start_delay_cycles, [0; 4]);
    assert_eq!(restored.bus.irq_delay_state(), Some(7));
}

#[test]
fn public_load_rejects_trailing_data_without_mutation() {
    let mut emu = Emulator::new(&minimal_rom(), 48_000).unwrap();
    emu.set_input(0x03, 0x05);
    emu.step_frame();
    emu.set_apu_sample_generation_enabled(false);
    emu.set_apu_channel_mutes([true, false, true, false, true, false]);
    emu.set_apu_debug_capture_enabled(true);
    seed_host_audio_output(&mut emu);
    let before = emu.encode_state().unwrap();
    let framebuffer = emu.framebuffer().to_vec();
    let mut expected = emu.clone();
    let mut invalid = before.clone();
    invalid.push(0xA5);

    assert!(emu.load_state(&invalid).is_err());
    assert_eq!(emu.encode_state().unwrap(), before);
    assert_eq!(emu.framebuffer(), framebuffer);
    assert_host_audio_output_eq(&emu, &expected);
    let mut actual_audio = Vec::new();
    let mut expected_audio = Vec::new();
    emu.drain_audio_samples_into(&mut actual_audio);
    expected.drain_audio_samples_into(&mut expected_audio);
    assert_eq!(actual_audio, expected_audio);
}

#[test]
fn direct_decode_failure_invalidates_derived_bus_deadline() {
    let mut emu = Emulator::new(&minimal_rom(), 48_000).unwrap();
    emu.bus.step_cycles(1);
    assert!(!emu.bus.event_deadline_is_invalid_for_test());

    assert!(decode_state(&mut emu, b"invalid").is_err());

    assert!(emu.bus.event_deadline_is_invalid_for_test());
}

use super::*;

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
fn direct_decode_failure_invalidates_derived_bus_deadline() {
    let mut emu = Emulator::new(&minimal_rom(), 48_000).unwrap();
    emu.bus.step_cycles(1);
    assert!(!emu.bus.event_deadline_is_invalid_for_test());

    assert!(decode_state(&mut emu, b"invalid").is_err());

    assert!(emu.bus.event_deadline_is_invalid_for_test());
}

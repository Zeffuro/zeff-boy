use super::*;

#[test]
fn reset_and_state_restore_retain_an_explicit_incomplete_trace() {
    for restore in [0, 1, 2] {
        let mut emu = traced(&[0x3e, 0x77, 0xe0, 0x24, 0x76], false);
        let mut initial = emu.encode_state_bytes().unwrap();
        run_to_halt(&mut emu);
        if restore != 0 {
            if restore == 2 {
                initial[0] ^= 0xff;
            }
            emu.load_state(&initial).unwrap();
        } else {
            emu.reset();
        }
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(
            trace.invalidated,
            Some(if restore != 0 {
                AudioTraceInvalidation::StateRestore
            } else {
                AudioTraceInvalidation::Reset
            })
        );
        assert!(!trace.events.is_empty());
        assert!(trace.validate_complete().is_err());
    }
}

#[test]
fn rejected_state_restore_preserves_a_complete_capture() {
    let mut emu = traced(&[0x76], false);
    assert!(emu.load_state(b"invalid").is_err());
    emu.finish_audio_trace()
        .unwrap()
        .validate_complete()
        .unwrap();
}

#[test]
fn mutations_and_unrecorded_link_inputs_invalidate_capture() {
    let mutations: &[fn(&mut Emulator)] = &[
        |emu| emu.write_byte(0xc000, 1),
        |emu| emu.cpu_write8(0xff24, 0x77),
        |emu| emu.set_apu_enabled(false),
        |emu| emu.clear_rom_patches(),
        |emu| {
            emu.load_battery_sram(&[]).unwrap();
        },
        |emu| emu.set_game_boy_link_peer_present(true),
        |emu| {
            emu.schedule_game_boy_external_link_transfer(0x5a, 512);
        },
        |emu| emu.set_mbc7_host_tilt(0.5, 0.5),
        |emu| emu.set_camera_host_frame(&[]),
        |emu| emu.debug_suspend(),
    ];
    for mutate in mutations {
        let mut emu = traced(&[0x76], false);
        mutate(&mut emu);
        let trace = emu.finish_audio_trace().unwrap();
        assert_eq!(
            trace.invalidated,
            Some(AudioTraceInvalidation::ExternalMutation)
        );
        assert!(trace.validate_complete().is_err());
    }
    let mut left = traced(&[0x76], false);
    let mut right = traced(&[0x76], false);
    left.sync_game_boy_link_peer(&mut right);
    for emu in [&mut left, &mut right] {
        assert_eq!(
            emu.finish_audio_trace().unwrap().invalidated,
            Some(AudioTraceInvalidation::ExternalMutation)
        );
    }
}

#[test]
fn capture_limits_and_unsupported_start_leave_existing_state_unchanged() {
    let mut emu = traced(&[0x76], false);
    emu.step_instruction();
    let state = emu.encode_state_bytes().unwrap();
    assert!(
        emu.reset_and_begin_audio_trace(MAX_AUDIO_TRACE_EVENTS + 1)
            .is_err()
    );
    assert_eq!(state, emu.encode_state_bytes().unwrap());
    emu.finish_audio_trace()
        .unwrap()
        .validate_complete()
        .unwrap();
    emu.set_apu_enabled(false);
    assert!(emu.reset_and_begin_audio_trace(100).is_err());
    let mut source = rom(&[0x76], false);
    source[0x146] = 3;
    source[0x14b] = 0x33;
    let mut emu = Emulator::from_rom_data(&source, HardwareModePreference::ForceSgb).unwrap();
    assert!(emu.reset_and_begin_audio_trace(100).is_err());
}

#[test]
fn capacity_exhaustion_retains_prefix_and_counts_dropped_events() {
    let mut emu = emulator(
        &[0x3e, 0x77, 0xe0, 0x24, 0xe0, 0x24, 0xe0, 0x24, 0x76],
        false,
    );
    emu.reset_and_begin_audio_trace(1).unwrap();
    run_to_halt(&mut emu);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.dropped_events, 2);
    assert!(trace.validate_complete().is_err());
    assert!(emu.finish_audio_trace().is_none());
}

#[test]
fn clock_discontinuity_is_visible_without_serializing_capture_state() {
    let mut emu = traced(&[0x76], false);
    let initial = emu.encode_state_bytes().unwrap();
    assert_eq!(
        initial,
        emulator(&[0x76], false).encode_state_bytes().unwrap()
    );
    emu.cycle_count = 1;
    emu.step_instruction();
    assert_eq!(
        emu.finish_audio_trace().unwrap().invalidated,
        Some(AudioTraceInvalidation::NonMonotonicCycles)
    );
}

#[test]
fn clock_overflow_invalidates_before_a_timestamp_can_wrap() {
    let mut emu = traced(&[0x76], false);
    emu.cycle_count = u64::MAX - 2;
    emu.bus.audio_trace_cycle = emu.cycle_count;
    emu.step_instruction();
    assert_eq!(
        emu.finish_audio_trace().unwrap().invalidated,
        Some(AudioTraceInvalidation::ClockOverflow),
    );
}

#[test]
fn unsupported_execution_preserves_native_behavior_but_invalidates_capture() {
    let code = [0x3e, 0x77, 0xe0, 0x24, 0xd3, 0x76];
    let mut plain = emulator(&code, false);
    let mut captured = traced(&code, false);
    run_to_halt(&mut plain);
    run_to_halt(&mut captured);
    assert_eq!(
        plain.encode_state_bytes().unwrap(),
        captured.encode_state_bytes().unwrap()
    );
    assert_eq!(plain.drain_audio_samples(), captured.drain_audio_samples());
    let trace = captured.finish_audio_trace().unwrap();
    assert_eq!(registers(&trace).len(), 1);
    assert_eq!(
        trace.invalidated,
        Some(AudioTraceInvalidation::ExecutionFault)
    );
}

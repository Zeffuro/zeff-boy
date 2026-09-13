use super::*;

fn assert_invalid(mut emu: Emulator, reason: AudioTraceInvalidation) {
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.invalidated, Some(reason));
    assert!(trace.validate_complete().is_err());
}

#[test]
fn reset_and_finish_bound_the_generation_and_rejected_start_is_transactional() {
    let mut emu = traced(&[0xb0, 0x5a, 0xe6, 0x88, 0xf4], false);
    run_to_halt(&mut emu);
    let before = emu.encode_state().unwrap();
    let original_trace = emu.clone().finish_audio_trace().unwrap();
    for limit in [0, MAX_AUDIO_TRACE_EVENTS + 1] {
        assert!(emu.reset_and_begin_audio_trace(limit).is_err());
        assert_eq!(emu.encode_state().unwrap(), before);
        assert_eq!(emu.clone().finish_audio_trace().unwrap(), original_trace);
    }
    let mut reset = emu.clone();
    reset.reset();
    assert_invalid(reset, AudioTraceInvalidation::Reset);
    let first = emu.finish_audio_trace().unwrap();
    first.validate_complete().unwrap();
    assert_eq!(emu.finish_audio_trace(), None);
    assert!(!emu.bus.audio_trace.is_enabled());
    emu.reset_and_begin_audio_trace(10).unwrap();
    let restarted = emu.finish_audio_trace().unwrap();
    assert_eq!(restarted.generation, first.generation + 1);
    assert!(restarted.events.is_empty());
    assert_eq!(restarted.end_cycle, 0);
    restarted.validate_complete().unwrap();
}

#[test]
fn overflow_keeps_a_bounded_prefix_and_refuses_completeness() {
    let mut code = Vec::new();
    for (port, value) in [(0x88, 0x11), (0x89, 0x22), (0x8a, 0x33)] {
        out(&mut code, port, value);
    }
    code.push(0xf4);
    let mut emu = emulator(&code, true);
    emu.reset_and_begin_audio_trace(1).unwrap();
    run_to_halt(&mut emu);
    assert_eq!(emu.io_peek8(0x8a), 0x33);
    let trace = emu.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.dropped_events, 2);
    assert_eq!(trace.invalidated, None);
    assert!(trace.validate_complete().is_err());
}

#[test]
fn public_state_decoder_invalidates_partial_mutation_but_wrapper_rolls_back() {
    let mut emu = traced(&[0x90, 0xf4], true);
    emu.step_instruction();
    let state = emu.encode_state().unwrap();
    let trace = emu.clone().finish_audio_trace().unwrap();
    let frame_offset = crate::save_state::SAVE_STATE_MAGIC.len() + 1 + 32;
    let mut truncated = state[..frame_offset + 8].to_vec();
    truncated[frame_offset..].copy_from_slice(&12_u64.to_le_bytes());

    assert!(emu.load_state(&truncated).is_err());
    assert_eq!(emu.encode_state().unwrap(), state);
    assert_eq!(emu.clone().finish_audio_trace().unwrap(), trace);
    let mut direct = emu.clone();
    assert!(crate::save_state::decode_state(&mut direct, &truncated).is_err());
    assert_eq!(direct.frame_count(), 12);
    assert_invalid(direct, AudioTraceInvalidation::StateRestore);

    let mut bad_magic = state.clone();
    bad_magic[0] ^= 0xff;
    assert!(crate::save_state::decode_state(&mut emu, &bad_magic).is_err());
    assert_eq!(emu.encode_state().unwrap(), state);
    assert_eq!(emu.clone().finish_audio_trace().unwrap(), trace);
    let mut direct = emu.clone();
    crate::save_state::decode_state(&mut direct, &state).unwrap();
    assert_invalid(direct, AudioTraceInvalidation::StateRestore);
    emu.load_state(&state).unwrap();
    assert_invalid(emu, AudioTraceInvalidation::StateRestore);
}

#[test]
fn persistence_mutation_invalidates_only_after_success() {
    let mut bytes = rom(&[0xf4], true);
    bytes[0xfffb] = 2;
    bytes[0xfffd] = 1;
    checksum(&mut bytes);
    let mut emu = Emulator::new(&bytes, 48_000).unwrap();
    let battery = vec![0x5a; emu.save_ram_kind().size()];
    let complete = emu.dump_complete_rtc_persistence().unwrap();
    emu.reset_and_begin_audio_trace(10).unwrap();
    let before = emu.encode_state().unwrap();
    let trace = emu.clone().finish_audio_trace().unwrap();
    assert!(emu.load_battery_sram(&[0]).is_err());
    assert!(emu.load_complete_rtc_persistence(&[0]).is_err());
    assert_eq!(emu.encode_state().unwrap(), before);
    assert_eq!(emu.clone().finish_audio_trace().unwrap(), trace);
    let mut battery_target = emu.clone();
    battery_target.load_battery_sram(&battery).unwrap();
    assert_eq!(battery_target.dump_battery_sram(), Some(battery));
    assert_invalid(battery_target, AudioTraceInvalidation::ExternalMutation);
    emu.load_complete_rtc_persistence(&complete).unwrap();
    assert_invalid(emu, AudioTraceInvalidation::ExternalMutation);
}

#[test]
fn debugger_writes_and_guest_call_injection_refuse_export() {
    let mut code = vec![0xf4; 0x14];
    code[0x10..].copy_from_slice(&[0xb0, 0x2a, 0xc3, 0xf4]);
    let emu = traced(&code, false);
    let mut memory = emu.clone();
    memory.cpu_write8(0x100, 0x5a);
    assert_invalid(memory, AudioTraceInvalidation::ExternalMutation);
    let mut io = emu.clone();
    io.io_write8(0x88, 0xa5);
    assert_invalid(io, AudioTraceInvalidation::ExternalMutation);

    let mut call = emu;
    call.step_instruction();
    call.debug_suspend();
    assert!(call.debug_execute_guest_call(0xf0010, 0).is_err());
    assert!(call.debug_execute_guest_call(0x10000, 10).is_err());
    call.clone()
        .finish_audio_trace()
        .unwrap()
        .validate_complete()
        .unwrap();
    assert_eq!(call.debug_execute_guest_call(0xf0010, 10), Ok(2));
    assert_invalid(call, AudioTraceInvalidation::ExternalMutation);
}

#[test]
fn external_link_input_invalidates_both_sync_participants() {
    let mut left = traced(&[0xf4], false);
    let mut right = left.clone();
    left.sync_wonder_swan_link_peer(&mut right);
    assert_invalid(left, AudioTraceInvalidation::ExternalMutation);
    assert_invalid(right, AudioTraceInvalidation::ExternalMutation);
    let mut received = traced(&[0xf4], true);
    received.receive_wonder_swan_link_byte(0x55);
    assert_invalid(received, AudioTraceInvalidation::ExternalMutation);
}

#[test]
fn traps_and_both_clock_overflows_refuse_export() {
    let mut faulted = traced(&[0xf1], false);
    faulted.step_instruction();
    faulted.step_instruction();
    assert!(faulted.last_trap().is_some());
    assert_invalid(faulted, AudioTraceInvalidation::ExecutionFault);
    let mut bus_overflow = traced(&[0xf4], true);
    bus_overflow.bus.cycles = u64::MAX;
    bus_overflow.bus.step_cycles(1);
    assert_invalid(bus_overflow, AudioTraceInvalidation::ClockOverflow);
    let mut cpu_overflow = traced(&[0xf4], true);
    cpu_overflow.cpu.cycles = u64::MAX;
    cpu_overflow.step_instruction();
    assert_invalid(cpu_overflow, AudioTraceInvalidation::ClockOverflow);
}

use super::*;
use crate::hardware::{CdDisc, CdTrack, CdTrackMode};
use zeff_emu_common::audio_trace::MAX_AUDIO_TRACE_EVENTS;

const STORE: &[u8] = &[0xA9, 255, 0x8D, 1, 8];

fn assert_invalid(machine: &mut PceMachine, reason: AudioTraceInvalidation) {
    let trace = machine.finish_audio_trace().unwrap();
    assert_eq!(trace.invalidated, Some(reason));
    assert!(trace.validate_complete().is_err());
}

#[test]
fn reset_invalidates_and_begin_uses_a_new_generation() {
    let mut machine = machine(STORE);
    machine.reset_and_begin_audio_trace(2).unwrap();
    run(&mut machine, 2);
    let original = finish(&mut machine);
    assert_eq!(original.generation, 1);
    machine.reset_and_begin_audio_trace(2).unwrap();
    run(&mut machine, 2);
    machine.reset();
    let reset = machine.finish_audio_trace().unwrap();
    assert_eq!(reset.generation, 2);
    assert_eq!(reset.invalidated, Some(AudioTraceInvalidation::Reset));
    assert!(reset.validate_complete().is_err());
    machine.reset_and_begin_audio_trace(2).unwrap();
    assert_eq!(finish(&mut machine).generation, 3);
}

#[test]
fn invalid_capacity_preserves_running_state_and_the_active_trace() {
    let mut machine = machine(STORE);
    machine.reset_and_begin_audio_trace(2).unwrap();
    run(&mut machine, 2);
    let before = save_state::encode_state(&machine).unwrap();
    for capacity in [0, MAX_AUDIO_TRACE_EVENTS + 1] {
        assert!(machine.reset_and_begin_audio_trace(capacity).is_err());
        assert_eq!(before, save_state::encode_state(&machine).unwrap());
    }
    let trace = finish(&mut machine);
    assert_eq!(trace.generation, 1);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.end_cycle, 84);
}

#[test]
fn event_capacity_never_evicts_writes_or_allows_a_complete_trace() {
    let mut machine = machine(&[0x8D, 1, 8, 0x8D, 2, 8, 0x8D, 3, 8]);
    machine.reset_and_begin_audio_trace(1).unwrap();
    run(&mut machine, 3);
    let trace = machine.finish_audio_trace().unwrap();
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].write.register, 1);
    assert_eq!(trace.dropped_events, 2);
    assert_eq!(trace.end_cycle, 180);
    assert!(trace.validate_complete().is_err());
}

#[test]
fn successful_restore_preserves_an_invalid_trace_and_its_generation() {
    let mut machine = machine(STORE);
    let initial = save_state::encode_state(&machine).unwrap();
    machine.reset_and_begin_audio_trace(2).unwrap();
    run(&mut machine, 2);
    save_state::decode_state(&mut machine, &initial).unwrap();
    assert_eq!(save_state::encode_state(&machine).unwrap(), initial);
    assert_invalid(&mut machine, AudioTraceInvalidation::StateRestore);
    machine.reset_and_begin_audio_trace(2).unwrap();
    assert_eq!(finish(&mut machine).generation, 2);
}

#[test]
fn rejected_restore_preserves_both_running_state_and_recording() {
    let mut machine = machine(STORE);
    machine.reset_and_begin_audio_trace(2).unwrap();
    run(&mut machine, 2);
    let state = save_state::encode_state(&machine).unwrap();
    let mut invalid = state.clone();
    invalid.pop();
    assert!(save_state::decode_state(&mut machine, &invalid).is_err());
    assert_eq!(save_state::encode_state(&machine).unwrap(), state);
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.generation, 1);
}

#[test]
fn external_state_access_cannot_continue_a_complete_capture() {
    let mutations: [fn(&mut PceMachine); 7] = [
        |machine| {
            let _ = machine.cpu_mut();
        },
        |machine| {
            let _ = machine.devices_mut();
        },
        |machine| {
            let _ = machine.mapped_work_ram_mut();
        },
        |machine| machine.debug_write_cpu8(0x0801, 255),
        |machine| machine.cheat_write8(0x2000, 1),
        |machine| machine.cheat_write_physical_ram(0x1F_0000, 1),
        |machine| {
            machine.debug_suspend();
            let _ = machine.debug_execute_guest_call(0xE100, 1);
        },
    ];
    for mutate in mutations {
        let mut machine = machine(STORE);
        machine.reset_and_begin_audio_trace(4).unwrap();
        mutate(&mut machine);
        assert_invalid(&mut machine, AudioTraceInvalidation::ExternalMutation);
    }
}

#[test]
fn observation_and_audio_output_controls_keep_the_capture_valid() {
    let mut machine = machine(STORE);
    machine.reset_and_begin_audio_trace(2).unwrap();
    machine.set_sample_rate(48_000);
    machine.set_channel_mutes(&[true, false]);
    machine.set_sample_generation_enabled(false);
    machine.set_sample_generation_enabled(true);
    machine.set_instruction_trace_enabled(true);
    machine.set_instruction_trace_capacity(4);
    machine.set_opcode_history_enabled(true);
    machine.debug_suspend();
    assert!(machine.debug_execute_guest_call(0xE100, 0).is_err());
    machine.debug_continue();
    run(&mut machine, 2);
    let _ = machine.debug_peek_cpu8(0x0801);
    let _ = machine.debug_peek_physical8(0x1F_E801);
    let _ = pcm(&mut machine);
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
}

#[test]
fn execution_and_clock_faults_reject_complete_capture() {
    let mut trapped = machine(STORE);
    trapped.reset_and_begin_audio_trace(2).unwrap();
    assert!(trapped.force_unsupported_opcode_trap_after_fetch().is_err());
    assert_invalid(&mut trapped, AudioTraceInvalidation::ExecutionFault);

    let mut overflowed = machine(&[0x8D, 1, 8]);
    overflowed.reset_and_begin_audio_trace(2).unwrap();
    overflowed.master_ticks = u64::MAX - 20;
    assert!(matches!(
        overflowed.step_boundary(),
        Err(PceMachineError::ClockOverflow { .. })
    ));
    let trace = overflowed.finish_audio_trace().unwrap();
    assert_eq!(
        trace.invalidated,
        Some(AudioTraceInvalidation::ClockOverflow)
    );
    assert!(trace.events.is_empty());
    assert!(trace.validate_complete().is_err());
}

#[test]
fn cd_capture_is_rejected_without_resetting_the_machine() {
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048]).unwrap(),
    ])
    .unwrap();
    let mut image = vec![0xEA; 0x40000];
    image[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    let mut machine = PceMachine::with_cdrom2(image, disc).unwrap();
    run(&mut machine, 2);
    let state = save_state::encode_state(&machine).unwrap();
    assert!(machine.reset_and_begin_audio_trace(10).is_err());
    assert_eq!(state, save_state::encode_state(&machine).unwrap());
    assert!(machine.finish_audio_trace().is_none());
}

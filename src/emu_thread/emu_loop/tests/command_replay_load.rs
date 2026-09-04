use super::super::game_boy_replay_start_capture_blocker;
use super::support::*;
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{AudioRecordingCapture, EmuCommand, EmuResponse, EmuThread};
use std::path::PathBuf;

#[test]
fn direct_step_commands_are_inert_after_runtime_fault() {
    let (mut emu_loop, _responses) = test_loop();
    let frame_before = emu_loop.backend.frame_count();
    emu_loop.runtime_fault.latch(Some("fault".to_string()));

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(3)))));
    let first = emu_loop.drain_rx.recv().unwrap();
    assert_eq!(first.runtime_fault.as_deref(), Some("fault"));
    assert_eq!(first.advanced_frames, 0);
    assert_eq!(emu_loop.backend.frame_count(), frame_before);

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(3)))));
    let second = emu_loop.drain_rx.recv().unwrap();
    assert_eq!(second.runtime_fault, None);
    assert_eq!(second.advanced_frames, 0);
    assert_eq!(emu_loop.backend.frame_count(), frame_before);

    assert!(emu_loop.handle_command(EmuCommand::SetUncapped(true)));
    assert!(!emu_loop.uncapped_mode);
}

#[test]
fn uncapped_batch_size_command_clamps_invalid_settings() {
    let (mut emu_loop, _responses) = test_loop();

    assert!(emu_loop.handle_command(EmuCommand::SetUncappedBatchSize(0)));
    assert_eq!(emu_loop.uncapped_batch_size, 1);

    assert!(emu_loop.handle_command(EmuCommand::SetUncappedBatchSize(17)));
    assert_eq!(emu_loop.uncapped_batch_size, 17);

    assert!(emu_loop.handle_command(EmuCommand::SetUncappedBatchSize(usize::MAX)));
    assert_eq!(
        emu_loop.uncapped_batch_size,
        crate::emu_thread::MAX_UNCAPPED_BATCH_SIZE
    );
}

#[test]
fn pal_sega8_loop_uses_pal_pacing_and_rewind_duration() {
    let (emu_loop, _responses) = test_pal_sega8_loop();

    assert_eq!(emu_loop.backend.nominal_frame_duration_ns(), 20_000_000);
    assert_eq!(emu_loop.rewind_buffer.capacity(), 125);
}

#[test]
fn native_scheduler_tracks_a_pal_sega8_state_load() {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let thread = EmuThread::spawn(
        EmuBackend::from_sega8(emu, PathBuf::from("test.sms")),
        false,
    );
    assert_eq!(thread.nominal_frame_duration_ns(), 16_666_667);

    let pal = zeff_sega8_core::emulator::Emulator::new_with_hint_and_video_standard(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal,
    )
    .unwrap();
    thread.send(EmuCommand::LoadStateBytes {
        state_bytes: pal.encode_state().unwrap(),
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    });

    assert!(matches!(
        thread.recv(),
        Some(EmuResponse::LoadStateOk { .. })
    ));
    assert_eq!(thread.nominal_frame_duration_ns(), 20_000_000);
}

fn replay_link_state(
    pending_master_byte: Option<u8>,
    pending_master_response: Option<u8>,
    queued_master_action: Option<zeff_emu_common::replay::ReplayGameBoyLinkAction>,
) -> zeff_emu_common::replay::ReplayGameBoyLinkState {
    zeff_emu_common::replay::ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte,
        pending_master_response,
        pending_master_completion_ready: false,
        queued_master_action,
        pending_passive_completion: None,
        serial_generation: 7,
    }
}

fn replay_coordinator(
    owner: zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorOwner,
    reply: Option<zeff_emu_common::replay::ReplayGameBoyLinkReply>,
) -> zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorState {
    zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorState {
        transfer_id: 17,
        action: zeff_emu_common::replay::ReplayGameBoyLinkAction {
            out_byte: 0x12,
            clock_period_t_cycles: 4096,
            serial_generation: 7,
        },
        owner,
        reply,
    }
}

#[test]
fn replay_start_capture_accepts_replay_owned_master_transfer() {
    let state = replay_link_state(Some(0x12), None, None);
    let coordinator = replay_coordinator(
        zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
        None,
    );

    assert_eq!(
        game_boy_replay_start_capture_blocker(Some(coordinator), Some(state)),
        None
    );
}

#[test]
fn replay_start_capture_rejects_consumed_core_master_without_reply() {
    let state = replay_link_state(Some(0x12), None, None);

    assert!(game_boy_replay_start_capture_blocker(None, Some(state)).is_some());
}

#[test]
fn replay_start_capture_allows_queued_core_master_but_rejects_unowned_reply() {
    let queued = replay_link_state(
        Some(0x12),
        None,
        Some(zeff_emu_common::replay::ReplayGameBoyLinkAction {
            out_byte: 0x12,
            clock_period_t_cycles: 4096,
            serial_generation: 7,
        }),
    );
    let replied = replay_link_state(Some(0x12), Some(0x34), None);

    assert_eq!(
        game_boy_replay_start_capture_blocker(None, Some(queued)),
        None
    );
    assert!(game_boy_replay_start_capture_blocker(None, Some(replied)).is_some());
}

#[test]
fn replay_start_capture_accepts_core_owned_applied_reply() {
    let state = replay_link_state(Some(0x12), Some(0x34), None);
    let reply = zeff_emu_common::replay::ReplayGameBoyLinkReply {
        out_byte: 0x34,
        passive: true,
        serial_generation: 8,
    };
    let coordinator = replay_coordinator(
        zeff_emu_common::replay::ReplayGameBoyLinkCoordinatorOwner::CoreHasReply,
        Some(reply),
    );

    assert_eq!(
        game_boy_replay_start_capture_blocker(Some(coordinator), Some(state)),
        None
    );
}

#[test]
fn replay_start_capture_allows_passive_in_flight_completion() {
    let state = zeff_emu_common::replay::ReplayGameBoyLinkState {
        pending_passive_completion: Some(zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
            peer_byte: 0xAB,
            remaining_t_cycles: 2048,
        }),
        ..replay_link_state(None, None, None)
    };

    assert_eq!(
        game_boy_replay_start_capture_blocker(None, Some(state)),
        None
    );
}

#[test]
fn replay_load_disconnects_tcp_before_restoring_core_owned_master() {
    use zeff_emu_common::replay::{
        ReplayGameBoyLinkCoordinatorOwner, ReplayGameBoyLinkCoordinatorState,
        ReplayGameBoyLinkReply,
    };
    use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};

    let (mut emu_loop, responses, _peer) = test_gb_loop_with_tcp();
    let EmuBackend::Gb(gb) = &mut emu_loop.backend else {
        unreachable!();
    };
    gb.emu.write_byte(SERIAL_SB, 0xAB);
    gb.emu.write_byte(SERIAL_SC, 0x81);
    gb.emu.set_game_boy_link_peer_present(true);
    let action = gb
        .emu
        .game_boy_link_replay_state()
        .queued_master_action
        .unwrap();
    let state = zeff_emu_common::replay::ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: Some(action.out_byte),
        pending_master_response: Some(0x34),
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: action.serial_generation,
    };
    let reply = ReplayGameBoyLinkReply {
        out_byte: 0x34,
        passive: true,
        serial_generation: 9,
    };
    let coordinator = ReplayGameBoyLinkCoordinatorState {
        transfer_id: 0x0100_0000_0000_0001,
        action,
        owner: ReplayGameBoyLinkCoordinatorOwner::CoreHasReply,
        reply: Some(reply),
    };
    let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();
    let start_tick = emu_loop.backend.game_boy_cpu_cycles();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes,
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: Some(Vec::new()),
        game_boy_link_start_state: Some(state),
        game_boy_link_coordinator_start_state: Some(coordinator),
        game_boy_link_start_tick: start_tick,
        wonder_swan_link_start_tick: None,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        crate::emu_thread::EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(emu_loop.backend.game_boy_link_replay_state(), Some(state));
    assert!(emu_loop.tcp_link.is_none());
    assert!(emu_loop.game_boy_replay_link.is_none());
}

#[test]
fn failed_replay_tick_validation_preserves_tcp_and_core_state() {
    let (mut emu_loop, responses, _peer) = test_gb_loop_with_tcp();
    let before = emu_loop.backend.encode_state_bytes().unwrap();
    let tick = emu_loop.backend.game_boy_cpu_cycles().unwrap();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes: before.clone(),
        buttons_pressed: 0x0F,
        dpad_pressed: 0x0F,
        replay_events: Some(Vec::new()),
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: Some(tick.wrapping_add(1)),
        wonder_swan_link_start_tick: None,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        crate::emu_thread::EmuResponse::LoadStateFailed(_)
    ));
    assert!(emu_loop.tcp_link.is_some());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}

#[test]
fn media_ack_uses_apply_boundary_after_already_advanced_frames() {
    use crate::emu_thread::EmuResponse;
    use zeff_emu_common::media::MediaEvent;

    let (mut emu_loop, responses) = test_fds_loop();
    emu_loop.backend.step_frame();
    let apply_frame = emu_loop.backend.frame_count();
    let snapshot = emu_loop.backend.media_slot_snapshot().unwrap();

    assert!(
        emu_loop.handle_command(EmuCommand::ApplyMediaEvent(MediaEvent::SetWriteProtected {
            slot: snapshot.state.slot,
            write_protected: true,
        }))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::MediaEventApplied {
            frame_count,
            snapshot,
            ..
        } if frame_count == apply_frame && snapshot.state.write_protected
    ));
}

#[test]
fn successful_state_load_starts_exactly_one_semantic_epoch() {
    let (mut emu_loop, responses) = test_loop();
    emu_loop.audio_recording_capture = AudioRecordingCapture {
        active: true,
        semantic: true,
    };
    let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes,
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(
        emu_loop.pending_audio_discontinuities,
        vec![crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad]
    );

    let mut result = semantic_result();
    emu_loop.attach_audio_discontinuities(&mut result);
    assert_eq!(
        result.audio_timeline_discontinuities,
        vec![crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad]
    );
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}

#[test]
fn state_load_discontinuity_is_not_started_after_validation_failure() {
    let (mut emu_loop, _responses) = test_loop();
    emu_loop.audio_recording_capture = AudioRecordingCapture {
        active: true,
        semantic: true,
    };
    let state_bytes = emu_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes,
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: Some(1),
        wonder_swan_link_start_tick: None,
    }));
    assert!(emu_loop.pending_audio_discontinuities.is_empty());

    let mut pre_mutation_result = semantic_result();
    pre_mutation_result.audio_semantic_frames.clear();
    emu_loop.attach_audio_discontinuities(&mut pre_mutation_result);
    assert!(
        pre_mutation_result
            .audio_timeline_discontinuities
            .is_empty()
    );
    assert!(emu_loop.pending_audio_discontinuities.is_empty());

    let mut post_mutation_result = semantic_result();
    emu_loop.attach_audio_discontinuities(&mut post_mutation_result);
    assert!(
        post_mutation_result
            .audio_timeline_discontinuities
            .is_empty()
    );
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}

#[test]
fn failed_state_decode_does_not_start_a_semantic_epoch() {
    let (mut emu_loop, _responses) = test_loop();
    emu_loop.audio_recording_capture = AudioRecordingCapture {
        active: true,
        semantic: true,
    };

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes: vec![0xFF],
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    }));
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
}

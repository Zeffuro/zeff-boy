use super::{EmuLoop, game_boy_replay_start_capture_blocker};
use crate::audio_tooling::{
    AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState, NTSC_60_TEMPO_US_PER_BEAT,
};
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    AudioConfig, AudioRecordingCapture, EmuCommand, FrameInput, FrameResult, JoypadInput,
    RenderSettings, ReusableBuffers, SnapshotRequest, ZapperInput,
};
use crate::link::transport::TcpLinkTransport;
use crate::link::{LinkEndpointId, LinkSession, LinkSystemType, RemoteLink};
use std::net::TcpListener;
use std::path::PathBuf;

fn test_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    (
        EmuLoop::new(backend, cmd_rx, frame_tx, drain_rx, resp_tx, shared),
        resp_rx,
    )
}

fn test_fds_loop() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    use zeff_nes_core::hardware::cartridge::mappers::{FDS_BIOS_SIZE, FDS_SIDE_SIZE};

    let mut disk = vec![0; FDS_SIDE_SIZE];
    disk[0] = 1;
    let emu =
        zeff_nes_core::emulator::Emulator::new_fds(&disk, vec![0xFF; FDS_BIOS_SIZE], 44_100.0)
            .unwrap();
    let backend = EmuBackend::from_nes(emu, PathBuf::from("test.fds"));
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    (
        EmuLoop::new(backend, cmd_rx, frame_tx, drain_rx, resp_tx, shared),
        resp_rx,
    )
}

fn test_gb_loop_with_tcp() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
    TcpLinkTransport,
) {
    let emu = zeff_gb_core::emulator::Emulator::from_rom_data(
        &[0; 0x8000],
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::Auto,
    )
    .unwrap();
    let backend = EmuBackend::from_gb(emu, PathBuf::from("test.gb"));
    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let drain_rx = frame_rx.clone();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let shared = crate::emu_thread::types::new_shared_framebuffer();
    let mut emu_loop = EmuLoop::new(backend, cmd_rx, frame_tx, drain_rx, resp_tx, shared);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let host = std::thread::spawn(move || TcpLinkTransport::accept_once(listener).unwrap());
    let peer = TcpLinkTransport::connect(addr).unwrap();
    let transport = host.join().unwrap();
    emu_loop.tcp_link = Some(RemoteLink::GameBoy(
        crate::link::gb::GameBoyRemoteLink::new(LinkSession::new(
            transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        )),
    ));
    (emu_loop, resp_rx, peer)
}

fn semantic_result() -> FrameResult {
    FrameResult {
        advanced_frames: 1,
        delivery_merged: false,
        replay_events: Vec::new(),
        replay_error: None,
        runtime_fault: None,
        rumble: false,
        audio_samples: Vec::new(),
        audio_playback_speed: 1,
        ui_data: crate::ui::UiFrameData::default(),
        is_mbc7: false,
        is_pocket_camera: false,
        game_boy_serial_device: None,
        game_boy_printer_jobs: Vec::new(),
        media_slot_snapshot: None,
        rewind_fill: 0.0,
        audio_semantic_frames: vec![AudioSemanticFrame {
            frame: 1,
            tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
            voices: vec![AudioVoiceState {
                channel: AudioChannelId(0),
                name: "Test",
                class: AudioVoiceClass::Tone,
                active: false,
                pitch_hz: Some(440.0),
                level: Some(0.0),
            }],
        }],
        audio_timeline_discontinuities: Vec::new(),
    }
}

fn frame_input(frames: usize) -> FrameInput {
    FrameInput {
        frames,
        replay_joypad_frames: None,
        host_tilt: (0.0, 0.0),
        host_camera_frame: None,
        joypad: JoypadInput {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            buttons_p3: 0,
            dpad_p3: 0,
            buttons_p4: 0,
            dpad_p4: 0,
            buttons_p5: 0,
            dpad_p5: 0,
        },
        pce_mouse: Default::default(),
        zapper: ZapperInput::default(),
        debug_step: false,
        debug_continue: false,
        debug_suspend_after_frame: false,
        audio: AudioConfig {
            apu_capture_enabled: false,
            skip_audio: true,
            playback_speed: 1,
            recording_capture: AudioRecordingCapture::default(),
        },
        debug_actions: crate::debug::DebugUiActions::none(),
        snapshot: SnapshotRequest {
            want_debug_info: false,
            want_perf_info: false,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
            show_apu_viewer: false,
            show_disassembler: false,
            show_rom_info: false,
            show_memory_viewer: false,
            memory_view_start: 0,
            show_rom_viewer: false,
            show_instruction_trace: false,
            trace_after_sequence: None,
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            memory_search: None,
            rom_search: None,
            render: RenderSettings {
                color_correction: crate::settings::ColorCorrection::None,
                color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                dmg_palette_preset: crate::settings::DmgPalettePreset::default(),
                nes_palette_mode: crate::settings::NesPaletteMode::default(),
                nes_custom_palette: None,
                pce_overscan_mode: crate::settings::PceOverscanMode::default(),
                pce_palette_mode: crate::settings::PcePaletteMode::default(),
                sgb_border_enabled: false,
            },
        },
        buffers: ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
        rewind_enabled: false,
        rewind_seconds: 10,
    }
}

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
        super::super::MAX_UNCAPPED_BATCH_SIZE
    );
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
fn state_load_discontinuity_survives_post_load_validation_failure() {
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
    assert_eq!(
        emu_loop.pending_audio_discontinuities,
        vec![crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad]
    );

    let mut pre_mutation_result = semantic_result();
    pre_mutation_result.audio_semantic_frames.clear();
    emu_loop.attach_audio_discontinuities(&mut pre_mutation_result);
    assert!(
        pre_mutation_result
            .audio_timeline_discontinuities
            .is_empty()
    );
    assert_eq!(emu_loop.pending_audio_discontinuities.len(), 1);

    let mut post_mutation_result = semantic_result();
    emu_loop.attach_audio_discontinuities(&mut post_mutation_result);
    assert_eq!(
        post_mutation_result.audio_timeline_discontinuities,
        vec![crate::audio_recorder::AudioTimelineDiscontinuity::StateLoad]
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

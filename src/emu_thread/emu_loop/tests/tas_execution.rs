use super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionProfile, TasExecutionRejectedReason as Rejected,
    TasExecutionRequest, TasFrameAdvanceRejectedReason as AdvanceRejected, TasFrameAdvanceRequest,
    TasInputFrame,
};
use crate::tas_project::TasDigest;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

mod coleco;
mod fds;
mod game_gear;
mod gba;
mod gbc;
mod nes;
mod pce;
mod sms;
mod ws;

fn acquire(
    emu_loop: &mut super::super::EmuLoop,
    responses: &crossbeam_channel::Receiver<EmuResponse>,
) -> (u64, Vec<u8>) {
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 70,
        profile: TasExecutionProfile::DirectNesCartridge,
    }));
    match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    }
}

fn direct_gb_backend(oversized: bool) -> crate::emu_backend::EmuBackend {
    let root = crate::test_support::test_directory("tas-control-direct-gb").unwrap();
    let path = root.path().join("control.gb");
    let mut rom = crate::test_support::build_gb_test_rom();
    if oversized {
        rom.push(0);
    }
    std::fs::write(&path, rom).unwrap();
    load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &path,
        &path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference: HardwareModePreference::ForceDmg,
            apply_mods: false,
            gb_load_battery_sram: false,
            initial_input: None,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend
}

#[test]
fn direct_nes_commands_reject_a_direct_gb_lease_without_mutation() {
    let backend = direct_gb_backend(false);
    let before = backend.encode_state_bytes().unwrap();
    let before_digest = TasDigest::from_bytes(&before);
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 71,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let lease_id = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 71,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectGbCartridgeDmg);
            lease_id
        }
        _ => panic!("unexpected acquisition response"),
    };

    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        before.clone(),
        vec![TasInputFrame::default()],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            requested_lease_id,
            run_id: 1,
            reason: Rejected::WrongExecutionProfile {
                active_profile: TasExecutionProfile::DirectGbCartridgeDmg,
            },
            ..
        } if requested_lease_id == lease_id
    ));

    assert!(emu_loop.handle_command(advance_request(
        lease_id,
        1,
        1,
        0,
        before_digest,
        TasInputFrame::default(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanceRejected {
            requested_lease_id,
            run_id: 1,
            advance_id: 1,
            reason: AdvanceRejected::WrongExecutionProfile {
                active_profile: TasExecutionProfile::DirectGbCartridgeDmg,
            },
            ..
        } if requested_lease_id == lease_id
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
}

#[test]
fn direct_gb_acquisition_rejects_an_oversized_direct_source_file() {
    let backend = direct_gb_backend(true);
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 72,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 72,
            reason: crate::emu_thread::TasControlAcquireRejectedReason::StateWitnessUnavailable,
        }
    ));
    assert!(!emu_loop.tas_control.is_leased());
}

fn gb_request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    let target_cursor = input_prefix.len() as u64;
    let branch_prefix_sha256 = synthetic_input_prefix_sha256(&input_prefix);
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: crate::emu_thread::TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x91; 32]),
            branch_prefix_sha256,
            target_cursor,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_gb_execution_matches_an_independent_backend_and_commit_preserves_it() {
    let mut expected = direct_gb_backend(false);
    let inputs = [
        TasInputFrame {
            p1_buttons: 1,
            ..TasInputFrame::default()
        },
        TasInputFrame {
            p1_dpad: 2,
            ..TasInputFrame::default()
        },
    ];
    for input in inputs {
        expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: input.p1_buttons,
            dpad: input.p1_dpad,
            ..Default::default()
        });
        expected.step_frame();
    }
    let expected_state = expected.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(direct_gb_backend(false));
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 73,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 73,
            lease_id,
            witness,
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(gb_request(lease_id, 1, start_state, inputs.to_vec())));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            lease_id: actual_lease_id,
            run_id: 1,
            state_sha256,
            ..
        } if actual_lease_id == lease_id && state_sha256 == TasDigest::from_bytes(&expected_state)
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state
    );
    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitted { lease_id: actual_lease_id } if actual_lease_id == lease_id
    ));
}

#[test]
fn direct_gb_silent_positioning_audio_does_not_leak_into_the_next_live_frame() {
    let mut expected = direct_gb_backend(false);
    let start_state = expected.encode_state_bytes().unwrap();
    let prefix = vec![TasInputFrame::default(); 8];
    let live_input = TasInputFrame {
        p1_buttons: 1,
        ..TasInputFrame::default()
    };
    for input in &prefix {
        expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: input.p1_buttons,
            dpad: input.p1_dpad,
            ..Default::default()
        });
        expected.step_frame();
    }
    let mut silent_positioning_audio = Vec::new();
    expected.drain_audio_samples_into(&mut silent_positioning_audio);
    assert!(!silent_positioning_audio.is_empty());
    expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
        buttons: live_input.p1_buttons,
        dpad: live_input.p1_dpad,
        ..Default::default()
    });
    expected.step_frame();
    let mut expected_live_audio = Vec::new();
    expected.drain_audio_samples_into(&mut expected_live_audio);

    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(direct_gb_backend(false));
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 81,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let lease_id = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 81,
            lease_id,
            ..
        } => lease_id,
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(gb_request(lease_id, 1, start_state, prefix)));
    let (frame_count, state_sha256) = completed_proof(responses.recv().unwrap(), lease_id, 1);
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectGbCartridgeDmg,
                lease_id,
                run_id: 1,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 8,
                expected_executed_project_frames: 8,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: live_input,
                snapshot: None,
            },
        )))
    );
    let actual_live_audio = match responses.recv().unwrap() {
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectGbCartridgeDmg,
            audio_samples,
            ..
        } => audio_samples,
        _ => panic!("unexpected frame-advance response"),
    };

    assert_eq!(actual_live_audio, expected_live_audio);
}

#[test]
fn direct_gb_zero_boundary_restores_start_state_and_completes() {
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(direct_gb_backend(false));
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 80,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 80,
            lease_id,
            witness,
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    emu_loop.backend.step_frame();
    assert_ne!(emu_loop.backend.encode_state_bytes().unwrap(), start_state);

    assert!(emu_loop.handle_command(gb_request(lease_id, 1, start_state.clone(), Vec::new(),)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGbCartridgeDmg,
            lease_id: actual_lease_id,
            run_id: 1,
            segment_id: 1,
            segment_frame_count: 0,
            executed_project_frames: 0,
            frame_count: 0,
            state_sha256,
        } if actual_lease_id == lease_id && state_sha256 == TasDigest::from_bytes(&start_state)
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), start_state);
}

#[test]
fn direct_gb_execution_rejects_non_joypad_input_without_mutation() {
    let backend = direct_gb_backend(false);
    let before = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 74,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(gb_request(
        lease_id,
        1,
        start_state,
        vec![TasInputFrame {
            p2_buttons: 1,
            ..TasInputFrame::default()
        }],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: Rejected::InvalidInput,
            ..
        }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
}

#[test]
fn direct_gb_execution_rejects_high_input_bits_without_mutation() {
    let backend = direct_gb_backend(false);
    let before = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 75,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(gb_request(
        lease_id,
        1,
        start_state,
        vec![TasInputFrame {
            p1_buttons: 0x80,
            ..TasInputFrame::default()
        }],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionRejected {
            reason: Rejected::InvalidInput,
            ..
        }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
}

#[test]
fn direct_gb_execution_rollback_restores_the_checkpoint() {
    let backend = direct_gb_backend(false);
    let before = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 76,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(gb_request(
        lease_id,
        1,
        start_state,
        vec![TasInputFrame {
            p1_buttons: 1,
            ..TasInputFrame::default()
        }],
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted { .. }
    ));
    assert_ne!(emu_loop.backend.encode_state_bytes().unwrap(), before);
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: actual_lease_id, .. }
            if actual_lease_id == lease_id
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
}

#[test]
fn direct_gb_rejects_incompatible_current_start_state_without_mutation() {
    let rom = crate::test_support::build_gb_test_rom();
    let mut printer =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::ForceDmg)
            .unwrap();
    printer.set_game_boy_serial_device(zeff_gb_core::hardware::GameBoySerialDevice::Printer);
    let auto = zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
        .unwrap();

    for (request_id, state) in [
        (77, printer.encode_state_bytes().unwrap()),
        (78, auto.encode_state_bytes().unwrap()),
    ] {
        let backend = direct_gb_backend(false);
        let before = backend.encode_state_bytes().unwrap();
        let before_frame = backend.frame_count();
        let before_framebuffer = backend.framebuffer().to_vec();
        let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
        assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
            request_id,
            profile: TasExecutionProfile::DirectGbCartridgeDmg,
        }));
        let lease_id = match responses.recv().unwrap() {
            EmuResponse::TasControlAcquired { lease_id, .. } => lease_id,
            _ => panic!("unexpected acquisition response"),
        };
        assert!(emu_loop.handle_command(gb_request(
            lease_id,
            1,
            state,
            vec![TasInputFrame::default()],
        )));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasExecutionRejected {
                profile: TasExecutionProfile::DirectGbCartridgeDmg,
                reason: Rejected::InvalidStartState,
                ..
            }
        ));
        assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);
        assert_eq!(emu_loop.backend.frame_count(), before_frame);
        assert_eq!(emu_loop.backend.framebuffer(), before_framebuffer);
        assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasControlRolledBack { lease_id: actual_lease_id, .. }
                if actual_lease_id == lease_id
        ));
    }
}

fn stage_direct_gb_segment_two(keep: bool) {
    let mut expected = direct_gb_backend(false);
    let initial = expected.encode_state_bytes().unwrap();
    let mut inputs = vec![TasInputFrame::default(); 600];
    inputs.push(TasInputFrame {
        p1_buttons: 1,
        p1_dpad: 2,
        ..TasInputFrame::default()
    });
    for input in &inputs {
        expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: input.p1_buttons,
            dpad: input.p1_dpad,
            ..Default::default()
        });
        expected.step_frame();
    }
    let expected_state = expected.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(direct_gb_backend(false));
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 79,
        profile: TasExecutionProfile::DirectGbCartridgeDmg,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            lease_id, witness, ..
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    assert!(emu_loop.handle_command(gb_request(lease_id, 1, start_state, inputs[..600].to_vec(),)));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGbCartridgeDmg,
            lease_id: actual_lease_id,
            run_id: 1,
            segment_id: 1,
            segment_frame_count: 600,
            executed_project_frames: 600,
            frame_count,
            state_sha256,
        } if actual_lease_id == lease_id => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    };
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectGbCartridgeDmg,
                lease_id,
                run_id: 1,
                advance_id: 1,
                segment_id: 2,
                expected_segment_frame_count: 600,
                expected_executed_project_frames: 600,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: inputs[600],
                snapshot: None,
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectGbCartridgeDmg,
            lease_id: actual_lease_id,
            run_id: 1,
            advance_id: 1,
            segment_id: 2,
            segment_frame_count: 1,
            executed_project_frames: 601,
            state_sha256: actual_digest,
            ..
        } if actual_lease_id == lease_id && actual_digest == TasDigest::from_bytes(&expected_state)
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected_state
    );
    if keep {
        assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id }));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasControlCommitted { lease_id: actual_lease_id }
                if actual_lease_id == lease_id
        ));
        assert_eq!(
            emu_loop.backend.encode_state_bytes().unwrap(),
            expected_state
        );
    } else {
        assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasControlRolledBack { lease_id: actual_lease_id, .. }
                if actual_lease_id == lease_id
        ));
        assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), initial);
    }
}

#[test]
fn direct_gb_segment_two_commit_matches_the_independent_backend() {
    stage_direct_gb_segment_two(true);
}

#[test]
fn direct_gb_segment_two_rollback_restores_the_checkpoint() {
    stage_direct_gb_segment_two(false);
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    let target_cursor = input_prefix.len() as u64;
    let branch_prefix_sha256 = synthetic_input_prefix_sha256(&input_prefix);
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: crate::emu_thread::TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x92; 32]),
            branch_prefix_sha256,
            target_cursor,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

fn synthetic_input_prefix_sha256(input_prefix: &[TasInputFrame]) -> TasDigest {
    let bytes = input_prefix
        .iter()
        .flat_map(|input| {
            [
                input.p1_buttons,
                input.p1_dpad,
                input.p2_buttons,
                input.p2_dpad,
            ]
        })
        .collect::<Vec<_>>();
    TasDigest::from_bytes(&bytes)
}

fn advance_request(
    lease_id: u64,
    run_id: u64,
    advance_id: u64,
    expected_frame_count: u64,
    expected_state_sha256: TasDigest,
    input: TasInputFrame,
) -> EmuCommand {
    advance_request_in_segment(
        lease_id,
        run_id,
        advance_id,
        (
            1,
            expected_frame_count.min(crate::tas_project::MAX_EDITOR_SEEK_EXECUTION_FRAMES),
            expected_frame_count,
        ),
        (expected_frame_count, expected_state_sha256),
        input,
    )
}

fn advance_request_in_segment(
    lease_id: u64,
    run_id: u64,
    advance_id: u64,
    (segment_id, expected_segment_frame_count, expected_executed_project_frames): (u64, u64, u64),
    (expected_frame_count, expected_state_sha256): (u64, TasDigest),
    input: TasInputFrame,
) -> EmuCommand {
    EmuCommand::AdvanceTasControl(Box::new(TasFrameAdvanceRequest {
        profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
        lease_id,
        run_id,
        advance_id,
        segment_id,
        expected_segment_frame_count,
        expected_executed_project_frames,
        expected_frame_count,
        expected_state_sha256,
        input,
        snapshot: None,
    }))
}

fn completed_proof(response: EmuResponse, lease_id: u64, run_id: u64) -> (u64, TasDigest) {
    match response {
        EmuResponse::TasExecutionCompleted {
            lease_id: actual_lease_id,
            run_id: actual_run_id,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id && actual_run_id == run_id => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    }
}

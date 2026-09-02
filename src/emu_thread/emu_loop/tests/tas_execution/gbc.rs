use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionProfile, TasExecutionRequest,
    TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::TasDigest;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

fn backend(label: &str) -> crate::emu_backend::EmuBackend {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.gbc");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom[0x143] = 0xC0;
    std::fs::write(&path, rom).unwrap();
    load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        &path,
        &path,
        None,
        BackendLoadConfig {
            gb_hardware_mode_preference: HardwareModePreference::ForceCgb,
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend
}

#[test]
fn direct_gbc_worker_executes_advances_and_rolls_back() {
    let mut expected = backend("tas-worker-gbc-expected");
    let live = backend("tas-worker-gbc-live");
    let checkpoint = live.encode_state_bytes().unwrap();
    let inputs = vec![
        TasInputFrame {
            p1_buttons: 1,
            ..TasInputFrame::default()
        },
        TasInputFrame {
            p1_dpad: 2,
            ..TasInputFrame::default()
        },
    ];
    for input in &inputs {
        expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: input.p1_buttons,
            dpad: input.p1_dpad,
            ..Default::default()
        });
        expected.step_frame();
    }
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(live);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 901,
        profile: TasExecutionProfile::DirectGbCartridgeCgb,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 901,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectGbCartridgeCgb);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0xC0; 32]),
        branch_prefix_sha256: super::synthetic_input_prefix_sha256(&inputs),
        target_cursor: inputs.len() as u64,
    };
    assert!(
        emu_loop.handle_command(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: TasExecutionProfile::DirectGbCartridgeCgb,
                lease_id,
                run_id: 1,
                cache_proof: proof,
                intermediate_cache_proofs: Vec::new(),
                predecessor_window: None,
                start_state_bytes: start_state,
                input_prefix: inputs,
            }
        )))
    );
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectGbCartridgeCgb,
            lease_id: actual_lease_id,
            run_id: 1,
            frame_count,
            state_sha256,
            ..
        } if actual_lease_id == lease_id => (frame_count, state_sha256),
        _ => panic!("unexpected execution response"),
    };
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    let input = TasInputFrame {
        p1_buttons: 2,
        ..TasInputFrame::default()
    };
    expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
        buttons: input.p1_buttons,
        ..Default::default()
    });
    expected.step_frame();
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectGbCartridgeCgb,
                lease_id,
                run_id: 1,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 2,
                expected_executed_project_frames: 2,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input,
                snapshot: None,
            }
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectGbCartridgeCgb,
            lease_id: actual_lease_id,
            run_id: 1,
            advance_id: 1,
            ..
        } if actual_lease_id == lease_id
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: actual, .. } if actual == lease_id
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

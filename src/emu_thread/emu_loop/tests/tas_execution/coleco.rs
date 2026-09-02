use super::super::support::tas_nes_test_loop_from_backend;
use crate::emu_backend::loader::DirectColecoTasExecutionLoader;
use crate::emu_thread::{
    EmuCommand, EmuResponse, TasExecutionCacheProof, TasExecutionPredecessorWindow,
    TasExecutionProfile, TasExecutionRequest, TasFrameAdvanceRequest, TasInputFrame,
};
use crate::tas_project::{TasColecoControllerInput, TasColecoKeypadKey, TasDigest};

static TEST_BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
    [0; zeff_coleco_core::constants::BIOS_SIZE];

fn loader_and_project(
    label: &str,
) -> (
    crate::test_support::TestDirectory,
    DirectColecoTasExecutionLoader,
    crate::tas_project::TasProject,
) {
    let directory = crate::test_support::test_directory(label).unwrap();
    let path = directory.path().join("game.col");
    let mut rom = vec![0; 8 * 1024];
    rom[..2].copy_from_slice(&[0xAA, 0x55]);
    std::fs::write(&path, rom).unwrap();
    let loader =
        DirectColecoTasExecutionLoader::new_with_bios_override(path, Vec::new(), &TEST_BIOS);
    let project = loader.create_project().unwrap();
    (directory, loader, project)
}

fn request(
    lease_id: u64,
    run_id: u64,
    start_state_bytes: Vec<u8>,
    input_prefix: Vec<TasInputFrame>,
) -> EmuCommand {
    let target_cursor = input_prefix.len() as u64;
    EmuCommand::ExecuteTasControl(Box::new(TasExecutionRequest {
        profile: TasExecutionProfile::DirectColecoCartridge,
        lease_id,
        run_id,
        intermediate_cache_proofs: Vec::new(),
        cache_proof: TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0xC0; 32]),
            branch_prefix_sha256: TasDigest([0x1E; 32]),
            target_cursor,
        },
        predecessor_window: None,
        start_state_bytes,
        input_prefix,
    }))
}

#[test]
fn direct_coleco_worker_executes_advances_and_rolls_back_exact_semantic_input() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-coleco");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let checkpoint = backend.encode_state_bytes().unwrap();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 90,
        profile: TasExecutionProfile::DirectColecoCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 90,
            lease_id,
            witness,
        } => {
            assert_eq!(witness.profile, TasExecutionProfile::DirectColecoCartridge);
            (lease_id, witness.current_state_bytes)
        }
        _ => panic!("unexpected acquisition response"),
    };
    let inputs = vec![TasInputFrame {
        coleco: [
            TasColecoControllerInput {
                left: true,
                left_button: true,
                keypad: TasColecoKeypadKey::Star,
                ..Default::default()
            },
            TasColecoControllerInput {
                right: true,
                right_button: true,
                keypad: TasColecoKeypadKey::Nine,
                ..Default::default()
            },
        ],
        ..Default::default()
    }];
    expected.apply_coleco_tas_input(inputs[0].coleco).unwrap();
    expected.step_frame();

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, inputs)));
    let (frame_count, state_sha256) = match responses.recv().unwrap() {
        EmuResponse::TasExecutionCompleted {
            profile: TasExecutionProfile::DirectColecoCartridge,
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

    let next_input = TasInputFrame {
        coleco: [
            TasColecoControllerInput {
                up: true,
                keypad: TasColecoKeypadKey::Pound,
                ..Default::default()
            },
            Default::default(),
        ],
        ..Default::default()
    };
    expected.apply_coleco_tas_input(next_input.coleco).unwrap();
    expected.step_frame();
    assert!(
        emu_loop.handle_command(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: TasExecutionProfile::DirectColecoCartridge,
                lease_id,
                run_id: 1,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 1,
                expected_executed_project_frames: 1,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: next_input,
                snapshot: None,
            },
        )))
    );
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasFrameAdvanced {
            profile: TasExecutionProfile::DirectColecoCartridge,
            frame_count: 2,
            ..
        }
    ));
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { frame_count: 0, .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn direct_coleco_worker_restores_a_semantic_predecessor_and_executes_the_suffix() {
    let (_directory, loader, project) = loader_and_project("tas-control-direct-coleco-cache");
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    let mut expected = loader.load_editor_engine(&project).unwrap().into_backend();
    let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id: 91,
        profile: TasExecutionProfile::DirectColecoCartridge,
    }));
    let (lease_id, start_state) = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 91,
            lease_id,
            witness,
        } => (lease_id, witness.current_state_bytes),
        _ => panic!("unexpected acquisition response"),
    };
    let inputs = vec![
        TasInputFrame {
            coleco: [
                TasColecoControllerInput {
                    keypad: TasColecoKeypadKey::Star,
                    ..Default::default()
                },
                Default::default(),
            ],
            ..Default::default()
        },
        TasInputFrame {
            coleco: [
                Default::default(),
                TasColecoControllerInput {
                    keypad: TasColecoKeypadKey::Nine,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    ];
    let first_proof = TasExecutionCacheProof {
        sync_identity_sha256: TasDigest([0xC0; 32]),
        branch_prefix_sha256: TasDigest([0x1E; 32]),
        target_cursor: 1,
    };
    assert!(emu_loop.handle_command(request(
        lease_id,
        1,
        start_state.clone(),
        inputs[..1].to_vec(),
    )));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 1,
            executed_project_frames: 1,
            ..
        }
    ));

    let mut second = request(lease_id, 2, start_state, inputs.clone());
    let EmuCommand::ExecuteTasControl(second_request) = &mut second else {
        unreachable!();
    };
    second_request.predecessor_window = Some(TasExecutionPredecessorWindow {
        source_proofs: vec![second_request.cache_proof, first_proof],
        input_start_cursor: 1,
        input_frames: inputs[1..].to_vec(),
    });
    assert!(emu_loop.handle_command(second));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            segment_frame_count: 1,
            executed_project_frames: 2,
            ..
        }
    ));
    for input in inputs {
        expected.apply_coleco_tas_input(input.coleco).unwrap();
        expected.step_frame();
    }
    assert_eq!(
        emu_loop.backend.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );
}

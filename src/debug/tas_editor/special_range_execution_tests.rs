use super::*;
use crate::emu_backend::loader::{
    DirectGbaTasExecutionLoader, DirectNesTasExecutionLoader, direct_nes_tas_identity,
};
use crate::tas_project::{TasInitialBranch, TasProject};

fn open_state(
    directory: &crate::test_support::TestDirectory,
    project: &TasProject,
) -> TasEditorWindowState {
    let path = directory.path().join("movie.ztas");
    project.save_atomic(&path).unwrap();
    let mut state = TasEditorWindowState::with_seek_cache_root(directory.path().join("seek"));
    state.reduce(TasEditorAction::OpenProject(path)).unwrap();
    state
}

#[test]
fn production_zapper_identity_exposes_range_edits_and_executes_the_edited_movie() {
    let directory = crate::test_support::test_directory("special-ranges-real-zapper").unwrap();
    let path = directory.path().join("game.nes");
    let rom = crate::test_support::build_nes_test_rom();
    std::fs::write(&path, &rom).unwrap();
    let loader = DirectNesTasExecutionLoader::new(path, Vec::new());
    let (mut backend, _) = loader.load_fresh_backend().unwrap();
    backend.set_zapper_state(true, false, false, Some((120, 80)));
    let start = backend.encode_state_bytes().unwrap();
    let identity = direct_nes_tas_identity(&backend, &rom, &start).unwrap();
    assert!(
        identity
            .devices
            .iter()
            .any(|device| device.device == special_input_editor::NES_STANDARD_OR_ZAPPER_DEVICE)
    );
    assert!(special_input_capabilities(&identity).nes_zapper);
    let mut project = TasProject::new(
        "production-zapper-range",
        identity,
        start,
        Default::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 4,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    project
        .edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                0,
                1,
                TasInputFrame {
                    zapper: TasZapperInput {
                        enabled: true,
                        trigger: true,
                        hit: false,
                        screen_pos: Some([10, 20]),
                    },
                    ..Default::default()
                },
            )?;
            edit.set_input_range(
                "main",
                2,
                1,
                TasInputFrame {
                    zapper: TasZapperInput {
                        enabled: true,
                        trigger: false,
                        hit: true,
                        screen_pos: Some([200, 140]),
                    },
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let mut state = open_state(&directory, &project);
    select(&mut state, 0, 3);
    let action = edit_action(
        &mut state,
        false,
        TasSpecialInputMask {
            zapper: true,
            ..Default::default()
        },
        TasSpecialTransform::Reverse,
    );
    state.reduce(action).unwrap();
    let session = state.session.as_mut().unwrap();
    assert_eq!(
        session.selected_branch().input_at(0).zapper,
        project.branch("main").unwrap().input_at(2).zapper
    );
    DirectNesTasExecutionLoader::validate_project_branch_scope(session.project(), "main").unwrap();
    let mut engine = loader.load_editor_engine(session.project()).unwrap();
    assert!(engine.seek(session, 3).unwrap().reached_target());

    let mut malformed_identity = project.identity().clone();
    malformed_identity.devices[1].configuration_sha256 = TasDigest([0xB7; 32]);
    assert!(special_input_capabilities(&malformed_identity).nes_zapper);
    let malformed = TasProject::new(
        "malformed-zapper-range",
        malformed_identity,
        project.start_state().to_vec(),
        Default::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 0,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )
    .unwrap();
    assert!(loader.load_editor_engine(&malformed).is_err());
}

#[test]
fn production_gba_tilt_range_edit_retains_profile_and_executes_exact_edited_input() {
    let directory = crate::test_support::test_directory("special-ranges-real-gba-tilt").unwrap();
    let path = directory.path().join("tilt.gba");
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"KYGE");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(b"EEPROM_V122");
    std::fs::write(&path, rom).unwrap();
    std::fs::write(path.with_extension("sav"), vec![0x3C; 0x2000]).unwrap();
    let loader = DirectGbaTasExecutionLoader::new(path);
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 1, 2))
        .unwrap();
    assert!(special_input_capabilities(project.identity()).gba_tilt);
    let identity = project.identity().clone();
    for frame in 0..3 {
        let mut input = TasInputFrame {
            tilt_x_bits: (frame as f32 * 0.25).to_bits(),
            tilt_y_bits: (-0.5_f32).to_bits(),
            ..Default::default()
        };
        input.players[0].buttons = 1;
        project
            .edit_transaction(|edit| edit.set_input_range("main", frame, 1, input))
            .unwrap();
    }
    let mut state = open_state(&directory, &project);
    mark(&mut state, 0, 3);
    let action = edit_action(
        &mut state,
        true,
        TasSpecialInputMask {
            tilt_x: true,
            ..Default::default()
        },
        TasSpecialTransform::Reverse,
    );
    state.reduce(action).unwrap();
    let session = state.session.as_mut().unwrap();
    assert_eq!(session.project().identity(), &identity);
    DirectGbaTasExecutionLoader::validate_project_branch_scope(session.project(), "main").unwrap();
    let mut engine = loader.load_editor_engine(session.project()).unwrap();
    assert!(engine.seek(session, 3).unwrap().reached_target());
    let (mut expected, _) = loader.load_fresh_backend().unwrap();
    crate::emu_backend::gba::restore_direct_gba_tas_execution_state(
        &mut expected,
        session.project().start_state(),
    )
    .unwrap();
    for frame in 0..3 {
        let input = session.selected_branch().input_at(frame);
        expected.apply_replay_input(&zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: input.players[0].buttons,
            dpad: input.players[0].dpad,
            host_tilt: (
                f32::from_bits(input.tilt_x_bits),
                f32::from_bits(input.tilt_y_bits),
            ),
            ..Default::default()
        });
        expected.step_frame();
    }
    assert_eq!(
        engine.backend().encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );
}

#[test]
fn production_combined_zapper_name_still_requires_nes_system() {
    let (_directory, state) = special_input_tests::synthetic_state(
        "pce",
        &[special_input_editor::NES_STANDARD_OR_ZAPPER_DEVICE],
        TasInputFrame::default(),
        BTreeMap::new(),
    );
    assert!(
        !special_input_capabilities(state.session.as_ref().unwrap().project().identity())
            .nes_zapper
    );
}

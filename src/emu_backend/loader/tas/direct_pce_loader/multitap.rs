use std::fs;

use super::*;
use crate::test_support::write_zip;

fn pce_rom() -> Vec<u8> {
    let mut rom = vec![0; PCEAS_HEADER_LEN];
    rom[0] = 1;
    rom.extend(vec![0xEA; 0x2000]);
    rom
}

fn five_player_input() -> crate::tas_project::TasInputFrame {
    crate::tas_project::TasInputFrame {
        players: [
            crate::tas_project::TasControllerInput {
                buttons: 0x01,
                dpad: 0x08,
            },
            crate::tas_project::TasControllerInput {
                buttons: 0x02,
                dpad: 0x04,
            },
            crate::tas_project::TasControllerInput {
                buttons: 0x04,
                dpad: 0x02,
            },
            crate::tas_project::TasControllerInput {
                buttons: 0x08,
                dpad: 0x01,
            },
            crate::tas_project::TasControllerInput {
                buttons: 0x0F,
                dpad: 0x05,
            },
        ],
        ..Default::default()
    }
}

#[test]
fn fresh_multitap_project_binds_the_native_reset_mux_state() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-reset-state")?;
    let rom_path = dir.path().join("multitap.pce");
    fs::write(&rom_path, pce_rom())?;

    let loader = DirectPceTasExecutionLoader::new_multitap(rom_path.clone());
    let (backend, _) = loader.load_fresh_backend()?;
    let inspection = validate_direct_pce_multitap_tas_runtime(&backend, false)?;
    let multitap = inspection.controller_multitap.unwrap();
    assert!(
        multitap
            .buttons
            .into_iter()
            .all(|buttons| buttons.is_empty())
    );
    assert_eq!(multitap.active_port, None);
    assert!(multitap.select_high);
    assert!(multitap.clear_high);

    let project = loader.create_project()?;
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
    );
    assert_eq!(
        project
            .identity()
            .devices
            .iter()
            .map(|device| device.port.as_str())
            .collect::<Vec<_>>(),
        ["p1", "p2", "p3", "p4", "p5"]
    );
    Ok(())
}

#[test]
fn multitap_loader_rejects_unsupported_media_without_panicking() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-media-reject")?;
    let path = dir.path().join("game.bin");
    fs::write(&path, [])?;
    let error = DirectPceTasExecutionLoader::new_multitap(path)
        .load_fresh_backend()
        .err()
        .expect("unsupported media should fail");
    assert!(
        error
            .to_string()
            .contains("direct .pce file or selected ZIP member")
    );
    Ok(())
}

#[test]
fn multitap_isolated_seek_and_reseek_preserve_all_five_players() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-isolated-seek")?;
    let rom_path = dir.path().join("multitap.pce");
    fs::write(&rom_path, pce_rom())?;

    let loader = DirectPceTasExecutionLoader::new_multitap(rom_path);
    let mut project = loader.create_project()?;
    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;

    let manual_path = dir.path().join("movie.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let mut engine = loader.load_editor_engine(editor.project())?;

    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let first_state = engine.backend().encode_state_bytes()?;
    let first_framebuffer = engine.backend().framebuffer().to_vec();

    assert!(engine.seek(&mut editor, 0)?.reached_target());
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(engine.backend().encode_state_bytes()?, first_state);
    assert_eq!(engine.backend().framebuffer(), first_framebuffer);

    let (mut expected, _) = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.set_input_p2(input.players[1].buttons, input.players[1].dpad);
    expected.set_input_p3(input.players[2].buttons, input.players[2].dpad);
    expected.set_input_p4(input.players[3].buttons, input.players[3].dpad);
    expected.set_input_p5(input.players[4].buttons, input.players[4].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    assert_eq!(engine.backend().framebuffer(), expected.framebuffer());
    Ok(())
}

#[test]
fn multitap_replay_roundtrip_preserves_all_five_players() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-replay")?;
    let rom_path = dir.path().join("multitap.pce");
    fs::write(&rom_path, pce_rom())?;

    let loader = DirectPceTasExecutionLoader::new_multitap(rom_path.clone());
    let mut project = loader.create_project()?;
    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;

    let manual_path = dir.path().join("source.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("replay-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectPce(loader);
    let replay_path = dir.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let replay = zeff_emu_common::replay::ReplayPlayer::load(&replay_path)?;
    assert_eq!(
        replay.peek_joypad_frames(0, 1).as_slice(),
        &[zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x08,
            buttons_p2: 0x02,
            dpad_p2: 0x04,
            buttons_p3: 0x04,
            dpad_p3: 0x02,
            buttons_p4: 0x08,
            dpad_p4: 0x01,
            buttons_p5: 0x0F,
            dpad_p5: 0x05,
            ..Default::default()
        }]
    );

    let imported_path = dir.path().join("imported.ztas");
    let start_state = crate::tas_project::TasProject::read_zrpl_start_state(&replay_path)?;
    let selected_plan = super::super::select_private_tas_execution_loader_for_replay(
        rom_path.clone(),
        None,
        ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected_plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
    );
    Ok(())
}

#[test]
fn replay_loader_rejects_an_unsupported_start_state() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-replay-reject")?;
    let rom_path = dir.path().join("multitap.pce");
    fs::write(&rom_path, pce_rom())?;

    let error = super::super::select_private_tas_execution_loader_for_replay(
        rom_path,
        None,
        ActiveSystem::Pce,
        Vec::new(),
        b"unsupported-state",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("exact HuCard controller topology")
    );
    Ok(())
}

#[test]
fn selected_zip_multitap_creates_reopens_and_seeks_exact_member() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-zip")?;
    let first = pce_rom();
    let mut selected = pce_rom();
    *selected.last_mut().unwrap() ^= 1;
    let archive_path = dir.path().join("games.zip");
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.pce", &first), ("folder/selected.pce", &selected)],
    )?;
    let selected_path = archive_path.join("folder/selected.pce");
    let loader = DirectPceTasExecutionLoader::new_zip_multitap(
        archive_path.clone(),
        Some(selected_path.clone()),
    );
    let mut project = loader.create_project()?;
    let profile = PceTasHardwareProfile {
        board: PceHuCardBoard::Plain,
        topology: PceHardwareTopology::Base,
        controller_mode: PceControllerMode::Multitap,
    };
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        zip_pce_tas_sync_config_sha256_for_profile(profile, "folder/selected.pce")
    );
    assert_ne!(
        project.identity().sync_config_sha256,
        direct_pce_tas_sync_config_sha256_for_profile(profile)
    );
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
    );

    let reopened =
        DirectPceTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    assert!(
        DirectPceTasExecutionLoader::new_zip_multitap(
            archive_path.clone(),
            Some(archive_path.join("first.pce")),
        )
        .load_editor_engine(&project)
        .is_err()
    );

    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let project_path = dir.path().join("movie.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &project_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("seek-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, project_path, autosaves, cache)?;
    let mut engine = reopened.load_editor_engine(editor.project())?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let (mut expected, _) = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.set_input_p2(input.players[1].buttons, input.players[1].dpad);
    expected.set_input_p3(input.players[2].buttons, input.players[2].dpad);
    expected.set_input_p4(input.players[3].buttons, input.players[3].dpad);
    expected.set_input_p5(input.players[4].buttons, input.players[4].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );

    write_zip(
        &archive_path,
        &[
            ("first.pce", &first),
            ("folder/selected.pce", &selected),
            ("changed.txt", b"changed"),
        ],
    )?;
    assert!(
        DirectPceTasExecutionLoader::new_zip_for_project(archive_path, editor.project()).is_err()
    );
    Ok(())
}

#[test]
fn selected_zip_multitap_replay_roundtrip_preserves_all_players() -> Result<()> {
    let dir = crate::test_support::test_directory("pce-tas-multitap-zip-replay")?;
    let first = pce_rom();
    let mut selected = pce_rom();
    *selected.last_mut().unwrap() ^= 1;
    let archive_path = dir.path().join("games.zip");
    write_zip(
        &archive_path,
        &[("first.pce", &first), ("folder/selected.pce", &selected)],
    )?;
    let selected_path = archive_path.join("folder/selected.pce");
    let loader = DirectPceTasExecutionLoader::new_zip_multitap(
        archive_path.clone(),
        Some(selected_path.clone()),
    );
    let mut project = loader.create_project()?;
    let input = five_player_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;

    let manual_path = dir.path().join("source.ztas");
    let autosaves = crate::tas_project::TasAutosaveStore::beside_manual_save(
        &manual_path,
        crate::tas_project::TasAutosaveConfig::default(),
    )?;
    let cache = crate::tas_project::TasSeekStateCache::open(dir.path().join("replay-cache"))?;
    let mut editor =
        crate::tas_project::TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectPce(loader);
    let replay_path = dir.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let start_state = crate::tas_project::TasProject::read_zrpl_start_state(&replay_path)?;
    let selected_plan = super::super::select_private_tas_execution_loader_for_replay(
        archive_path,
        Some(selected_path),
        ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported =
        selected_plan.import_replay_file(&replay_path, &dir.path().join("imported.ztas"), false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectPceMultitapHuCard
    );
    Ok(())
}

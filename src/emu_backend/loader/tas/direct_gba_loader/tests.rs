use anyhow::Result;
use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayJoypadFrame};

use super::*;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasExternalIdentity, TasInputFrame, TasSeekStateCache,
};
use crate::test_support::write_zip;

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"ABCD");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom
}

fn gba_rom_with_backup(marker: &[u8]) -> Vec<u8> {
    let mut rom = gba_rom();
    rom.extend_from_slice(marker);
    rom
}

fn gba_rtc_rom(marker: &[u8]) -> Vec<u8> {
    let mut rom = gba_rom_with_backup(marker);
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom
}

fn gba_tilt_rom() -> Vec<u8> {
    let mut rom = gba_rom_with_backup(b"EEPROM_V122");
    rom[0xAC..0xB0].copy_from_slice(b"KYGE");
    rom
}

fn loader(
    label: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectGbaTasExecutionLoader,
    Vec<u8>,
)> {
    let directory = crate::test_support::test_directory(label)?;
    let path = directory.path().join("game.gba");
    let rom = gba_rom();
    std::fs::write(&path, &rom)?;
    Ok((directory, DirectGbaTasExecutionLoader::new(path), rom))
}

fn keypad_input() -> TasInputFrame {
    TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0x31,
                dpad: 0x04,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..TasInputFrame::default()
    }
}

#[test]
fn creates_and_executes_a_one_pad_direct_gba_project() -> Result<()> {
    let (directory, loader, rom) = loader("tas-direct-gba-isolated")?;
    let mut project = loader.create_project()?;
    assert_eq!(
        project.project_id(),
        format!(
            "gba-{}",
            crate::tas_project::TasDigest::from_bytes(&rom).to_hex()
        )
    );
    assert_eq!(project.identity().devices.len(), 1);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        crate::emu_thread::TasExecutionProfile::DirectGbaCartridge
    );

    let input = keypad_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;

    let outcome = engine.seek(&mut editor, 1)?;
    let (mut expected, _) = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert!(
        crate::emu_backend::gba::validate_direct_gba_tas_execution_runtime(&expected, false)
            .is_ok()
    );
    assert!(outcome.reached_target());
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    Ok(())
}

#[test]
fn tilt_project_preserves_recorded_sensor_input_through_direct_execution_and_replay() -> Result<()>
{
    let directory = crate::test_support::test_directory("tas-gba-tilt-direct")?;
    let source_path = directory.path().join("tilt.gba");
    let save_path = source_path.with_extension("sav");
    let rom = gba_tilt_rom();
    let save = vec![0x3C; 0x2000];
    std::fs::write(&source_path, &rom)?;
    std::fs::write(&save_path, &save)?;
    let loader = DirectGbaTasExecutionLoader::new(source_path);
    let mut project = loader.create_project()?;
    assert_eq!(project.identity().devices.len(), 2);
    assert_eq!(
        project.identity().determinism_abi,
        zeff_gba_core::save_state::TILT_TAS_DETERMINISM_ABI_ID
    );
    assert_eq!(
        project.identity().state_format_compatibility_id,
        zeff_gba_core::save_state::TILT_TAS_STATE_FORMAT_COMPATIBILITY_ID
    );
    assert_ne!(project.identity().sensor_state, TasExternalIdentity::Absent);

    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0x21,
                dpad: 0x04,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        tilt_x_bits: 0x3E80_0000,
        tilt_y_bits: 0xBF00_0000,
        ..TasInputFrame::default()
    };
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let mut engine = loader.load_editor_engine(&project)?;
    let manual_path = directory.path().join("tilt.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let (mut expected, _) = loader.load_fresh_backend()?;
    crate::emu_backend::gba::restore_direct_gba_tas_execution_state(
        &mut expected,
        editor.project().start_state(),
    )?;
    expected.apply_replay_input(&ReplayJoypadFrame {
        buttons: input.players[0].buttons,
        dpad: input.players[0].dpad,
        host_tilt: (
            f32::from_bits(input.tilt_x_bits),
            f32::from_bits(input.tilt_y_bits),
        ),
        ..ReplayJoypadFrame::default()
    });
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );

    let plan = super::super::PrivateTasExecutionLoader::DirectGba(loader);
    let replay_path = directory.path().join("tilt.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    let imported =
        plan.import_replay_file(&replay_path, &directory.path().join("imported.ztas"), false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    Ok(())
}

#[test]
fn tilt_zip_reopens_by_member_identity_and_rejects_unsupported_topology() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-tilt-zip")?;
    let archive_path = directory.path().join("tilt.zip");
    let rom = gba_tilt_rom();
    let save = vec![0xA4; 0x2000];
    write_zip(&archive_path, &[("games/tilt.gba", &rom)])?;
    std::fs::write(archive_path.with_extension("sav"), &save)?;
    let loader = DirectGbaTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("games/tilt.gba")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        crate::emu_backend::gba::zip_gba_tilt_tas_sync_config_sha256("games/tilt.gba")
    );
    let reopened = DirectGbaTasExecutionLoader::new_zip_for_project(archive_path, &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    let unsupported_path = directory.path().join("unsupported.gba");
    let mut unsupported = gba_rom();
    unsupported[0xAC..0xB0].copy_from_slice(b"KYGE");
    std::fs::write(&unsupported_path, unsupported)?;
    assert!(
        DirectGbaTasExecutionLoader::new(unsupported_path)
            .create_project()
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_host_configuration_and_unowned_input() -> Result<()> {
    let (_directory, loader, _) = loader("tas-direct-gba-rejections")?;
    let (backend, _) = loader.load_fresh_backend()?;
    assert!(crate::emu_backend::gba::validate_direct_gba_tas_runtime(&backend, false).is_ok());
    assert!(crate::emu_backend::gba::validate_direct_gba_tas_runtime(&backend, true).is_err());

    for config in [
        BackendLoadConfig {
            sample_rate: Some(44_100),
            gba_load_battery_sram: false,
            gba_use_external_bios: false,
            ..BackendLoadConfig::default()
        },
        BackendLoadConfig {
            initial_input: Some((1, 0)),
            gba_load_battery_sram: false,
            gba_use_external_bios: false,
            ..BackendLoadConfig::default()
        },
    ] {
        let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
            ActiveSystem::GameBoyAdvance,
            &loader.source_path,
            gba_rom(),
            config,
        )?
        .backend;
        assert!(crate::emu_backend::gba::validate_direct_gba_tas_runtime(&backend, false).is_err());
    }

    let mut project = loader.create_project()?;
    let mut input = keypad_input();
    input.players[1].buttons = 1;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert!(DirectGbaTasExecutionLoader::validate_project_branch_scope(&project, "main").is_err());

    let mut sensor_project = loader.create_project()?;
    let mut sensor_input = keypad_input();
    sensor_input.tilt_x_bits = 1;
    sensor_project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, sensor_input))?;
    assert!(
        DirectGbaTasExecutionLoader::validate_project_branch_scope(&sensor_project, "main")
            .is_err()
    );

    let mut link_project = loader.create_project()?;
    link_project.edit_transaction(|edit| {
        edit.replace_branch_events(
            "main",
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 1,
                    clock_period_t_cycles: 512,
                    out_byte: 0xA5,
                    serial_generation: 0,
                },
            }],
        )
    })?;
    assert!(
        DirectGbaTasExecutionLoader::validate_project_branch_scope(&link_project, "main").is_err()
    );
    Ok(())
}

#[test]
fn rtc_project_owns_fixed_epoch_backup_and_two_pass_execution() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-rtc")?;
    let source_path = directory.path().join("emerald.gba");
    let save_path = source_path.with_extension("sav");
    let initial_save = (0..0x20000)
        .map(|index| (index as u8).wrapping_mul(13).wrapping_add(5))
        .collect::<Vec<_>>();
    std::fs::write(&source_path, gba_rtc_rom(b"FLASH1M_V103"))?;
    std::fs::write(&save_path, &initial_save)?;
    let loader = DirectGbaTasExecutionLoader::new(source_path);
    let mut project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_save))
    );
    assert!(matches!(
        project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));
    let mut wrong_identity = project.identity().clone();
    wrong_identity.rtc_state = TasExternalIdentity::ExternalSha256(TasDigest([0xA7; 32]));
    let wrong_project = TasProject::new(
        "wrong-gba-rtc".to_owned(),
        wrong_identity,
        project.start_state().to_vec(),
        ReplayStartMetadata::default(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: "Main".to_owned(),
            frame_count: 1,
            input_spans: Vec::new(),
            events: Vec::new(),
        },
        BTreeMap::new(),
    )?;
    assert!(loader.load_editor_engine(&wrong_project).is_err());

    let (backend, _) = loader.load_fresh_backend()?;
    assert!(
        crate::emu_backend::gba::validate_direct_gba_tas_private_runtime(&backend, false).is_ok()
    );
    assert!(crate::emu_backend::gba::validate_direct_gba_tas_runtime(&backend, false).is_err());
    let gba = backend.gba().expect("GBA backend");
    let inspection = zeff_gba_core::save_state::inspect_current_native_gba_tas_state(
        &gba.emu,
        project.start_state(),
    )?;
    let rtc = inspection.rtc_state.expect("RTC state");
    assert_eq!(
        (
            rtc.date_time.year(),
            rtc.date_time.month(),
            rtc.date_time.day(),
            rtc.date_time.weekday(),
            rtc.date_time.hour(),
            rtc.date_time.minute(),
            rtc.date_time.second(),
            rtc.subsecond_cycles,
        ),
        (2000, 1, 1, 6, 0, 0, 0, 0)
    );

    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, keypad_input()))?;
    let changed_save = vec![0xA6; initial_save.len()];
    std::fs::write(&save_path, &changed_save)?;
    let manual_path = directory.path().join("emerald.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    super::super::PrivateTasExecutionLoader::DirectGba(loader)
        .verify_and_export_editor_session(&mut editor, &directory.path().join("emerald.zrpl"))?;
    assert!(editor.project().verification_is_current("main")?);
    assert_eq!(std::fs::read(save_path)?, changed_save);
    Ok(())
}

#[test]
fn selected_zip_rtc_project_reopens_by_exact_member_identity() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-rtc-zip")?;
    let archive_path = directory.path().join("emerald.zip");
    let save_path = archive_path.with_extension("sav");
    let rom = gba_rtc_rom(b"FLASH1M_V103");
    let initial_save = vec![0x39; 0x20000];
    write_zip(&archive_path, &[("games/emerald.gba", &rom)])?;
    std::fs::write(&save_path, &initial_save)?;
    let loader = DirectGbaTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("games/emerald.gba")),
    );
    let project = loader.create_project()?;
    assert!(matches!(
        project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));
    let reopened =
        DirectGbaTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    write_zip(
        &archive_path,
        &[("games/emerald.gba", &rom), ("changed.txt", b"x")],
    )?;
    assert!(DirectGbaTasExecutionLoader::new_zip_for_project(archive_path, &project).is_err());
    assert_eq!(std::fs::read(save_path)?, initial_save);
    Ok(())
}

#[test]
fn gba_rtc_complete_persistence_witness_covers_direct_zip_and_timer_only() -> Result<()> {
    for (label, zip, marker, backup_len) in [
        ("direct-timer", false, b"".as_slice(), 0usize),
        ("direct-flash", false, b"FLASH1M_V103".as_slice(), 0x20000),
        ("zip-timer", true, b"".as_slice(), 0),
        ("zip-flash", true, b"FLASH1M_V103".as_slice(), 0x20000),
    ] {
        let directory = crate::test_support::test_directory(&format!("tas-gba-rtc-{label}"))?;
        let source_path = directory
            .path()
            .join(if zip { "emerald.zip" } else { "emerald.gba" });
        let rom = gba_rtc_rom(marker);
        let loader = if zip {
            write_zip(&source_path, &[("games/emerald.gba", &rom)])?;
            DirectGbaTasExecutionLoader::new_zip(
                source_path.clone(),
                Some(source_path.join("games/emerald.gba")),
            )
        } else {
            std::fs::write(&source_path, &rom)?;
            DirectGbaTasExecutionLoader::new(source_path.clone())
        };
        if backup_len != 0 {
            std::fs::write(source_path.with_extension("sav"), vec![0x4D; backup_len])?;
        }
        let project = loader.create_project()?;
        let backend = super::super::PrivateTasExecutionLoader::DirectGba(loader)
            .load_repair_backend(&project)?;
        let witness = crate::emu_backend::gba::gba_rtc_persistence_witness(&backend)?;
        assert_eq!(
            witness.persistent_state,
            project.identity().persistent_state
        );
        assert_eq!(witness.rtc_state, project.identity().rtc_state);
        assert_eq!(witness.complete_byte_len, (backup_len + 40) as u64);
        let complete = backend
            .gba()
            .unwrap()
            .emu
            .dump_complete_rtc_persistence()
            .unwrap();
        assert_eq!(witness.complete_sha256, TasDigest::from_bytes(&complete));
    }
    Ok(())
}

#[test]
fn opening_refuses_changed_media_and_the_wrong_extension() -> Result<()> {
    let (directory, loader, mut rom) = loader("tas-direct-gba-identity")?;
    let project = loader.create_project()?;
    rom[0] ^= 1;
    std::fs::write(&loader.source_path, rom)?;
    assert!(loader.load_session(project.start_state()).is_err());

    let wrong = directory.path().join("game.zip");
    std::fs::write(&wrong, gba_rom())?;
    assert!(
        DirectGbaTasExecutionLoader::new(wrong)
            .create_project()
            .is_err()
    );
    Ok(())
}

#[test]
fn replay_export_import_uses_two_fresh_direct_gba_passes() -> Result<()> {
    let (directory, loader, _) = loader("tas-direct-gba-replay")?;
    let mut project = loader.create_project()?;
    let input = keypad_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual_path = directory.path().join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("replay-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectGba(loader.clone());
    let replay_path = directory.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    assert!(editor.project().verification_is_current("main")?);

    let imported_path = directory.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectGbaCartridge
    );
    Ok(())
}

#[test]
fn selected_zip_member_creates_reopens_and_rejects_archive_changes() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = gba_rom();
    let mut selected = first.clone();
    selected[0] = 1;
    let archive_bytes = write_zip(
        &archive_path,
        &[("first.gba", &first), ("folder/selected.gba", &selected)],
    )?;
    let loader = DirectGbaTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.gba")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive_bytes)
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        TasDigest::from_bytes(&selected)
    );
    let reopened =
        DirectGbaTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );

    write_zip(
        &archive_path,
        &[
            ("first.gba", &first),
            ("folder/selected.gba", &selected),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(DirectGbaTasExecutionLoader::new_zip_for_project(archive_path, &project).is_err());
    Ok(())
}

#[test]
fn zip_gba_keeps_rtc_and_selection_gates() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-zip-gates")?;
    let archive_path = directory.path().join("games.zip");
    let persistent = gba_rtc_rom(b"SRAM_V113");
    write_zip(&archive_path, &[("persistent.gba", &persistent)])?;
    let rtc_project =
        DirectGbaTasExecutionLoader::new_zip(archive_path.clone(), None).create_project()?;
    assert!(matches!(
        rtc_project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));

    let plain = gba_rom();
    write_zip(&archive_path, &[("one.gba", &plain), ("two.gba", &plain)])?;
    assert!(
        DirectGbaTasExecutionLoader::new_zip(archive_path, None)
            .create_project()
            .is_err()
    );
    Ok(())
}

#[test]
fn battery_projects_bind_recognized_cartridge_saves_once() -> Result<()> {
    for (label, marker, len) in [
        ("sram", b"SRAM_V113".as_slice(), 0x10000usize),
        ("flash512", b"FLASH512_V131".as_slice(), 0x10000),
        ("flash1m", b"FLASH1M_V103".as_slice(), 0x20000),
        ("eeprom", b"EEPROM_V122".as_slice(), 0x2000),
    ] {
        let directory = crate::test_support::test_directory(&format!("tas-gba-save-{label}"))?;
        let source_path = directory.path().join("game.gba");
        let save_path = source_path.with_extension("sav");
        let initial_save = (0..len)
            .map(|index| (index as u8).wrapping_mul(19).wrapping_add(7))
            .collect::<Vec<_>>();
        std::fs::write(&source_path, gba_rom_with_backup(marker))?;
        std::fs::write(&save_path, &initial_save)?;
        let loader = DirectGbaTasExecutionLoader::new(source_path);
        let mut project = loader.create_project()?;
        assert_eq!(
            project.identity().persistent_state,
            TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_save))
        );
        project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, keypad_input()))?;
        let start_state = project.start_state().to_vec();

        let changed_save = vec![0xD3; len];
        std::fs::write(&save_path, &changed_save)?;
        let mut wrong_identity = project.identity().clone();
        wrong_identity.persistent_state =
            TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(b"wrong save"));
        let wrong_project = TasProject::new(
            format!("wrong-gba-save-{label}"),
            wrong_identity,
            project.start_state().to_vec(),
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: 1,
                input_spans: Vec::new(),
                events: Vec::new(),
            },
            BTreeMap::new(),
        )?;
        assert!(loader.load_editor_engine(&wrong_project).is_err());

        let mut engine = loader.load_editor_engine(&project)?;
        let manual_path = directory.path().join("manual.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
        let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
        let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
        assert!(engine.seek(&mut editor, 1)?.reached_target());
        assert_eq!(std::fs::read(&save_path)?, changed_save);
        let mut backend = engine.into_backend();
        crate::emu_backend::gba::restore_direct_gba_tas_execution_state(
            &mut backend,
            &start_state,
        )?;
        assert_eq!(backend.encode_state_bytes()?, start_state);
    }
    Ok(())
}

#[test]
fn battery_project_rejects_mismatched_save_and_uses_zip_outer_stem() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-save-zip")?;
    let source_path = directory.path().join("game.gba");
    std::fs::write(&source_path, gba_rom_with_backup(b"SRAM_V113"))?;
    std::fs::write(source_path.with_extension("sav"), [0x55; 31])?;
    assert!(
        DirectGbaTasExecutionLoader::new(source_path)
            .create_project()
            .is_err()
    );

    let archive_path = directory.path().join("golden-sun.zip");
    let save_path = archive_path.with_extension("sav");
    let rom = gba_rom_with_backup(b"SRAM_V113");
    let initial_save = vec![0xA5; 0x10000];
    write_zip(&archive_path, &[("Golden Sun.gba", &rom)])?;
    std::fs::write(&save_path, &initial_save)?;
    let loader = DirectGbaTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("Golden Sun.gba")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_save))
    );
    let changed_save = vec![0x3C; initial_save.len()];
    std::fs::write(&save_path, &changed_save)?;
    let manual_path = directory.path().join("golden-sun.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    super::super::PrivateTasExecutionLoader::DirectGba(loader)
        .verify_and_export_editor_session(&mut editor, &directory.path().join("golden-sun.zrpl"))?;
    assert_eq!(std::fs::read(&save_path)?, changed_save);
    Ok(())
}

#[test]
fn rejects_unknown_and_ambiguous_backup_markers() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-gba-backup-kind")?;
    for (name, markers) in [
        ("unknown.gba", b"FLASH256_V100".as_slice()),
        ("ambiguous.gba", b"SRAM_V113FLASH1M_V103".as_slice()),
    ] {
        let path = directory.path().join(name);
        std::fs::write(path.clone(), gba_rom_with_backup(markers))?;
        assert!(
            DirectGbaTasExecutionLoader::new(path)
                .create_project()
                .is_err()
        );
    }
    Ok(())
}

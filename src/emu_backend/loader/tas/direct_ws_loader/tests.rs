use std::collections::BTreeMap;

use anyhow::Result;
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_ws_core::hardware::cartridge::{RomOrientation, SaveKind, compute_footer_checksum};

use super::*;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasExternalIdentity, TasInputFrame, TasSeekStateCache,
};
use crate::test_support::write_zip;

fn ws_rom(system: u8, orientation: RomOrientation, save_kind: u8, rtc: bool) -> Vec<u8> {
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer..].fill(0);
    rom[footer + 1] = system;
    rom[footer + 4] = 0x01;
    rom[footer + 5] = save_kind;
    rom[footer + 6] = u8::from(orientation == RomOrientation::Vertical);
    rom[footer + 7] = u8::from(rtc);
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn loader(
    label: &str,
    extension: &str,
    orientation: RomOrientation,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectWsTasExecutionLoader,
    Vec<u8>,
)> {
    let directory = crate::test_support::test_directory(label)?;
    let path = directory.path().join(format!("game.{extension}"));
    let system = u8::from(extension == "wsc");
    let rom = ws_rom(system, orientation, 0, false);
    std::fs::write(&path, &rom)?;
    Ok((directory, DirectWsTasExecutionLoader::new(path), rom))
}

fn keypad_input() -> TasInputFrame {
    TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0x91,
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
fn creates_and_executes_direct_ws_and_wsc_projects() -> Result<()> {
    for (extension, orientation) in [
        ("ws", RomOrientation::Horizontal),
        ("wsc", RomOrientation::Vertical),
    ] {
        let (directory, loader, rom) = loader(
            &format!("tas-direct-ws-{extension}"),
            extension,
            orientation,
        )?;
        let mut project = loader.create_project()?;
        assert_eq!(
            project.project_id(),
            format!(
                "ws-{}",
                crate::tas_project::TasDigest::from_bytes(&rom).to_hex()
            )
        );
        assert_eq!(
            super::super::direct_ws_tas_orientation(&project)?,
            orientation
        );
        assert_eq!(
            super::super::classify_direct_tas_execution_profile(&project)?,
            crate::emu_thread::TasExecutionProfile::DirectWsCartridge
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
        expected.set_input(0x91, 0x04);
        expected.step_frame();
        assert!(outcome.reached_target());
        assert_eq!(
            engine.backend().encode_state_bytes()?,
            expected.encode_state_bytes()?
        );
    }
    Ok(())
}

#[test]
fn rejects_wrong_extension_system_footer_and_host_configuration() -> Result<()> {
    let (directory, loader, _) =
        loader("tas-direct-ws-rejections", "ws", RomOrientation::Horizontal)?;
    let (backend, _) = loader.load_fresh_backend()?;
    assert!(super::super::validate_direct_ws_tas_runtime(&backend, false).is_ok());
    assert!(super::super::validate_direct_ws_tas_runtime(&backend, true).is_err());

    for (name, rom) in [
        ("color.ws", ws_rom(1, RomOrientation::Horizontal, 0, false)),
        (
            "unknown-save.ws",
            ws_rom(0, RomOrientation::Horizontal, 0x7F, false),
        ),
        (
            "unknown-rtc-save.ws",
            ws_rom(0, RomOrientation::Horizontal, 0x7F, true),
        ),
    ] {
        let path = directory.path().join(name);
        std::fs::write(&path, rom)?;
        assert!(
            DirectWsTasExecutionLoader::new(path)
                .create_project()
                .is_err()
        );
    }

    let wrong = directory.path().join("game.zip");
    std::fs::write(&wrong, ws_rom(0, RomOrientation::Horizontal, 0, false))?;
    assert!(
        DirectWsTasExecutionLoader::new(wrong)
            .create_project()
            .is_err()
    );

    for config in [
        BackendLoadConfig {
            sample_rate: Some(44_100),
            ws_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
        BackendLoadConfig {
            initial_input: Some((1, 0)),
            ws_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    ] {
        let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
            ActiveSystem::WonderSwan,
            &loader.source_path,
            ws_rom(0, RomOrientation::Horizontal, 0, false),
            config,
        )?
        .backend;
        assert!(super::super::validate_direct_ws_tas_runtime(&backend, false).is_err());
    }
    Ok(())
}

#[test]
fn rtc_projects_use_a_fixed_epoch_and_deterministically_tick_without_host_state() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-direct-ws-rtc")?;
    let source_path = directory.path().join("clock.wsc");
    std::fs::write(&source_path, ws_rom(1, RomOrientation::Vertical, 0, true))?;
    let loader = DirectWsTasExecutionLoader::new(source_path);
    let mut project = loader.create_project()?;
    let repeated = loader.create_project()?;
    assert_eq!(project.start_state(), repeated.start_state());
    assert_eq!(project.identity(), repeated.identity());
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert!(matches!(
        project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));
    let mut wrong_identity = project.identity().clone();
    wrong_identity.rtc_state = TasExternalIdentity::ExternalSha256(TasDigest([0xA7; 32]));
    let wrong_project = TasProject::new(
        "wrong-ws-rtc",
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
    assert!(super::super::validate_direct_ws_tas_runtime(&backend, false).is_err());
    assert!(super::super::validate_direct_ws_tas_private_runtime(&backend, false).is_ok());
    project.edit_transaction(|edit| edit.insert_frames("main", 1, 90))?;

    let mut first = loader.load_editor_engine(&project)?;
    let mut second = loader.load_editor_engine(&project)?;
    let first_path = directory.path().join("first.ztas");
    let second_path = directory.path().join("second.ztas");
    let first_autosaves =
        TasAutosaveStore::beside_manual_save(&first_path, TasAutosaveConfig::default())?;
    let second_autosaves =
        TasAutosaveStore::beside_manual_save(&second_path, TasAutosaveConfig::default())?;
    let first_cache = TasSeekStateCache::open(directory.path().join("first-cache"))?;
    let second_cache = TasSeekStateCache::open(directory.path().join("second-cache"))?;
    let mut first_editor =
        TasEditorSession::new(project.clone(), first_path, first_autosaves, first_cache)?;
    let mut second_editor =
        TasEditorSession::new(project, second_path, second_autosaves, second_cache)?;
    assert!(first.seek(&mut first_editor, 91)?.reached_target());
    assert!(second.seek(&mut second_editor, 91)?.reached_target());
    let first_state = first.backend().encode_state_bytes()?;
    assert_eq!(first_state, second.backend().encode_state_bytes()?);
    let ws = first.backend().ws().unwrap();
    let inspection = zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(
        &ws.emu,
        &first_state,
    )?;
    assert_ne!(inspection.rtc.payload[6], 0);
    Ok(())
}

#[test]
fn rtc_zip_with_eeprom_owns_backup_and_archive_identity_once() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-ws-rtc-zip")?;
    let archive_path = directory.path().join("clocks.zip");
    let selected = ws_rom(1, RomOrientation::Horizontal, 0x10, true);
    let archive = write_zip(&archive_path, &[("folder/clock.wsc", &selected)])?;
    let save = (0..128).map(|value| value as u8 ^ 0xA5).collect::<Vec<_>>();
    let save_path = archive_path.with_extension("sav");
    std::fs::write(&save_path, &save)?;
    let loader = DirectWsTasExecutionLoader::new_zip(archive_path.clone(), None);
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive)
    );
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&save))
    );
    assert!(matches!(
        project.identity().rtc_state,
        TasExternalIdentity::ExternalSha256(_)
    ));
    std::fs::write(&save_path, [0x3C; 128])?;
    let reopened = DirectWsTasExecutionLoader::new_zip_for_project(archive_path, &project)?;
    let mut engine = reopened.load_editor_engine(&project)?;
    let manual_path = directory.path().join("clock.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("clock-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    assert_eq!(std::fs::read(save_path)?, [0x3C; 128]);
    Ok(())
}

#[test]
fn rtc_persistence_witness_covers_direct_and_selected_zip_media() -> Result<()> {
    for (label, zip, save_byte) in [
        ("direct-timer", false, 0),
        ("direct-eeprom", false, 0x10),
        ("zip-timer", true, 0),
        ("zip-eeprom", true, 0x10),
    ] {
        let directory = crate::test_support::test_directory(&format!("tas-ws-rtc-{label}"))?;
        let source_path = directory
            .path()
            .join(if zip { "clocks.zip" } else { "clock.wsc" });
        let rom = ws_rom(1, RomOrientation::Vertical, save_byte, true);
        let loader = if zip {
            let other = ws_rom(0, RomOrientation::Horizontal, 0, false);
            write_zip(
                &source_path,
                &[("other.ws", &other), ("games/clock.wsc", &rom)],
            )?;
            DirectWsTasExecutionLoader::new_zip(
                source_path.clone(),
                Some(source_path.join("games/clock.wsc")),
            )
        } else {
            std::fs::write(&source_path, &rom)?;
            DirectWsTasExecutionLoader::new(source_path.clone())
        };
        let save_kind = SaveKind::from_byte(save_byte);
        let backup = vec![0x6B; save_kind.size()];
        if !backup.is_empty() {
            std::fs::write(source_path.with_extension("sav"), &backup)?;
        }
        let project = loader.create_project()?;
        let backend = super::super::PrivateTasExecutionLoader::DirectWs(loader)
            .load_repair_backend(&project)?;
        let witness = crate::emu_backend::ws::ws_rtc_persistence_witness(&backend)?;
        let complete = backend.ws().unwrap().tas_rtc_battery_bytes().unwrap();
        assert_eq!(witness.save_kind, save_kind);
        assert_eq!(
            witness.persistent_state,
            project.identity().persistent_state
        );
        assert_eq!(witness.rtc_state, project.identity().rtc_state);
        assert_eq!(witness.complete_byte_len, (backup.len() + 24) as u64);
        assert_eq!(witness.complete_sha256, TasDigest::from_bytes(&complete));
        assert_eq!(&complete[..backup.len()], backup.as_slice());
    }
    Ok(())
}

#[test]
fn battery_projects_bind_exact_sram_and_eeprom_once_without_later_sidecar_io() -> Result<()> {
    for (label, extension, kind) in [("sram", "ws", 0x03), ("eeprom", "wsc", 0x20)] {
        let directory = crate::test_support::test_directory(&format!("tas-direct-ws-{label}"))?;
        let source_path = directory.path().join(format!("game.{extension}"));
        let save_path = source_path.with_extension("sav");
        let initial_save = (0..zeff_ws_core::hardware::cartridge::SaveKind::from_byte(kind).size())
            .map(|index| (index as u8).wrapping_mul(17).wrapping_add(11))
            .collect::<Vec<_>>();
        std::fs::write(
            &source_path,
            ws_rom(
                u8::from(extension == "wsc"),
                RomOrientation::Horizontal,
                kind,
                false,
            ),
        )?;
        std::fs::write(&save_path, &initial_save)?;
        let loader = DirectWsTasExecutionLoader::new(source_path);

        let project = loader.create_project()?;
        assert_eq!(
            project.identity().persistent_state,
            TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&initial_save))
        );

        let changed_sidecar = vec![0xD3; initial_save.len()];
        std::fs::write(&save_path, &changed_sidecar)?;
        let mut wrong_identity = project.identity().clone();
        wrong_identity.persistent_state =
            TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(b"wrong save"));
        let wrong_project = TasProject::new(
            "wrong-ws-save",
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
        assert_eq!(std::fs::read(&save_path)?, changed_sidecar);
    }
    Ok(())
}

#[test]
fn accepts_all_recognized_exact_save_sizes_and_rejects_mismatches() -> Result<()> {
    for rtc in [false, true] {
        for kind in [0x01, 0x02, 0x03, 0x04, 0x05, 0x10, 0x20, 0x50] {
            let directory = crate::test_support::test_directory(&format!(
                "tas-direct-ws-save-{kind:02x}-rtc-{rtc}"
            ))?;
            let source_path = directory.path().join("game.ws");
            let save_path = source_path.with_extension("sav");
            let expected_len = zeff_ws_core::hardware::cartridge::SaveKind::from_byte(kind).size();
            std::fs::write(
                &source_path,
                ws_rom(0, RomOrientation::Horizontal, kind, rtc),
            )?;
            std::fs::write(&save_path, vec![kind; expected_len])?;
            assert!(
                DirectWsTasExecutionLoader::new(source_path)
                    .create_project()
                    .is_ok()
            );
        }
    }

    let directory = crate::test_support::test_directory("tas-direct-ws-save-size")?;
    let source_path = directory.path().join("game.ws");
    std::fs::write(
        &source_path,
        ws_rom(0, RomOrientation::Horizontal, 0x01, false),
    )?;
    std::fs::write(source_path.with_extension("sav"), [0x55; 31])?;
    assert!(
        DirectWsTasExecutionLoader::new(source_path)
            .create_project()
            .is_err()
    );

    let directory = crate::test_support::test_directory("tas-direct-ws-default-save")?;
    let source_path = directory.path().join("game.wsc");
    std::fs::write(
        &source_path,
        ws_rom(1, RomOrientation::Vertical, 0x10, false),
    )?;
    let project = DirectWsTasExecutionLoader::new(source_path).create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&[0xFF; 128]))
    );
    Ok(())
}

#[test]
fn opening_refuses_changed_media_and_invalid_checksum() -> Result<()> {
    let (directory, loader, mut rom) =
        loader("tas-direct-ws-identity", "wsc", RomOrientation::Vertical)?;
    let project = loader.create_project()?;
    rom[0] ^= 1;
    std::fs::write(&loader.source_path, &rom)?;
    assert!(loader.load_session(project.start_state()).is_err());

    let invalid = directory.path().join("invalid.ws");
    let mut invalid_rom = ws_rom(0, RomOrientation::Horizontal, 0, false);
    invalid_rom[5] ^= 1;
    std::fs::write(&invalid, invalid_rom)?;
    assert!(
        DirectWsTasExecutionLoader::new(invalid)
            .create_project()
            .is_err()
    );
    Ok(())
}

#[test]
fn branch_scope_rejects_unowned_input_and_events() -> Result<()> {
    let (_directory, loader, _) = loader("tas-direct-ws-scope", "ws", RomOrientation::Horizontal)?;
    let mut extra_input = loader.create_project()?;
    let mut input = keypad_input();
    input.players[1].buttons = 1;
    extra_input.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert!(
        DirectWsTasExecutionLoader::validate_project_branch_scope(&extra_input, "main").is_err()
    );

    let mut event = loader.create_project()?;
    event.edit_transaction(|edit| {
        edit.replace_branch_events(
            "main",
            vec![zeff_emu_common::replay::ReplayEvent::FdsDiskSide { frame: 0, side: 0 }],
        )
    })?;
    assert!(DirectWsTasExecutionLoader::validate_project_branch_scope(&event, "main").is_err());
    Ok(())
}

#[test]
fn replay_export_and_import_preserve_direct_ws_keypad_input() -> Result<()> {
    let (directory, loader, _) = loader("tas-direct-ws-replay", "wsc", RomOrientation::Vertical)?;
    let mut project = loader.create_project()?;
    let input = keypad_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let manual_path = directory.path().join("source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("replay-cache"))?;
    let mut editor = TasEditorSession::new(project, manual_path, autosaves, cache)?;
    let plan = super::super::PrivateTasExecutionLoader::DirectWs(loader.clone());
    let replay_path = directory.path().join("movie.zrpl");
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;
    let imported_path = directory.path().join("imported.ztas");
    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.branch("main").unwrap().input_at(0), input);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&imported)?,
        crate::emu_thread::TasExecutionProfile::DirectWsCartridge
    );
    Ok(())
}

#[test]
fn selected_zip_member_reopens_and_binds_archive_and_save() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-ws-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = ws_rom(0, RomOrientation::Horizontal, 0, false);
    let selected = ws_rom(1, RomOrientation::Vertical, 0x10, false);
    let archive = write_zip(
        &archive_path,
        &[("first.ws", &first), ("folder/selected.wsc", &selected)],
    )?;
    let save = vec![0x5A; 128];
    std::fs::write(archive_path.with_extension("sav"), &save)?;
    let loader = DirectWsTasExecutionLoader::new_zip(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.wsc")),
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        TasDigest::from_bytes(&archive)
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        TasDigest::from_bytes(&selected)
    );
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&save))
    );
    let reopened = DirectWsTasExecutionLoader::new_zip_for_project(archive_path.clone(), &project)?;
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    write_zip(
        &archive_path,
        &[
            ("first.ws", &first),
            ("folder/selected.wsc", &selected),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(DirectWsTasExecutionLoader::new_zip_for_project(archive_path, &project).is_err());
    Ok(())
}

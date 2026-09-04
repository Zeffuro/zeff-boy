use anyhow::Result;
use zeff_emu_common::replay::{ReplayColecoControllerFrame, ReplayPlayer};

use super::direct_coleco::direct_coleco_tas_sync_config_sha256;
use super::direct_coleco_loader::DirectColecoTasExecutionLoader;
use super::{
    PrivateTasExecutionLoader, classify_direct_tas_execution_profile,
    select_private_tas_execution_loader,
};
use crate::emu_backend::ActiveSystem;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasColecoControllerInput, TasColecoKeypadKey,
    TasEditorSession, TasExecutionWitness, TasInputFrame, TasSeekStateCache,
};
use crate::test_support::write_zip;

static TEST_BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
    [0; zeff_coleco_core::constants::BIOS_SIZE];
static OTHER_TEST_BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
    [1; zeff_coleco_core::constants::BIOS_SIZE];

fn write_direct_cartridge(
    label: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    std::path::PathBuf,
    Vec<u8>,
)> {
    let directory = crate::test_support::test_directory(label)?;
    let path = directory.path().join("game.col");
    let mut bytes = vec![0; 8 * 1024];
    bytes[..2].copy_from_slice(&[0xAA, 0x55]);
    std::fs::write(&path, &bytes)?;
    Ok((directory, path, bytes))
}

#[test]
fn creates_reopens_and_executes_two_semantic_coleco_controllers() -> Result<()> {
    let (directory, source_path, source_bytes) = write_direct_cartridge("tas-direct-coleco-flow")?;
    let loader =
        DirectColecoTasExecutionLoader::new_with_bios_override(source_path, Vec::new(), &TEST_BIOS);
    let project_path = directory.path().join("movie.ztas");

    let created = loader.create_project_file(&project_path)?;
    let mut reopened = crate::tas_project::TasProject::load(&project_path)?;
    assert_eq!(created, reopened);
    assert_eq!(
        reopened.project_id(),
        format!(
            "coleco-{}",
            crate::tas_project::TasDigest::from_bytes(&source_bytes).to_hex()
        )
    );
    assert_eq!(reopened.identity().devices.len(), 2);
    assert_eq!(
        reopened.identity().sync_config_sha256,
        direct_coleco_tas_sync_config_sha256()
    );
    assert_eq!(
        classify_direct_tas_execution_profile(&reopened)?,
        crate::emu_thread::TasExecutionProfile::DirectColecoCartridge
    );

    let controllers = [
        TasColecoControllerInput {
            left: true,
            left_button: true,
            keypad: TasColecoKeypadKey::Star,
            ..TasColecoControllerInput::default()
        },
        TasColecoControllerInput {
            right: true,
            right_button: true,
            keypad: TasColecoKeypadKey::Nine,
            ..TasColecoControllerInput::default()
        },
    ];
    reopened.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                coleco: controllers,
                ..TasInputFrame::default()
            },
        )
    })?;
    let mut engine = loader.load_editor_engine(&reopened)?;
    let manual_path = directory.path().join("manual.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
    let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(reopened, manual_path, autosaves, seek_cache)?;
    let outcome = engine.seek(&mut editor, 1)?;

    assert!(outcome.reached_target());
    assert_eq!(outcome.cursor, 1);
    let backend = engine.backend().coleco().expect("Coleco backend");
    assert_eq!(
        backend.emu.controller_ports().player(0),
        Some(controllers[0].into())
    );
    assert_eq!(
        backend.emu.controller_ports().player(1),
        Some(controllers[1].into())
    );
    Ok(())
}

#[test]
fn profile_selector_only_accepts_direct_col_media() -> Result<()> {
    let (directory, source_path, _) = write_direct_cartridge("tas-direct-coleco-selector")?;
    assert!(matches!(
        select_private_tas_execution_loader(source_path, ActiveSystem::Coleco, Vec::new())?,
        PrivateTasExecutionLoader::DirectColeco(_)
    ));

    let wrong_path = directory.path().join("game.zip");
    std::fs::write(&wrong_path, [0xAA, 0x55])?;
    assert!(
        select_private_tas_execution_loader(wrong_path, ActiveSystem::Coleco, Vec::new()).is_err()
    );
    Ok(())
}

#[test]
fn opening_refuses_changed_media_or_bios_without_touching_the_project() -> Result<()> {
    let (directory, source_path, source_bytes) =
        write_direct_cartridge("tas-direct-coleco-identity")?;
    let loader = DirectColecoTasExecutionLoader::new_with_bios_override(
        source_path.clone(),
        Vec::new(),
        &TEST_BIOS,
    );
    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    let before = std::fs::read(&project_path)?;

    let other_bios = DirectColecoTasExecutionLoader::new_with_bios_override(
        source_path.clone(),
        Vec::new(),
        &OTHER_TEST_BIOS,
    );
    assert!(other_bios.load_session(project.start_state()).is_err());
    assert_eq!(std::fs::read(&project_path)?, before);

    let mut changed_source = source_bytes;
    changed_source[2] ^= 1;
    std::fs::write(&source_path, changed_source)?;
    assert!(loader.load_session(project.start_state()).is_err());
    assert_eq!(std::fs::read(&project_path)?, before);
    Ok(())
}

#[test]
fn verifies_exports_imports_and_loads_semantic_coleco_zrpl() -> Result<()> {
    let (directory, source_path, _) = write_direct_cartridge("tas-direct-coleco-verify")?;
    let loader =
        DirectColecoTasExecutionLoader::new_with_bios_override(source_path, Vec::new(), &TEST_BIOS);
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                coleco: [
                    TasColecoControllerInput {
                        keypad: TasColecoKeypadKey::One,
                        ..TasColecoControllerInput::default()
                    },
                    TasColecoControllerInput {
                        right_button: true,
                        keypad: TasColecoKeypadKey::Pound,
                        ..TasColecoControllerInput::default()
                    },
                ],
                ..TasInputFrame::default()
            },
        )
    })?;
    let start_state = project.start_state().to_vec();
    let witness = TasExecutionWitness {
        identity: loader.load_session(&start_state)?.identity().clone(),
    };

    let replay_path = directory.path().join("verified.zrpl");
    project.verify_and_export_zrpl_with_factory("main", &replay_path, &witness, || {
        loader.load_session(&start_state)
    })?;

    assert!(project.verification_is_current("main")?);
    let mut player = ReplayPlayer::load(&replay_path)?;
    assert_eq!(
        u32::from_le_bytes(std::fs::read(&replay_path)?[4..8].try_into()?),
        3
    );
    assert_eq!(
        player
            .next_joypad_frame()
            .expect("one Coleco replay frame")
            .coleco,
        [
            ReplayColecoControllerFrame {
                keypad: 2,
                ..ReplayColecoControllerFrame::default()
            },
            ReplayColecoControllerFrame {
                right_button: true,
                keypad: 12,
                ..ReplayColecoControllerFrame::default()
            },
        ]
    );
    let imported_path = directory.path().join("imported.ztas");
    let imported = PrivateTasExecutionLoader::DirectColeco(loader.clone()).import_replay_file(
        &replay_path,
        &imported_path,
        false,
    )?;
    loader.load_editor_engine(&imported)?;
    Ok(())
}

#[test]
fn selected_zip_member_reopens_and_rejects_archive_changes() -> Result<()> {
    let directory = crate::test_support::test_directory("tas-coleco-zip")?;
    let archive_path = directory.path().join("games.zip");
    let mut first = vec![0; 8 * 1024];
    first[..2].copy_from_slice(&[0xAA, 0x55]);
    let mut selected = first.clone();
    selected[2] = 1;
    let archive = write_zip(
        &archive_path,
        &[("first.col", &first), ("folder/selected.col", &selected)],
    )?;
    let loader = DirectColecoTasExecutionLoader::new_zip_with_bios_override(
        archive_path.clone(),
        Some(archive_path.join("folder/selected.col")),
        &TEST_BIOS,
    );
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256,
        crate::tas_project::TasDigest::from_bytes(&archive)
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        crate::tas_project::TasDigest::from_bytes(&selected)
    );
    let reopened = DirectColecoTasExecutionLoader::new_zip_for_project(
        archive_path.clone(),
        Vec::new(),
        &project,
    )?;
    let reopened = DirectColecoTasExecutionLoader::new_zip_with_bios_override(
        reopened.source_path,
        reopened.rom_path,
        &TEST_BIOS,
    );
    assert_eq!(
        reopened.load_session(project.start_state())?.identity(),
        project.identity()
    );
    write_zip(
        &archive_path,
        &[
            ("first.col", &first),
            ("folder/selected.col", &selected),
            ("note.txt", b"changed"),
        ],
    )?;
    assert!(
        DirectColecoTasExecutionLoader::new_zip_for_project(archive_path, Vec::new(), &project,)
            .is_err()
    );
    Ok(())
}

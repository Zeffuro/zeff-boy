use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::Result;
use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};

use super::*;
use crate::emu_backend::loader::tas::direct_pce_cd::{
    direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256,
};

#[derive(Clone, Copy)]
enum ArchiveKind {
    SevenZip,
    Rar,
    Zip,
}

impl ArchiveKind {
    fn extension(self) -> &'static str {
        match self {
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Zip => "zip",
        }
    }

    fn multitap_sync(self, selected: bool) -> TasDigest {
        match (self, selected) {
            (Self::SevenZip, false) => direct_pce_multitap_cd_archive_tas_sync_config_sha256(),
            (Self::SevenZip, true) => {
                direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256()
            }
            (Self::Rar, false) => direct_pce_multitap_cd_rar_tas_sync_config_sha256(),
            (Self::Rar, true) => direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256(),
            (Self::Zip, false) => direct_pce_multitap_cd_zip_tas_sync_config_sha256(),
            (Self::Zip, true) => direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256(),
        }
    }

    fn selected_card_sync(self, card: CardKind) -> TasDigest {
        match (self, card) {
            (Self::SevenZip, CardKind::Arcade) => {
                direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256()
            }
            (Self::SevenZip, CardKind::MemoryBase) => {
                direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256()
            }
            (Self::Rar, CardKind::Arcade) => {
                direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256()
            }
            (Self::Rar, CardKind::MemoryBase) => {
                direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256()
            }
            (Self::Zip, CardKind::Arcade) => {
                direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256()
            }
            (Self::Zip, CardKind::MemoryBase) => {
                direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256()
            }
            (_, CardKind::None) => unreachable!("selected card profile required"),
        }
    }
}

#[test]
fn selected_card_replay_auto_import_reopens_and_seeks_all_six_routes() -> Result<()> {
    let mut fill = 0xA1;
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for card in [CardKind::Arcade, CardKind::MemoryBase] {
            run_selected_card_replay(kind, card, fill)?;
            fill += 1;
        }
    }
    Ok(())
}

fn run_selected_card_replay(kind: ArchiveKind, card: CardKind, fill: u8) -> Result<()> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-zrpl-selected-{}-{card:?}",
        kind.extension()
    ))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    write_multicue_archive_with_fill(&archive, kind, fill)?;
    let rom_path = archive.join("second").join("disc.cue");
    let system_card = system_card();
    let loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
        archive.clone(),
        rom_path.clone(),
        system_card,
        SYSTEM_CARD_SHA256,
    )?;
    let disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("selected fixture disc");
    let (_arcade_catalog, _memory_base_catalog) = match card {
        CardKind::Arcade => (
            Some(
                crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                    disc_sha256,
                ),
            ),
            None,
        ),
        CardKind::MemoryBase => (
            None,
            Some(
                crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                    disc_sha256,
                ),
            ),
        ),
        CardKind::None => unreachable!("selected card profile required"),
    };
    let project_path = directory.path().join("source.ztas");
    let replay_path = directory.path().join("verified.zrpl");
    let imported_path = directory.path().join("imported.ztas");
    let mut project = loader.create_project()?;
    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert_eq!(
        project.identity().sync_config_sha256,
        kind.selected_card_sync(card)
    );

    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader);
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let _system_card =
        super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, system_card);
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected_loader = super::super::select_private_tas_execution_loader_for_replay(
        archive.clone(),
        Some(rom_path),
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected_loader.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.identity(), project.identity());
    assert_eq!(imported.branch("main").expect("main").input_at(0), input);
    let reopened = super::super::select_private_tas_execution_loader_for_project(
        archive,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &imported,
    )?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&imported_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("imported-seek-cache"))?;
    let mut imported_editor = TasEditorSession::open(&imported_path, autosaves, cache)?;
    let mut engine = reopened.load_editor_engine(imported_editor.project())?;
    assert!(engine.seek(&mut imported_editor, 1)?.reached_target());
    Ok(())
}

#[test]
fn archive_multitap_replay_auto_import_reopens_and_seeks_all_six_routes() -> Result<()> {
    for (index, kind) in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip]
        .into_iter()
        .enumerate()
    {
        run_multitap_replay(kind, false, 0xC1 + index as u8)?;
        run_multitap_replay(kind, true, 0xD1 + index as u8)?;
    }
    Ok(())
}

fn run_multitap_replay(kind: ArchiveKind, selected: bool, fill: u8) -> Result<()> {
    let directory = crate::test_support::test_directory(&format!(
        "pce-cd-zrpl-archive-multitap-{}-{selected}",
        kind.extension()
    ))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    if selected {
        write_multicue_archive_with_fill(&archive, kind, fill)?;
    } else {
        write_unique_archive(&archive, kind, fill)?;
    }
    let rom_path = selected.then(|| archive.join("second").join("disc.cue"));
    let system_card = system_card();
    let base = if let Some(rom_path) = rom_path.clone() {
        DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
            archive.clone(),
            rom_path,
            system_card,
            SYSTEM_CARD_SHA256,
        )?
    } else {
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive.clone(),
            system_card,
            SYSTEM_CARD_SHA256,
        )
    };
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let loader = if let Some(rom_path) = rom_path.clone() {
        DirectPceCdTasExecutionLoader::new_multitap_with_rom_path_and_system_card_override(
            archive.clone(),
            rom_path,
            system_card,
            SYSTEM_CARD_SHA256,
        )?
    } else {
        DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
            archive.clone(),
            system_card,
            SYSTEM_CARD_SHA256,
        )
    };
    let project_path = directory.path().join("source.ztas");
    let replay_path = directory.path().join("verified.zrpl");
    let imported_path = directory.path().join("imported.ztas");
    let mut project = loader.create_project()?;
    let mut input = TasInputFrame::default();
    input.players[0].dpad = 0x02;
    input.players[4].buttons = 0x01;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    assert_eq!(
        project.identity().sync_config_sha256,
        kind.multitap_sync(selected)
    );

    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader);
    plan.verify_and_export_editor_session(&mut editor, &replay_path)?;

    let _system_card =
        super::super::register_test_pce_cd_system_card(SYSTEM_CARD_SHA256, system_card);
    let start_state = TasProject::read_zrpl_start_state(&replay_path)?;
    let selected_loader = super::super::select_private_tas_execution_loader_for_replay(
        archive.clone(),
        rom_path,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &start_state,
    )?;
    let imported = selected_loader.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(imported.identity(), project.identity());
    assert_eq!(imported.branch("main").expect("main").input_at(0), input);
    let reopened = super::super::select_private_tas_execution_loader_for_project(
        archive,
        crate::emu_backend::ActiveSystem::Pce,
        Vec::new(),
        &imported,
    )?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&imported_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("imported-seek-cache"))?;
    let mut imported_editor = TasEditorSession::open(&imported_path, autosaves, cache)?;
    let mut engine = reopened.load_editor_engine(imported_editor.project())?;
    assert!(engine.seek(&mut imported_editor, 1)?.reached_target());
    Ok(())
}

#[test]
fn selected_second_cue_replay_roundtrip_reopens_and_seeks_for_archives() -> Result<()> {
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        let directory = crate::test_support::test_directory(&format!(
            "pce-cd-zrpl-selected-multicue-{}",
            kind.extension()
        ))?;
        let archive = directory.path().join(format!("disc.{}", kind.extension()));
        write_multicue_archive(&archive, kind)?;
        let system_card = system_card();
        let loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
            archive.clone(),
            archive.join("second").join("disc.cue"),
            system_card,
            SYSTEM_CARD_SHA256,
        )?;
        let project_path = directory.path().join("source.ztas");
        let replay_path = directory.path().join("verified.zrpl");
        let imported_path = directory.path().join("imported.ztas");
        let mut project = loader.create_project()?;
        project.edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                0,
                1,
                TasInputFrame {
                    players: [
                        TasControllerInput {
                            buttons: 0x03,
                            dpad: 0x04,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            )
        })?;

        let autosaves =
            TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
        let cache = TasSeekStateCache::open(directory.path().join("source-seek-cache"))?;
        let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
        let plan = PrivateTasExecutionLoader::DirectPceCd(loader.clone());
        assert_eq!(
            plan.verify_and_export_editor_session(&mut editor, &replay_path)?,
            replay_path
        );

        let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
        assert_eq!(imported.identity(), project.identity());
        assert!(imported.verification_is_current("main")?);
        let autosaves =
            TasAutosaveStore::beside_manual_save(&imported_path, TasAutosaveConfig::default())?;
        let cache = TasSeekStateCache::open(directory.path().join("imported-seek-cache"))?;
        let mut imported_editor = TasEditorSession::open(&imported_path, autosaves, cache)?;
        let mut engine = plan.load_editor_engine(imported_editor.project())?;
        assert!(engine.seek(&mut imported_editor, 1)?.reached_target());
    }
    Ok(())
}

fn write_multicue_archive(path: &Path, kind: ArchiveKind) -> Result<()> {
    write_multicue_archive_with_fill(path, kind, 0x22)
}

fn write_multicue_archive_with_fill(path: &Path, kind: ArchiveKind, fill: u8) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let first = vec![0x11; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    let mut second = vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    second[0..4].copy_from_slice(&[0x52, 0x4D, fill, fill.rotate_left(1)]);
    let entries = [
        ("first/disc.cue", cue.as_slice()),
        ("first/disc.bin", first.as_slice()),
        ("second/disc.cue", cue.as_slice()),
        ("second/disc.bin", second.as_slice()),
    ];
    match kind {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer.push_archive_entry(
                    ArchiveEntry::new_file(name),
                    Some(Cursor::new(bytes.to_vec())),
                )?;
            }
            writer.finish()?;
        }
        ArchiveKind::Rar => {
            let entries = entries
                .into_iter()
                .map(|(name, bytes)| {
                    RarArchiveEntry::new(
                        name.as_bytes().to_vec(),
                        EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(bytes.to_vec())),
                    )
                })
                .collect::<Vec<_>>();
            let bytes = Rar50Writer::new(
                WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
                    .with_compression_level(0),
            )
            .entries(entries)
            .finish()?;
            fs::write(path, bytes)?;
        }
        ArchiveKind::Zip => {
            let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
            for (name, bytes) in entries {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}

fn write_unique_archive(path: &Path, kind: ArchiveKind, fill: u8) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let mut disc = vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    disc[0..4].copy_from_slice(&[0x52, 0x55, fill, fill.rotate_left(1)]);
    let entries = [
        ("set/disc.cue", cue.as_slice()),
        ("set/disc.bin", disc.as_slice()),
    ];
    match kind {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer.push_archive_entry(
                    ArchiveEntry::new_file(name),
                    Some(Cursor::new(bytes.to_vec())),
                )?;
            }
            writer.finish()?;
        }
        ArchiveKind::Rar => {
            let entries = entries
                .into_iter()
                .map(|(name, bytes)| {
                    RarArchiveEntry::new(
                        name.as_bytes().to_vec(),
                        EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(bytes.to_vec())),
                    )
                })
                .collect::<Vec<_>>();
            let bytes = Rar50Writer::new(
                WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
                    .with_compression_level(0),
            )
            .entries(entries)
            .finish()?;
            fs::write(path, bytes)?;
        }
        ArchiveKind::Zip => {
            let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
            for (name, bytes) in entries {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}

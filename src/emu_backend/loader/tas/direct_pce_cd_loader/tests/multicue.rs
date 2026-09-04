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

#[derive(Clone, Copy, Debug)]
pub(super) enum ArchiveKind {
    SevenZip,
    Rar,
    Zip,
}

impl ArchiveKind {
    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Zip => "zip",
        }
    }

    pub(super) fn selected_sync(self) -> TasDigest {
        match self {
            Self::SevenZip => direct_pce_cd_selected_archive_tas_sync_config_sha256(),
            Self::Rar => direct_pce_cd_selected_rar_tas_sync_config_sha256(),
            Self::Zip => direct_pce_cd_selected_zip_tas_sync_config_sha256(),
        }
    }

    pub(super) fn unique_sync(self) -> TasDigest {
        match self {
            Self::SevenZip => direct_pce_cd_archive_tas_sync_config_sha256(),
            Self::Rar => direct_pce_cd_rar_tas_sync_config_sha256(),
            Self::Zip => direct_pce_cd_zip_tas_sync_config_sha256(),
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
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CardKind {
    Arcade,
    MemoryBase,
}

#[test]
fn explicit_second_cue_projects_reopen_by_identity_and_preserve_unique_profiles() -> Result<()> {
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        exercise_explicit_second_cue(kind)?;
    }
    Ok(())
}

#[test]
fn selected_second_cue_card_profiles_create_reopen_reject_wrong_member_and_seek() -> Result<()> {
    let mut fill = 0xB1;
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for card in [CardKind::Arcade, CardKind::MemoryBase] {
            exercise_selected_card_profile(kind, card, fill)?;
            fill += 1;
        }
    }
    Ok(())
}

fn exercise_selected_card_profile(kind: ArchiveKind, card: CardKind, fill: u8) -> Result<()> {
    let directory =
        crate::test_support::test_directory(&format!("pce-cd-tas-{kind:?}-multicue-{card:?}"))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    write_multicue_archive_with_second_fill(&archive, kind, fill)?;
    let second = archive.join("second").join("disc.cue");
    let first = archive.join("first").join("disc.cue");

    let base = configured_loader(DirectPceCdTasExecutionLoader::new_with_rom_path(
        archive.clone(),
        Some(second.clone()),
        Vec::new(),
    )?);
    let disc_sha256 = base
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
    };

    let mut project = base.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        kind.selected_card_sync(card)
    );
    let backend = base.load_fresh_backend()?;
    let pce = backend.pce().expect("selected fixture must remain PCE-CD");
    assert_eq!(
        pce.arcade_card_mode(),
        if matches!(card, CardKind::Arcade) {
            zeff_pce_core::hardware::PceArcadeCardMode::Enabled
        } else {
            zeff_pce_core::hardware::PceArcadeCardMode::Disabled
        }
    );
    assert_eq!(
        pce.memory_base_mode(),
        if matches!(card, CardKind::MemoryBase) {
            zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
        } else {
            zeff_pce_core::hardware::PceMemoryBaseMode::Disabled
        }
    );

    let recovered = configured_loader(DirectPceCdTasExecutionLoader::new_for_project(
        archive.clone(),
        Vec::new(),
        &project,
    )?);
    assert_eq!(
        recovered.archive_cue_member.as_deref(),
        Some("second/disc.cue")
    );
    recovered.load_editor_engine(&project)?;

    let wrong = configured_loader(DirectPceCdTasExecutionLoader::new_with_rom_path(
        archive,
        Some(first),
        Vec::new(),
    )?);
    assert!(wrong.load_editor_engine(&project).is_err());

    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let project_path = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, project_path, autosaves, cache)?;
    let mut engine = recovered.load_editor_engine(editor.project())?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let mut expected = recovered.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    Ok(())
}

fn exercise_explicit_second_cue(kind: ArchiveKind) -> Result<()> {
    let directory = crate::test_support::test_directory(&format!("pce-cd-tas-{kind:?}-multicue"))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    write_multicue_archive(&archive, kind)?;
    let second = archive.join("second").join("disc.cue");
    let first = archive.join("first").join("disc.cue");

    let selected = configured_loader(DirectPceCdTasExecutionLoader::new_with_rom_path(
        archive.clone(),
        Some(second),
        Vec::new(),
    )?);
    let project = selected.create_project()?;
    assert_eq!(project.identity().sync_config_sha256, kind.selected_sync());
    assert_ne!(project.identity().sync_config_sha256, kind.unique_sync());
    assert!(selected.load_editor_engine(&project).is_ok());

    let recovered = configured_loader(DirectPceCdTasExecutionLoader::new_for_project(
        archive.clone(),
        Vec::new(),
        &project,
    )?);
    assert_eq!(
        recovered.archive_cue_member.as_deref(),
        Some("second/disc.cue")
    );
    assert!(recovered.load_editor_engine(&project).is_ok());

    let wrong = configured_loader(DirectPceCdTasExecutionLoader::new_with_rom_path(
        archive,
        Some(first),
        Vec::new(),
    )?);
    assert!(wrong.load_editor_engine(&project).is_err());

    let unique = directory
        .path()
        .join(format!("unique.{}", kind.extension()));
    match kind {
        ArchiveKind::SevenZip => write_archive_fixture(&unique, 0x31)?,
        ArchiveKind::Rar => write_rar_fixture(&unique, "set", 0x31)?,
        ArchiveKind::Zip => write_zip_fixture(&unique, "set", 0x31)?,
    }
    let unique_project = configured_loader(DirectPceCdTasExecutionLoader::new_with_rom_path(
        unique,
        None,
        Vec::new(),
    )?)
    .create_project()?;
    assert_eq!(
        unique_project.identity().sync_config_sha256,
        kind.unique_sync()
    );
    Ok(())
}

fn configured_loader(mut loader: DirectPceCdTasExecutionLoader) -> DirectPceCdTasExecutionLoader {
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    loader.system_card_override = Some(system_card);
    loader.system_card_sha256_override = Some(TEST_SYSTEM_CARD_SHA256);
    loader
}

pub(super) fn write_multicue_archive(path: &Path, kind: ArchiveKind) -> Result<()> {
    write_multicue_archive_with_second_fill(path, kind, 0x22)
}

pub(super) fn write_multicue_archive_with_second_fill(
    path: &Path,
    kind: ArchiveKind,
    second_fill: u8,
) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let first = vec![0x11; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    let mut second = vec![second_fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    second[0..4].copy_from_slice(&[0x4D, 0x54, second_fill, second_fill.rotate_left(1)]);
    match kind {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in [
                ("first/disc.cue", cue.as_slice()),
                ("first/disc.bin", first.as_slice()),
                ("second/disc.cue", cue.as_slice()),
                ("second/disc.bin", second.as_slice()),
            ] {
                writer.push_archive_entry(
                    ArchiveEntry::new_file(name),
                    Some(Cursor::new(bytes.to_vec())),
                )?;
            }
            writer.finish()?;
        }
        ArchiveKind::Rar => {
            let entries = [
                ("first/disc.cue", cue.as_slice()),
                ("first/disc.bin", first.as_slice()),
                ("second/disc.cue", cue.as_slice()),
                ("second/disc.bin", second.as_slice()),
            ]
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
            for (name, bytes) in [
                ("first/disc.cue", cue.as_slice()),
                ("first/disc.bin", first.as_slice()),
                ("second/disc.cue", cue.as_slice()),
                ("second/disc.bin", second.as_slice()),
            ] {
                writer.start_file(name, zip::write::SimpleFileOptions::default())?;
                writer.write_all(bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}

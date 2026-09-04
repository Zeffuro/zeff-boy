use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};
use zeff_pce_core::hardware::{PceArcadeCardMode, PceMemoryBaseMode};

use super::{
    DirectPceCdTasExecutionLoader, PrivateTasExecutionLoader, TasEditorExecutionProvider,
    direct_pce_cd::{
        direct_pce_cd_arcade_tas_sync_config_sha256,
        direct_pce_cd_archive_arcade_tas_sync_config_sha256,
        direct_pce_cd_archive_memory_base_tas_sync_config_sha256,
        direct_pce_cd_archive_ppf_tas_sync_config_sha256,
        direct_pce_cd_archive_tas_sync_config_sha256,
        direct_pce_cd_chd_arcade_tas_sync_config_sha256,
        direct_pce_cd_chd_memory_base_tas_sync_config_sha256,
        direct_pce_cd_chd_tas_sync_config_sha256, direct_pce_cd_iso_arcade_tas_sync_config_sha256,
        direct_pce_cd_iso_memory_base_tas_sync_config_sha256,
        direct_pce_cd_iso_tas_sync_config_sha256, direct_pce_cd_memory_base_tas_sync_config_sha256,
        direct_pce_cd_ppf_arcade_tas_sync_config_sha256,
        direct_pce_cd_ppf_memory_base_tas_sync_config_sha256,
        direct_pce_cd_ppf_tas_sync_config_sha256, direct_pce_cd_rar_arcade_tas_sync_config_sha256,
        direct_pce_cd_rar_memory_base_tas_sync_config_sha256,
        direct_pce_cd_rar_ppf_tas_sync_config_sha256, direct_pce_cd_rar_tas_sync_config_sha256,
        direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256,
        direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256,
        direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256,
        direct_pce_cd_tas_sync_config_sha256, direct_pce_cd_zip_arcade_tas_sync_config_sha256,
        direct_pce_cd_zip_memory_base_tas_sync_config_sha256,
        direct_pce_cd_zip_ppf_tas_sync_config_sha256, direct_pce_cd_zip_tas_sync_config_sha256,
        direct_pce_multitap_cd_archive_tas_sync_config_sha256,
        direct_pce_multitap_cd_rar_tas_sync_config_sha256,
        direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256,
        direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256,
        direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256,
        direct_pce_multitap_cd_zip_tas_sync_config_sha256,
    },
};
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasDigest, TasEditorSession,
    TasExternalIdentity, TasInitialBranch, TasInputFrame, TasProject, TasProjectIdentity,
    TasSeekStateCache,
};

mod archive_ppf;
mod multicue;

const SYSTEM_CARD_SHA256: [u8; 32] = [
    0x8A, 0x39, 0xD2, 0xAB, 0xD3, 0x99, 0x9A, 0xB7, 0x3C, 0x34, 0xDB, 0x24, 0x76, 0x84, 0x9C, 0xDD,
    0xF3, 0x03, 0xCE, 0x38, 0x9B, 0x35, 0x82, 0x68, 0x50, 0xF9, 0xA7, 0x00, 0x58, 0x9B, 0x4A, 0x90,
];

#[derive(Clone, Copy, Debug)]
enum MediaKind {
    Cue,
    Chd,
    Iso,
    Ppf,
    Archive,
    Rar,
    Zip,
    ArchivePpf,
    SelectedArchivePpf,
    RarPpf,
    SelectedRarPpf,
    ZipPpf,
    SelectedZipPpf,
}

#[derive(Clone, Copy, Debug)]
enum CardKind {
    None,
    Arcade,
    MemoryBase,
}

fn system_card() -> &'static [u8] {
    Box::leak(vec![0; 256 * 1024].into_boxed_slice())
}

fn fixture(
    name: &str,
    media: MediaKind,
    seed: u8,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    PathBuf,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let source_path = match media {
        MediaKind::Cue => {
            let bin_path = directory.path().join("disc.bin");
            let cue_path = directory.path().join("disc.cue");
            fs::write(&bin_path, deterministic_disc_bytes(seed))?;
            write_cue(&cue_path, "disc.bin")?;
            cue_path
        }
        MediaKind::Chd => {
            let chd_path = directory.path().join("disc.chd");
            crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&chd_path)?;
            let mut bytes = fs::read(&chd_path)?;
            bytes[4 * 2_448] ^= seed;
            fs::write(&chd_path, bytes)?;
            chd_path
        }
        MediaKind::Iso => {
            let iso_path = directory.path().join("disc.iso");
            fs::write(&iso_path, deterministic_disc_bytes(seed))?;
            write_cue(&directory.path().join("disc.cue"), "disc.iso")?;
            iso_path
        }
        MediaKind::Ppf
        | MediaKind::Archive
        | MediaKind::Rar
        | MediaKind::Zip
        | MediaKind::ArchivePpf
        | MediaKind::SelectedArchivePpf
        | MediaKind::RarPpf
        | MediaKind::SelectedRarPpf
        | MediaKind::ZipPpf
        | MediaKind::SelectedZipPpf => {
            unreachable!("special fixture required")
        }
    };
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card(),
        SYSTEM_CARD_SHA256,
    );
    Ok((directory, loader, source_path))
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

fn ppf_fixture(
    name: &str,
    seed: u8,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    [u8; 32],
)> {
    let (directory, base_loader, cue_path) = fixture(name, MediaKind::Cue, seed)?;
    let source_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .expect("PPF fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("PPF fixture must mount a normalized disc");
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), ppf1(0, &[seed ^ 0xA5])),
            ("second.ppf".to_owned(), ppf1(1, &[seed ^ 0x5A])),
        ],
    )?;
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        system_card(),
        SYSTEM_CARD_SHA256,
        stack,
    );
    Ok((directory, loader, source_disc_sha256))
}

fn write_archive_fixture(path: &Path, fill: u8) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let mut writer = ArchiveWriter::create(path)?;
    writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
    writer.push_archive_entry(
        ArchiveEntry::new_file("set/disc.cue"),
        Some(Cursor::new(cue.to_vec())),
    )?;
    writer.push_archive_entry(
        ArchiveEntry::new_file("set/disc.bin"),
        Some(Cursor::new(
            vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        )),
    )?;
    writer.finish()?;
    Ok(())
}

fn archive_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    PathBuf,
)> {
    archive_fixture_with_fill(name, 0x51)
}

fn archive_fixture_with_fill(
    name: &str,
    fill: u8,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    PathBuf,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let archive_path = directory.path().join("disc.7z");
    write_archive_fixture(&archive_path, fill)?;
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        archive_path.clone(),
        system_card(),
        SYSTEM_CARD_SHA256,
    );
    Ok((directory, loader, archive_path))
}

fn write_rar_fixture(path: &Path, fill: u8) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let entries = [
        ("set/disc.cue".as_bytes().to_vec(), cue.to_vec()),
        (
            "set/disc.bin".as_bytes().to_vec(),
            vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        ),
    ]
    .into_iter()
    .map(|(name, data)| {
        RarArchiveEntry::new(
            name,
            EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(data)),
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
    Ok(())
}

fn rar_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    PathBuf,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let archive_path = directory.path().join("disc.rar");
    write_rar_fixture(&archive_path, 0x59)?;
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        archive_path.clone(),
        system_card(),
        SYSTEM_CARD_SHA256,
    );
    Ok((directory, loader, archive_path))
}

fn write_zip_fixture(path: &Path, fill: u8) -> Result<()> {
    let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("set/disc.cue", options)?;
    writer.write_all(b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n")?;
    writer.start_file("set/disc.bin", options)?;
    writer.write_all(&vec![
        fill;
        4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES
    ])?;
    writer.finish()?;
    Ok(())
}

fn deterministic_disc_bytes(seed: u8) -> Vec<u8> {
    (0..4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES)
        .map(|index| (index as u8).wrapping_add(seed))
        .collect()
}

fn write_cue(path: &Path, referenced_file: &str) -> Result<()> {
    fs::write(
        path,
        format!("FILE \"{referenced_file}\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n"),
    )?;
    Ok(())
}

fn expected_sync_config(media: MediaKind, card: CardKind) -> TasDigest {
    match (media, card) {
        (MediaKind::Cue, CardKind::None) => direct_pce_cd_tas_sync_config_sha256(),
        (MediaKind::Cue, CardKind::Arcade) => direct_pce_cd_arcade_tas_sync_config_sha256(),
        (MediaKind::Cue, CardKind::MemoryBase) => {
            direct_pce_cd_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::Chd, CardKind::None) => direct_pce_cd_chd_tas_sync_config_sha256(),
        (MediaKind::Chd, CardKind::Arcade) => direct_pce_cd_chd_arcade_tas_sync_config_sha256(),
        (MediaKind::Chd, CardKind::MemoryBase) => {
            direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::Iso, CardKind::None) => direct_pce_cd_iso_tas_sync_config_sha256(),
        (MediaKind::Iso, CardKind::Arcade) => direct_pce_cd_iso_arcade_tas_sync_config_sha256(),
        (MediaKind::Iso, CardKind::MemoryBase) => {
            direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::Ppf, CardKind::None) => direct_pce_cd_ppf_tas_sync_config_sha256(),
        (MediaKind::Ppf, CardKind::Arcade) => direct_pce_cd_ppf_arcade_tas_sync_config_sha256(),
        (MediaKind::Ppf, CardKind::MemoryBase) => {
            direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::Archive, CardKind::None) => direct_pce_cd_archive_tas_sync_config_sha256(),
        (MediaKind::Archive, CardKind::Arcade) => {
            direct_pce_cd_archive_arcade_tas_sync_config_sha256()
        }
        (MediaKind::Archive, CardKind::MemoryBase) => {
            direct_pce_cd_archive_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::Rar, CardKind::None) => direct_pce_cd_rar_tas_sync_config_sha256(),
        (MediaKind::Rar, CardKind::Arcade) => direct_pce_cd_rar_arcade_tas_sync_config_sha256(),
        (MediaKind::Rar, CardKind::MemoryBase) => {
            direct_pce_cd_rar_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::Zip, CardKind::None) => direct_pce_cd_zip_tas_sync_config_sha256(),
        (MediaKind::Zip, CardKind::Arcade) => direct_pce_cd_zip_arcade_tas_sync_config_sha256(),
        (MediaKind::Zip, CardKind::MemoryBase) => {
            direct_pce_cd_zip_memory_base_tas_sync_config_sha256()
        }
        (MediaKind::ArchivePpf, CardKind::None) => {
            direct_pce_cd_archive_ppf_tas_sync_config_sha256()
        }
        (MediaKind::SelectedArchivePpf, CardKind::None) => {
            direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256()
        }
        (MediaKind::RarPpf, CardKind::None) => direct_pce_cd_rar_ppf_tas_sync_config_sha256(),
        (MediaKind::SelectedRarPpf, CardKind::None) => {
            direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256()
        }
        (MediaKind::ZipPpf, CardKind::None) => direct_pce_cd_zip_ppf_tas_sync_config_sha256(),
        (MediaKind::SelectedZipPpf, CardKind::None) => {
            direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256()
        }
        (
            MediaKind::ArchivePpf
            | MediaKind::SelectedArchivePpf
            | MediaKind::RarPpf
            | MediaKind::SelectedRarPpf
            | MediaKind::ZipPpf
            | MediaKind::SelectedZipPpf,
            _,
        ) => unreachable!("archive PPF has no card profile"),
    }
}

fn export_import_reopen(
    directory: &Path,
    loader: &DirectPceCdTasExecutionLoader,
    media: MediaKind,
    card: CardKind,
) -> Result<TasProject> {
    let project_path = directory.join("source.ztas");
    let replay_path = directory.join("verified.zrpl");
    let imported_path = directory.join("imported.ztas");
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
    assert_eq!(
        project.identity().sync_config_sha256,
        expected_sync_config(media, card)
    );
    assert_eq!(
        project.identity().source_media_sha256 == project.identity().effective_media_sha256,
        matches!(media, MediaKind::Cue)
    );
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert!(project.branch("main").unwrap().events().is_empty());

    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.join("source-seek-cache"))?;
    let mut editor = TasEditorSession::new(project.clone(), &project_path, autosaves, cache)?;
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    assert_eq!(
        plan.verify_and_export_editor_session(&mut editor, &replay_path)?,
        replay_path
    );

    let imported = plan.import_replay_file(&replay_path, &imported_path, false)?;
    assert_eq!(TasProject::load(&imported_path)?, imported);
    assert_eq!(imported.identity(), project.identity());
    assert_eq!(imported.start_state(), project.start_state());
    assert_eq!(
        imported.branch("main").unwrap().input_spans(),
        project.branch("main").unwrap().input_spans()
    );
    assert!(imported.branch("main").unwrap().events().is_empty());
    assert!(imported.source_replay_sha256().is_some());
    assert!(imported.verification_is_current("main")?);

    let imported_autosaves =
        TasAutosaveStore::beside_manual_save(&imported_path, TasAutosaveConfig::default())?;
    let imported_cache = TasSeekStateCache::open(directory.join("imported-seek-cache"))?;
    let mut imported_editor =
        TasEditorSession::open(&imported_path, imported_autosaves, imported_cache)?;
    let mut engine = plan.load_editor_engine(imported_editor.project())?;
    assert!(engine.seek(&mut imported_editor, 1)?.reached_target());
    Ok(imported)
}

fn project_with_identity(project: &TasProject, identity: TasProjectIdentity) -> Result<TasProject> {
    let branch = project.branch("main").unwrap();
    TasProject::new(
        project.project_id(),
        identity,
        project.start_state().to_vec(),
        project.replay_start().clone(),
        TasInitialBranch {
            id: "main".to_owned(),
            name: branch.name().to_owned(),
            frame_count: branch.frame_count(),
            input_spans: branch.input_spans().to_vec(),
            events: branch.events().to_vec(),
        },
        BTreeMap::new(),
    )
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_every_exact_media_and_card_profile() -> Result<()> {
    let mut seed = 1;
    for media in [MediaKind::Cue, MediaKind::Chd, MediaKind::Iso] {
        for card in [CardKind::None, CardKind::Arcade, CardKind::MemoryBase] {
            let name = format!("pce-cd-zrpl-{media:?}-{card:?}").to_ascii_lowercase();
            let (directory, loader, _) = fixture(&name, media, seed)?;
            seed += 1;
            let backend = loader.load_fresh_backend()?;
            let disc_sha256 = backend
                .pce()
                .expect("PCE-CD fixture must load a PC Engine backend")
                .normalized_disc_hash()
                .expect("PCE-CD fixture must mount a normalized disc");
            let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
                crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                    disc_sha256,
                )
            });
            let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
                crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                    disc_sha256,
                )
            });
            let loaded = loader.load_fresh_backend()?;
            let pce = loaded.pce().expect("PCE-CD fixture must remain loaded");
            assert_eq!(
                pce.arcade_card_mode(),
                if matches!(card, CardKind::Arcade) {
                    PceArcadeCardMode::Enabled
                } else {
                    PceArcadeCardMode::Disabled
                }
            );
            assert_eq!(
                pce.memory_base_mode(),
                if matches!(card, CardKind::MemoryBase) {
                    PceMemoryBaseMode::Enabled
                } else {
                    PceMemoryBaseMode::Disabled
                }
            );
            export_import_reopen(directory.path(), &loader, media, card)?;
        }
    }
    Ok(())
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_every_exact_ppf_card_profile() -> Result<()> {
    for (seed, card) in (1u8..).zip([CardKind::None, CardKind::Arcade, CardKind::MemoryBase]) {
        let name = format!("pce-cd-zrpl-ppf-{card:?}").to_ascii_lowercase();
        let (directory, loader, source_disc_sha256) = ppf_fixture(&name, seed)?;
        let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                source_disc_sha256,
            )
        });
        let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                source_disc_sha256,
            )
        });
        let backend = loader.load_fresh_backend()?;
        let pce = backend.pce().expect("PPF fixture must remain loaded");
        assert_eq!(
            pce.arcade_card_mode(),
            if matches!(card, CardKind::Arcade) {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            }
        );
        assert_eq!(
            pce.memory_base_mode(),
            if matches!(card, CardKind::MemoryBase) {
                PceMemoryBaseMode::Enabled
            } else {
                PceMemoryBaseMode::Disabled
            }
        );
        let imported = export_import_reopen(directory.path(), &loader, MediaKind::Ppf, card)?;
        assert_ne!(
            imported.identity().source_media_sha256,
            imported.identity().effective_media_sha256
        );
    }
    Ok(())
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_every_exact_7z_card_profile() -> Result<()> {
    for card in [CardKind::None, CardKind::Arcade, CardKind::MemoryBase] {
        let name = format!("pce-cd-zrpl-archive-{card:?}").to_ascii_lowercase();
        let (directory, loader, _) = archive_fixture(&name)?;
        let disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("archive fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("archive fixture must mount a normalized disc");
        let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
        });
        let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
        });
        let loaded = loader.load_fresh_backend()?;
        let pce = loaded.pce().expect("archive fixture must remain loaded");
        assert_eq!(
            pce.arcade_card_mode(),
            if matches!(card, CardKind::Arcade) {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            }
        );
        assert_eq!(
            pce.memory_base_mode(),
            if matches!(card, CardKind::MemoryBase) {
                PceMemoryBaseMode::Enabled
            } else {
                PceMemoryBaseMode::Disabled
            }
        );
        export_import_reopen(directory.path(), &loader, MediaKind::Archive, card)?;
    }
    Ok(())
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_every_exact_rar_card_profile() -> Result<()> {
    for (fill, card) in (0x71..).zip([CardKind::None, CardKind::Arcade, CardKind::MemoryBase]) {
        let name = format!("pce-cd-zrpl-rar-{card:?}").to_ascii_lowercase();
        let directory = crate::test_support::test_directory(&name)?;
        let archive_path = directory.path().join("disc.rar");
        write_rar_fixture(&archive_path, fill)?;
        let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path,
            system_card(),
            SYSTEM_CARD_SHA256,
        );
        let disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("RAR fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("RAR fixture must mount a normalized disc");
        let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
        });
        let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
        });
        let loaded = loader.load_fresh_backend()?;
        let pce = loaded.pce().expect("RAR fixture must remain loaded");
        assert_eq!(
            pce.arcade_card_mode(),
            if matches!(card, CardKind::Arcade) {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            }
        );
        assert_eq!(
            pce.memory_base_mode(),
            if matches!(card, CardKind::MemoryBase) {
                PceMemoryBaseMode::Enabled
            } else {
                PceMemoryBaseMode::Disabled
            }
        );
        export_import_reopen(directory.path(), &loader, MediaKind::Rar, card)?;
    }
    Ok(())
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_every_exact_zip_card_profile() -> Result<()> {
    for (fill, card) in (0x81..).zip([CardKind::None, CardKind::Arcade, CardKind::MemoryBase]) {
        let name = format!("pce-cd-zrpl-zip-{card:?}").to_ascii_lowercase();
        let directory = crate::test_support::test_directory(&name)?;
        let archive_path = directory.path().join("disc.zip");
        write_zip_fixture(&archive_path, fill)?;
        let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path,
            system_card(),
            SYSTEM_CARD_SHA256,
        );
        let disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("ZIP fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("ZIP fixture must mount a normalized disc");
        let _arcade_catalog = matches!(card, CardKind::Arcade).then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
        });
        let _memory_base_catalog = matches!(card, CardKind::MemoryBase).then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
        });
        export_import_reopen(directory.path(), &loader, MediaKind::Zip, card)?;
    }
    Ok(())
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_unique_7z_cue_and_rejects_mutation() -> Result<()> {
    let (directory, loader, archive_path) = archive_fixture_with_fill("pce-cd-zrpl-archive", 0x61)?;
    let imported = export_import_reopen(
        directory.path(),
        &loader,
        MediaKind::Archive,
        CardKind::None,
    )?;
    assert_ne!(
        imported.identity().source_media_sha256,
        imported.identity().effective_media_sha256
    );
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    write_archive_fixture(&archive_path, 0x52)?;

    let mutation_replay_path = directory.path().join("mutated.zrpl");
    let project_path = directory.path().join("mutation-source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("mutation-seek-cache"))?;
    let mut editor = TasEditorSession::new(imported, &project_path, autosaves, cache)?;
    assert!(
        plan.verify_and_export_editor_session(&mut editor, &mutation_replay_path)
            .is_err()
    );
    assert!(!mutation_replay_path.exists());

    let imported_path = directory.path().join("mutated-import.ztas");
    assert!(
        plan.import_replay_file(
            &directory.path().join("verified.zrpl"),
            &imported_path,
            false
        )
        .is_err()
    );
    assert!(!imported_path.exists());
    Ok(())
}

#[test]
fn direct_pce_cd_verified_replay_roundtrips_unique_rar_cue_and_rejects_mutation() -> Result<()> {
    let (directory, loader, archive_path) = rar_fixture("pce-cd-zrpl-rar")?;
    let imported = export_import_reopen(directory.path(), &loader, MediaKind::Rar, CardKind::None)?;
    assert_ne!(
        imported.identity().source_media_sha256,
        imported.identity().effective_media_sha256
    );
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    write_rar_fixture(&archive_path, 0x5A)?;

    let mutation_replay_path = directory.path().join("mutated-rar.zrpl");
    let project_path = directory.path().join("mutation-rar-source.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("mutation-rar-seek-cache"))?;
    let mut editor = TasEditorSession::new(imported, &project_path, autosaves, cache)?;
    assert!(
        plan.verify_and_export_editor_session(&mut editor, &mutation_replay_path)
            .is_err()
    );
    assert!(!mutation_replay_path.exists());

    let imported_path = directory.path().join("mutated-rar-import.ztas");
    assert!(
        plan.import_replay_file(
            &directory.path().join("verified.zrpl"),
            &imported_path,
            false
        )
        .is_err()
    );
    assert!(!imported_path.exists());
    Ok(())
}

#[test]
fn direct_pce_cd_replay_import_rejects_mismatched_witnesses_before_publication() -> Result<()> {
    let (directory, loader, _) = fixture("pce-cd-zrpl-rejections", MediaKind::Cue, 0xE1)?;
    let imported = export_import_reopen(directory.path(), &loader, MediaKind::Cue, CardKind::None)?;
    let replay_path = directory.path().join("verified.zrpl");
    let plan = PrivateTasExecutionLoader::DirectPceCd(loader.clone());

    let disc_path = directory.path().join("disc.bin");
    let original_disc = fs::read(&disc_path)?;
    let mut changed_disc = original_disc.clone();
    changed_disc[0] ^= 1;
    fs::write(&disc_path, changed_disc)?;
    let wrong_source_path = directory.path().join("wrong-source.ztas");
    assert!(
        plan.import_replay_file(&replay_path, &wrong_source_path, false)
            .is_err()
    );
    assert!(!wrong_source_path.exists());
    fs::write(&disc_path, original_disc)?;

    let wrong_firmware = PrivateTasExecutionLoader::DirectPceCd(
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            directory.path().join("disc.cue"),
            system_card(),
            [0; 32],
        ),
    );
    let wrong_firmware_path = directory.path().join("wrong-firmware.ztas");
    assert!(
        wrong_firmware
            .import_replay_file(&replay_path, &wrong_firmware_path, false)
            .is_err()
    );
    assert!(!wrong_firmware_path.exists());

    let disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("PCE-CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("PCE-CD fixture must mount a normalized disc");
    let _arcade_catalog =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    let wrong_topology_path = directory.path().join("wrong-topology.ztas");
    assert!(
        plan.import_replay_file(&replay_path, &wrong_topology_path, false)
            .is_err()
    );
    assert!(!wrong_topology_path.exists());

    let mut wrong_devices = imported.identity().clone();
    wrong_devices.devices[0].configuration_sha256 = TasDigest([0xA5; 32]);
    let wrong_devices = project_with_identity(&imported, wrong_devices)?;
    assert!(
        plan.validate_project_branch_scope(&wrong_devices, "main")
            .is_err()
    );

    let mut wrong_persistence = imported.identity().clone();
    wrong_persistence.persistent_state = TasExternalIdentity::ExternalSha256(TasDigest([0x5A; 32]));
    let wrong_persistence = project_with_identity(&imported, wrong_persistence)?;
    assert!(
        plan.validate_project_branch_scope(&wrong_persistence, "main")
            .is_err()
    );
    Ok(())
}

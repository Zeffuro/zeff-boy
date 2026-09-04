use std::fs;
use std::io::{Cursor, Write};

use anyhow::Result;
use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};

use super::*;
use crate::emu_thread::TasExecutionProfile;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasDigest, TasEditorSession, TasExternalIdentity,
    TasInputFrame, TasProject, TasSeekStateCache,
};

mod arcade_multitap;
mod archive_multitap;
mod archive_ppf;
mod chd_multitap;
mod iso_multitap;
mod loaded_path;
mod memory_base_multitap;
mod multicue;
mod multitap;
mod runtime_validation;

const TEST_SYSTEM_CARD_SHA256: [u8; 32] = [
    0x8A, 0x39, 0xD2, 0xAB, 0xD3, 0x99, 0x9A, 0xB7, 0x3C, 0x34, 0xDB, 0x24, 0x76, 0x84, 0x9C, 0xDD,
    0xF3, 0x03, 0xCE, 0x38, 0x9B, 0x35, 0x82, 0x68, 0x50, 0xF9, 0xA7, 0x00, 0x58, 0x9B, 0x4A, 0x90,
];

fn fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let cue_path = directory.path().join("disc.cue");
    let disc_path = directory.path().join("disc.bin");
    let mut disc = vec![0; zeff_pce_core::hardware::CD_USER_SECTOR_BYTES * 4];
    for (index, byte) in disc.iter_mut().enumerate() {
        *byte = index as u8;
    }
    for (byte, seed) in disc.iter_mut().zip(name.bytes()) {
        *byte ^= seed;
    }
    fs::write(&disc_path, disc)?;
    fs::write(
        &cue_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    assert_eq!(
        zeff_firmware::sha256_bytes(system_card),
        TEST_SYSTEM_CARD_SHA256
    );
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            cue_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn chd_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let chd_path = directory.path().join("disc.chd");
    crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&chd_path)?;
    let mut bytes = fs::read(&chd_path)?;
    bytes[4 * 2_448] ^= name.bytes().fold(0, u8::wrapping_add);
    fs::write(&chd_path, bytes)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            chd_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn iso_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let iso_path = directory.path().join("disc.iso");
    let cue_path = directory.path().join("disc.cue");
    let mut disc = vec![0; zeff_pce_core::hardware::CD_USER_SECTOR_BYTES * 4];
    for (index, byte) in disc.iter_mut().enumerate() {
        *byte = index as u8;
    }
    for (byte, seed) in disc.iter_mut().zip(name.bytes()) {
        *byte ^= seed;
    }
    fs::write(&iso_path, disc)?;
    fs::write(
        cue_path,
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            iso_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn write_archive_fixture(path: &std::path::Path, fill: u8) -> Result<()> {
    write_archive_fixture_layout(path, "set", fill, false)
}

fn write_archive_fixture_layout(
    path: &std::path::Path,
    directory: &str,
    fill: u8,
    extra_member: bool,
) -> Result<()> {
    write_archive_fixture_bytes(
        path,
        directory,
        vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        extra_member,
    )
}

fn write_archive_fixture_bytes(
    path: &std::path::Path,
    directory: &str,
    disc: Vec<u8>,
    extra_member: bool,
) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let cue_name = format!("{directory}/disc.cue");
    let disc_name = format!("{directory}/disc.bin");
    let mut writer = ArchiveWriter::create(path)?;
    writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
    writer.push_archive_entry(
        ArchiveEntry::new_file(&cue_name),
        Some(Cursor::new(cue.to_vec())),
    )?;
    writer.push_archive_entry(ArchiveEntry::new_file(&disc_name), Some(Cursor::new(disc)))?;
    if extra_member {
        writer.push_archive_entry(
            ArchiveEntry::new_file("metadata.txt"),
            Some(Cursor::new(b"repacked".to_vec())),
        )?;
    }
    writer.finish()?;
    Ok(())
}

fn archive_fixture_with_fill(
    name: &str,
    fill: u8,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let archive_path = directory.path().join("disc.7z");
    write_archive_fixture(&archive_path, fill)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn write_rar_fixture(path: &Path, directory: &str, fill: u8) -> Result<()> {
    write_rar_fixture_bytes(
        path,
        directory,
        vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
    )
}

fn write_rar_fixture_bytes(path: &Path, directory: &str, disc: Vec<u8>) -> Result<()> {
    let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
    let entries = [
        (format!("{directory}/disc.cue"), cue.to_vec()),
        (format!("{directory}/disc.bin"), disc),
    ]
    .into_iter()
    .map(|(name, data)| {
        RarArchiveEntry::new(
            name.into_bytes(),
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

fn rar_fixture_with_fill(
    name: &str,
    fill: u8,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let archive_path = directory.path().join("disc.rar");
    write_rar_fixture(&archive_path, "set", fill)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn write_zip_fixture(path: &Path, directory: &str, fill: u8) -> Result<()> {
    write_zip_fixture_bytes(
        path,
        directory,
        vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
    )
}

fn write_zip_fixture_bytes(path: &Path, directory: &str, disc: Vec<u8>) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut writer = zip::ZipWriter::new(file);
    for (name, bytes) in [
        (
            format!("{directory}/disc.cue"),
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec(),
        ),
        (format!("{directory}/disc.bin"), disc),
    ] {
        writer.start_file(name, zip::write::SimpleFileOptions::default())?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    Ok(())
}

fn zip_fixture_with_fill(
    name: &str,
    fill: u8,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
)> {
    let directory = crate::test_support::test_directory(name)?;
    let archive_path = directory.path().join("disc.zip");
    write_zip_fixture(&archive_path, "set", fill)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    Ok((
        directory,
        DirectPceCdTasExecutionLoader::new_with_system_card_override(
            archive_path,
            system_card,
            TEST_SYSTEM_CARD_SHA256,
        ),
    ))
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

fn ppf_arcade_fixture(
    name: &str,
) -> Result<(
    crate::test_support::TestDirectory,
    DirectPceCdTasExecutionLoader,
    crate::emu_backend::pce_profiles::TestArcadeCardCatalogGuard,
)> {
    let (directory, mut loader) = fixture(name)?;
    let source_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("PC Engine CD fixture must load")
        .normalized_disc_hash()
        .expect("PC Engine CD fixture must mount a normalized disc");
    let catalog = crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
        source_disc_sha256,
    );
    loader.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &loader.source_path,
        vec![
            ("first.ppf".to_owned(), ppf1(0, &[0xA5])),
            ("second.ppf".to_owned(), ppf1(1, &[0x5A])),
        ],
    )?);
    Ok((directory, loader, catalog))
}

#[test]
fn legacy_direct_cue_source_identity_vector_is_stable() -> Result<()> {
    let (_directory, loader) = fixture("pce-cd-tas-legacy-vectors")?;
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().source_media_sha256.to_hex(),
        "fd13f7e42a934245cf82cacffd20431380a32bc882036192e8ce415c8f88cb7f"
    );
    assert_eq!(
        project.identity().effective_media_sha256,
        project.identity().source_media_sha256
    );
    Ok(())
}

#[test]
fn direct_cue_ppf_arcade_binds_source_order_and_normalized_disc() -> Result<()> {
    let (directory, loader, _catalog) = ppf_arcade_fixture("pce-cd-tas-ppf-arcade")?;
    let project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_ppf_arcade_tas_sync_config_sha256()
    );
    let backend = loader.load_fresh_backend()?;
    assert_eq!(
        backend.pce().expect("PC Engine backend").arcade_card_mode(),
        zeff_pce_core::hardware::PceArcadeCardMode::Enabled
    );
    loader.load_editor_engine(&project)?;

    let mut reordered = loader.clone();
    reordered.ppf_stack_override = Some(crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &reordered.source_path,
        vec![
            ("second.ppf".to_owned(), ppf1(1, &[0x5A])),
            ("first.ppf".to_owned(), ppf1(0, &[0xA5])),
        ],
    )?);
    assert!(reordered.load_editor_engine(&project).is_err());
    let mut bytes = fs::read(directory.path().join("disc.bin"))?;
    bytes[0] ^= 1;
    fs::write(directory.path().join("disc.bin"), bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn create_reopen_execute_and_continue_direct_pce_cd_without_host_persistence() -> Result<()> {
    let (directory, loader) = fixture("pce-cd-tas-create")?;
    let project_path = directory.path().join("movie.ztas");
    let mut project = loader.create_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, project);
    assert_eq!(
        super::super::classify_direct_tas_execution_profile(&project)?,
        TasExecutionProfile::DirectPceCd
    );
    assert_eq!(
        project.identity().persistent_state,
        TasExternalIdentity::Absent
    );
    assert_eq!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(project.identity().firmware.len(), 1);

    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let mut engine = loader.load_editor_engine(&project)?;
    let autosaves =
        TasAutosaveStore::beside_manual_save(&project_path, TasAutosaveConfig::default())?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(project, &project_path, autosaves, cache)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());

    let mut expected = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );
    let mut actual = engine.into_backend();
    actual.step_frame();
    expected.step_frame();
    assert_eq!(actual.encode_state_bytes()?, expected.encode_state_bytes()?);
    assert_eq!(actual.flush_battery_sram()?, None);
    let replay_path = directory.path().join("verified.zrpl");
    let plan = super::super::PrivateTasExecutionLoader::DirectPceCd(loader.clone());
    assert_eq!(
        plan.verify_and_export_editor_session(&mut editor, &replay_path)?,
        replay_path
    );
    assert!(replay_path.exists());
    Ok(())
}

#[test]
fn unique_7z_cue_binds_outer_source_and_rejects_mutation() -> Result<()> {
    let (directory, loader) = archive_fixture_with_fill("pce-cd-tas-archive", 0x53)?;
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_archive_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert!(loader.load_editor_engine(&project).is_ok());
    write_archive_fixture(&directory.path().join("disc.7z"), 0x63)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn unique_rar_cue_binds_outer_source_and_rejects_mutation() -> Result<()> {
    let (directory, loader) = rar_fixture_with_fill("pce-cd-tas-rar", 0x54)?;
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_rar_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    loader.load_editor_engine(&project)?;
    write_rar_fixture(&directory.path().join("disc.rar"), "set", 0x64)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn unique_zip_cue_binds_outer_source_and_rejects_mutation() -> Result<()> {
    let (directory, loader) = zip_fixture_with_fill("pce-cd-tas-zip", 0x55)?;
    let project = loader.create_project()?;
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_zip_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    loader.load_editor_engine(&project)?;
    write_zip_fixture(&directory.path().join("disc.zip"), "set", 0x65)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn equal_7z_and_rar_discs_have_distinct_strict_identities() -> Result<()> {
    let (_seven_zip_directory, seven_zip_loader) =
        archive_fixture_with_fill("pce-cd-tas-archive-cross-format", 0x55)?;
    let (_rar_directory, rar_loader) = rar_fixture_with_fill("pce-cd-tas-rar-cross-format", 0x55)?;
    let seven_zip = seven_zip_loader.create_project()?;
    let rar = rar_loader.create_project()?;
    assert_eq!(
        seven_zip.identity().effective_media_sha256,
        rar.identity().effective_media_sha256
    );
    assert_ne!(
        seven_zip.identity().source_media_sha256,
        rar.identity().source_media_sha256
    );
    assert_ne!(
        seven_zip.identity().sync_config_sha256,
        rar.identity().sync_config_sha256
    );
    assert!(seven_zip_loader.load_editor_engine(&rar).is_err());
    assert!(rar_loader.load_editor_engine(&seven_zip).is_err());
    Ok(())
}

#[test]
fn catalog_recognized_rar_disc_selects_each_exact_card_profile() -> Result<()> {
    for (name, arcade, fill) in [
        ("pce-cd-tas-rar-arcade", true, 0xC5),
        ("pce-cd-tas-rar-memory-base", false, 0xC6),
    ] {
        let (directory, loader) = rar_fixture_with_fill(name, fill)?;
        let disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("RAR fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("RAR fixture must mount a normalized disc");
        let _arcade_catalog = arcade.then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
        });
        let _memory_base_catalog = (!arcade).then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
        });
        let backend = loader.load_fresh_backend()?;
        let pce = backend.pce().expect("RAR fixture must remain loaded");
        assert_eq!(
            pce.arcade_card_mode(),
            if arcade {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            }
        );
        assert_eq!(
            pce.memory_base_mode(),
            if arcade {
                PceMemoryBaseMode::Disabled
            } else {
                PceMemoryBaseMode::Enabled
            }
        );
        let project = loader.create_project()?;
        assert_eq!(
            project.identity().sync_config_sha256,
            if arcade {
                super::super::direct_pce_cd::direct_pce_cd_rar_arcade_tas_sync_config_sha256()
            } else {
                super::super::direct_pce_cd::direct_pce_cd_rar_memory_base_tas_sync_config_sha256()
            }
        );
        loader.load_editor_engine(&project)?;
        write_rar_fixture(
            &directory.path().join("disc.rar"),
            "set",
            fill.wrapping_add(0x10),
        )?;
        assert!(loader.load_editor_engine(&project).is_err());
    }
    Ok(())
}

#[test]
fn unique_7z_cue_binds_repackaging_and_selected_member_with_equal_effective_disc() -> Result<()> {
    let (directory, loader) =
        archive_fixture_with_fill("pce-cd-tas-archive-source-identity", 0x56)?;
    let original = loader.create_project()?;
    let archive = directory.path().join("disc.7z");

    write_archive_fixture_layout(&archive, "set", 0x56, true)?;
    let repacked = loader.create_project()?;
    assert_eq!(
        repacked.identity().effective_media_sha256,
        original.identity().effective_media_sha256
    );
    assert_ne!(
        repacked.identity().source_media_sha256,
        original.identity().source_media_sha256
    );
    assert!(loader.load_editor_engine(&original).is_err());

    write_archive_fixture_layout(&archive, "renamed", 0x56, false)?;
    let renamed = loader.create_project()?;
    assert_eq!(
        renamed.identity().effective_media_sha256,
        original.identity().effective_media_sha256
    );
    assert_ne!(
        renamed.identity().source_media_sha256,
        original.identity().source_media_sha256
    );
    assert_ne!(
        renamed.identity().source_media_sha256,
        repacked.identity().source_media_sha256
    );
    Ok(())
}

#[test]
fn catalog_recognized_7z_disc_selects_each_exact_card_profile() -> Result<()> {
    for (name, arcade, fill) in [
        ("pce-cd-tas-archive-arcade", true, 0x57),
        ("pce-cd-tas-archive-memory-base", false, 0x58),
    ] {
        let (directory, loader) = archive_fixture_with_fill(name, fill)?;
        let disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("archive fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("archive fixture must mount a normalized disc");
        let _arcade_catalog = arcade.then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256)
        });
        let _memory_base_catalog = (!arcade).then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(disc_sha256)
        });
        let backend = loader.load_fresh_backend()?;
        let pce = backend.pce().expect("archive fixture must remain loaded");
        assert_eq!(
            pce.arcade_card_mode(),
            if arcade {
                zeff_pce_core::hardware::PceArcadeCardMode::Enabled
            } else {
                zeff_pce_core::hardware::PceArcadeCardMode::Disabled
            }
        );
        assert_eq!(
            pce.memory_base_mode(),
            if arcade {
                PceMemoryBaseMode::Disabled
            } else {
                PceMemoryBaseMode::Enabled
            }
        );
        let project = loader.create_project()?;
        assert_eq!(
            project.identity().sync_config_sha256,
            if arcade {
                super::super::direct_pce_cd::direct_pce_cd_archive_arcade_tas_sync_config_sha256()
            } else {
                super::super::direct_pce_cd::direct_pce_cd_archive_memory_base_tas_sync_config_sha256()
            }
        );
        assert!(loader.load_editor_engine(&project).is_ok());
        write_archive_fixture(&directory.path().join("disc.7z"), fill.wrapping_add(0x10))?;
        assert!(loader.load_editor_engine(&project).is_err());
    }
    Ok(())
}

#[test]
fn direct_chd_memory_base_binds_raw_source_and_normalized_disc_before_continuation() -> Result<()> {
    let (directory, loader) = chd_fixture("pce-cd-tas-chd-memory-base")?;
    let normalized_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let mut project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
    );
    let mut input = TasInputFrame::default();
    input.players[0].buttons = 0x03;
    input.players[0].dpad = 0x04;
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let autosaves = TasAutosaveStore::beside_manual_save(
        &directory.path().join("movie.ztas"),
        TasAutosaveConfig::default(),
    )?;
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
    let mut editor = TasEditorSession::new(
        project.clone(),
        directory.path().join("movie.ztas"),
        autosaves,
        cache,
    )?;
    let mut engine = loader.load_editor_engine(&project)?;
    assert!(engine.seek(&mut editor, 1)?.reached_target());
    let mut expected = loader.load_fresh_backend()?;
    expected.set_input(input.players[0].buttons, input.players[0].dpad);
    expected.step_frame();
    assert_eq!(
        engine.backend().encode_state_bytes()?,
        expected.encode_state_bytes()?
    );

    let path = directory.path().join("disc.chd");
    let mut bytes = fs::read(&path)?;
    bytes[4 * 2_448] ^= 1;
    fs::write(path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_iso_memory_base_binds_raw_source_and_rejects_mutated_or_ambiguous_cue_selection()
-> Result<()> {
    let (directory, loader) = iso_fixture("pce-cd-tas-iso-memory-base")?;
    let normalized_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
    );

    let iso_path = directory.path().join("disc.iso");
    let mut bytes = fs::read(&iso_path)?;
    bytes[0] ^= 1;
    fs::write(&iso_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());

    let (directory, loader) = iso_fixture("pce-cd-tas-iso-memory-base-ambiguous")?;
    let normalized_disc_sha256 = loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            normalized_disc_sha256,
        );
    let project = loader.create_project()?;
    fs::write(
        directory.path().join("duplicate.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_ppf_binds_ordered_snapshot_and_normalized_disc_before_reopen() -> Result<()> {
    let (directory, base_loader) = fixture("pce-cd-tas-ppf")?;
    let cue_path = directory.path().join("disc.cue");
    let first = ppf1(0, &[0xA5]);
    let second = ppf1(1, &[0x5A]);
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), first.clone()),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let reversed = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("second.ppf".to_owned(), second),
            ("first.ppf".to_owned(), first),
        ],
    )?;
    assert_ne!(
        stack.source_media_identity(),
        reversed.source_media_identity()
    );
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        base_loader.system_card_override.unwrap(),
        TEST_SYSTEM_CARD_SHA256,
        stack,
    );
    let project = loader.create_project()?;
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_ppf_tas_sync_config_sha256()
    );
    let disc_path = directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[2] ^= 1;
    fs::write(disc_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

#[test]
fn direct_ppf_memory_base_reopens_and_rejects_patch_order_or_base_mutation() -> Result<()> {
    let (directory, base_loader) = fixture("pce-cd-tas-ppf-memory-base")?;
    let cue_path = directory.path().join("disc.cue");
    let base_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(base_disc_sha256);
    let first = ppf1(0, &[0xA5]);
    let second = ppf1(1, &[0x5A]);
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), first.clone()),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let mutated = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("first.ppf".to_owned(), ppf1(0, &[0xA4])),
            ("second.ppf".to_owned(), second.clone()),
        ],
    )?;
    let reversed = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &cue_path,
        vec![
            ("second.ppf".to_owned(), second),
            ("first.ppf".to_owned(), first),
        ],
    )?;
    let system_card = base_loader.system_card_override.unwrap();
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path.clone(),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
        stack,
    );
    let project_path = directory.path().join("movie.ztas");
    let project = loader.create_project_file(&project_path)?;
    assert_eq!(TasProject::load(&project_path)?, project);
    assert_eq!(
        project.identity().sync_config_sha256,
        super::super::direct_pce_cd::direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
    );
    assert_ne!(
        project.identity().source_media_sha256,
        project.identity().effective_media_sha256
    );
    assert_eq!(
        loader
            .load_editor_engine(&project)?
            .backend()
            .pce()
            .unwrap()
            .memory_base_mode(),
        PceMemoryBaseMode::Enabled
    );

    let reversed_loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path.clone(),
        system_card,
        TEST_SYSTEM_CARD_SHA256,
        reversed,
    );
    assert!(reversed_loader.load_editor_engine(&project).is_err());
    let mutated_loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        cue_path,
        system_card,
        TEST_SYSTEM_CARD_SHA256,
        mutated,
    );
    assert!(mutated_loader.load_editor_engine(&project).is_err());

    let disc_path = directory.path().join("disc.bin");
    let mut bytes = fs::read(&disc_path)?;
    bytes[3] ^= 1;
    fs::write(disc_path, bytes)?;
    assert!(loader.load_editor_engine(&project).is_err());
    Ok(())
}

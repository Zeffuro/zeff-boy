use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::Result;
use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};

use super::*;

#[derive(Clone, Copy)]
enum ArchiveKind {
    SevenZip,
    Rar,
    Zip,
}

#[derive(Clone, Copy, Debug)]
enum CardKind {
    Arcade,
    MemoryBase,
}

impl ArchiveKind {
    fn extension(self) -> &'static str {
        match self {
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Zip => "zip",
        }
    }
}

#[test]
fn native_cli_two_pass_verifies_and_exports_selected_second_cue_for_archives() -> Result<()> {
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        let directory = test_directory(&format!(
            "tas-cli-pce-cd-selected-multicue-{}",
            kind.extension()
        ))?;
        let archive = directory.path().join(format!("disc.{}", kind.extension()));
        write_multicue_archive(&archive, kind)?;
        let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
        let loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
            archive.clone(),
            archive.join("second").join("disc.cue"),
            system_card,
            zeff_firmware::sha256_bytes(system_card),
        )?;
        verifies_and_exports_direct_pce_cd(
            directory.path(),
            loader,
            zeff_pce_core::hardware::PceMemoryBaseMode::Disabled,
            zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
        )?;
    }
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_selected_card_profiles_for_all_six_routes() -> Result<()> {
    let mut fill = 0xA1;
    for kind in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip] {
        for card in [CardKind::Arcade, CardKind::MemoryBase] {
            verify_selected_card_profile(kind, card, fill)?;
            fill += 1;
        }
    }
    Ok(())
}

fn verify_selected_card_profile(kind: ArchiveKind, card: CardKind, fill: u8) -> Result<()> {
    let directory = test_directory(&format!(
        "tas-cli-pce-cd-selected-{}-{card:?}",
        kind.extension()
    ))?;
    let archive = directory.path().join(format!("disc.{}", kind.extension()));
    write_multicue_archive_with_fill(&archive, kind, fill)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
        archive.clone(),
        archive.join("second").join("disc.cue"),
        system_card,
        zeff_firmware::sha256_bytes(system_card),
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
    };
    verifies_and_exports_direct_pce_cd(
        directory.path(),
        loader,
        if matches!(card, CardKind::MemoryBase) {
            zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
        } else {
            zeff_pce_core::hardware::PceMemoryBaseMode::Disabled
        },
        if matches!(card, CardKind::Arcade) {
            zeff_pce_core::hardware::PceArcadeCardMode::Enabled
        } else {
            zeff_pce_core::hardware::PceArcadeCardMode::Disabled
        },
    )?;
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_archive_multitap_for_all_six_routes() -> Result<()> {
    for (index, kind) in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip]
        .into_iter()
        .enumerate()
    {
        for selected in [false, true] {
            let directory = test_directory(&format!(
                "tas-cli-pce-cd-archive-multitap-{}-{selected}",
                kind.extension()
            ))?;
            let archive = directory.path().join(format!("disc.{}", kind.extension()));
            let fill = 0xE1 + index as u8 + u8::from(selected) * 8;
            if selected {
                write_multicue_archive_with_fill(&archive, kind, fill)?;
            } else {
                write_unique_archive(&archive, kind, fill)?;
            }
            let rom_path = selected.then(|| archive.join("second").join("disc.cue"));
            let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
            let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
            let base = if let Some(rom_path) = rom_path.clone() {
                DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
                    archive.clone(),
                    rom_path,
                    system_card,
                    firmware_sha256,
                )?
            } else {
                DirectPceCdTasExecutionLoader::new_with_system_card_override(
                    archive.clone(),
                    system_card,
                    firmware_sha256,
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
            let loader = if let Some(rom_path) = rom_path {
                DirectPceCdTasExecutionLoader::new_multitap_with_rom_path_and_system_card_override(
                    archive,
                    rom_path,
                    system_card,
                    firmware_sha256,
                )?
            } else {
                DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
                    archive,
                    system_card,
                    firmware_sha256,
                )
            };
            verifies_and_exports_direct_pce_cd_multitap(directory.path(), loader)?;
        }
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
    second[0..4].copy_from_slice(&[0x43, 0x4D, fill, fill.rotate_left(1)]);
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
    disc[0..4].copy_from_slice(&[0x43, 0x55, fill, fill.rotate_left(1)]);
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

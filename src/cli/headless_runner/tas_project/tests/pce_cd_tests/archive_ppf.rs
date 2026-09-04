use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::Result;
use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod};

use super::*;

#[derive(Clone, Copy, Debug)]
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
}

#[test]
fn native_cli_two_pass_verifies_archive_ppf_for_all_six_routes() -> Result<()> {
    for (index, kind) in [ArchiveKind::SevenZip, ArchiveKind::Rar, ArchiveKind::Zip]
        .into_iter()
        .enumerate()
    {
        for selected in [false, true] {
            let directory = test_directory(&format!(
                "tas-cli-pce-cd-archive-ppf-{}-{selected}",
                kind.extension()
            ))?;
            let archive = directory.path().join(format!("disc.{}", kind.extension()));
            write_archive_ppf(&archive, kind, selected, 0x71 + index as u8)?;
            let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
            let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
            let loader = if selected {
                DirectPceCdTasExecutionLoader::new_with_rom_path_and_system_card_override(
                    archive.clone(),
                    archive.join("second").join("disc.cue"),
                    system_card,
                    firmware_sha256,
                )?
            } else {
                DirectPceCdTasExecutionLoader::new_with_system_card_override(
                    archive,
                    system_card,
                    firmware_sha256,
                )
            };
            verifies_and_exports_direct_pce_cd(
                directory.path(),
                loader,
                zeff_pce_core::hardware::PceMemoryBaseMode::Disabled,
                zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
            )?;
        }
    }
    Ok(())
}

fn write_archive_ppf(path: &Path, kind: ArchiveKind, selected: bool, fill: u8) -> Result<()> {
    let target = if selected { "second" } else { "set" };
    let mut entries = Vec::new();
    if selected {
        entries.extend(cue_entries("first", fill ^ 0xFF));
    }
    entries.extend(cue_entries(target, fill));
    entries.push((
        format!("{target}/disc.ppf/0001.ppf"),
        ppf1(0, &[fill.rotate_left(1)]),
    ));
    write_entries(path, kind, entries)
}

fn cue_entries(directory: &str, fill: u8) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            format!("{directory}/disc.cue"),
            b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec(),
        ),
        (
            format!("{directory}/disc.bin"),
            vec![fill; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
        ),
    ]
}

fn write_entries(path: &Path, kind: ArchiveKind, entries: Vec<(String, Vec<u8>)>) -> Result<()> {
    match kind {
        ArchiveKind::SevenZip => {
            let mut writer = ArchiveWriter::create(path)?;
            writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::COPY)]);
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(&name), Some(Cursor::new(bytes)))?;
            }
            writer.finish()?;
        }
        ArchiveKind::Rar => {
            let entries = entries
                .into_iter()
                .map(|(name, bytes)| {
                    RarArchiveEntry::new(
                        name.into_bytes(),
                        EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(bytes)),
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
                writer.write_all(&bytes)?;
            }
            writer.finish()?;
        }
    }
    Ok(())
}

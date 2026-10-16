use super::*;
use crate::emu_backend::MdBackend;
use crate::emu_backend::capabilities::TasPersistentStateIdentity;

fn save_rom(size: usize) -> Vec<u8> {
    let mut bytes = rom(b'U', &LOOP);
    bytes[0x1b0..0x1ba].copy_from_slice(&[0x52, 0x41, 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20]);
    bytes[0x1ba..0x1bc].copy_from_slice(&((size * 2 - 1) as u16).to_be_bytes());
    bytes
}

fn pattern(size: usize) -> Vec<u8> {
    (0..size)
        .map(|index| (index as u8).wrapping_mul(37) ^ (index >> 8) as u8)
        .collect()
}

fn paths(
    directory: &std::path::Path,
    size: usize,
    stem: &str,
) -> [(std::path::PathBuf, std::path::PathBuf); 2] {
    let suffix = if size == 512 {
        String::new()
    } else {
        format!("-{size}")
    };
    let direct_name = if stem.is_empty() { "direct" } else { stem };
    let archive_name = if stem.is_empty() {
        "collection".to_owned()
    } else {
        format!("{stem}-collection")
    };
    let direct = directory.join(format!("{direct_name}{suffix}.md"));
    let archive = directory.join(format!("{archive_name}{suffix}.zip"));
    [
        (direct.clone(), direct),
        (archive.clone(), archive.join("folder/game.md")),
    ]
}

fn load_at(
    bytes: Vec<u8>,
    source: &std::path::Path,
    path: &std::path::Path,
    load_save: bool,
) -> anyhow::Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Md,
        source,
        path,
        Some(bytes),
        BackendLoadConfig {
            md_load_save_ram: load_save,
            ..BackendLoadConfig::default()
        },
    )?
    .backend)
}

fn md_mut(backend: &mut EmuBackend) -> &mut MdBackend {
    let EmuBackend::Md(backend) = backend else {
        panic!("expected Mega Drive backend");
    };
    backend
}

#[test]
fn md_nonvolatile_ram_roundtrips_all_cells_through_direct_and_archive_paths() -> anyhow::Result<()>
{
    let directory = crate::test_support::test_directory("md-save-roundtrip")?;
    for size in [512, 8192, 32768] {
        let expected = pattern(size);
        for (source, path) in paths(directory.path(), size, "") {
            let bytes = save_rom(size);
            let identity = zeff_firmware::sha256_bytes(&bytes);
            let mut backend = load_at(bytes.clone(), &source, &path, true)?;
            assert_eq!(
                backend.save_ram_kind(),
                SaveRamKind::known_nonvolatile(size)
            );
            assert!(!backend.has_battery());
            assert!(backend.save_ram_kind().is_persistent());
            assert_eq!(
                TasPersistentStateIdentity::from_save_ram_kind(backend.save_ram_kind()),
                TasPersistentStateIdentity::ExternalPersistent { size }
            );
            md_mut(&mut backend).emu.load_save_ram(&expected)?;
            assert_eq!(backend.rom_hash(), identity);
            assert_eq!(
                backend.battery_components(),
                vec![(crate::save_paths::SRAM_COMPONENT, expected.clone())]
            );
            let save = source.with_extension("sav");
            assert_eq!(
                backend.flush_battery_sram()?,
                Some(save.display().to_string())
            );
            assert_eq!(std::fs::read(&save)?, expected);
            let receipt = backend.battery_generation_receipt()?;
            assert!(receipt.is_consistent());
            assert_eq!(receipt.component_sha256, backend.battery_component_hash());
            assert_eq!(receipt.components.len(), 1);
            assert_eq!(
                receipt.components[0].name,
                crate::save_paths::SRAM_COMPONENT
            );
            assert_eq!(receipt.components[0].byte_len, size as u64);
            assert_eq!(
                receipt.components[0].sha256,
                zeff_firmware::sha256_bytes(&std::fs::read(&save)?)
            );
            backend.flush_battery_sram()?;
            assert_eq!(std::fs::read(&save)?, expected);
            assert_eq!(backend.battery_generation_receipt()?, receipt);
            drop(backend);

            let restored = load_at(bytes, &source, &path, true)?;
            assert_eq!(machine(&restored).dump_save_ram(), Some(expected.clone()));
            assert_eq!(restored.rom_hash(), identity);
            assert_eq!(restored.battery_generation_receipt()?, receipt);
        }
    }
    Ok(())
}

#[test]
fn md_invalid_existing_save_is_rejected_without_publication() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-save-invalid")?;
    for size in [512, 8192, 32768] {
        for (source, path) in paths(directory.path(), size, "invalid") {
            let save = source.with_extension("sav");
            for length in [0, size - 1, size + 1, size * 2] {
                let invalid = vec![0xa7; length];
                std::fs::write(&save, &invalid)?;
                assert!(load_at(save_rom(size), &source, &path, true).is_err());
                assert_eq!(std::fs::read(&save)?, invalid);
                let skipped = load_at(save_rom(size), &source, &path, false)?;
                assert_eq!(machine(&skipped).dump_save_ram(), Some(vec![0xff; size]));
                assert_eq!(std::fs::read(&save)?, invalid);
            }
        }
    }
    Ok(())
}

#[test]
fn md_save_read_and_write_failures_preserve_existing_target() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-save-failure")?;
    for size in [512, 8192, 32768] {
        for (source, path) in paths(directory.path(), size, "blocked") {
            let save = source.with_extension("sav");
            std::fs::create_dir(&save)?;
            let marker = save.join("preserved");
            std::fs::write(&marker, b"keep")?;
            assert!(load_at(save_rom(size), &source, &path, true).is_err());
            let mut backend = load_at(save_rom(size), &source, &path, false)?;
            md_mut(&mut backend).emu.load_save_ram(&pattern(size))?;
            assert!(backend.flush_battery_sram().is_err());
            assert_eq!(std::fs::read(marker)?, b"keep");
        }
    }
    Ok(())
}

#[test]
fn md_disabled_publication_and_absent_ram_do_not_create_saves() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-save-disabled")?;
    for size in [512, 8192, 32768] {
        for (source, path) in paths(directory.path(), size, "disabled") {
            let mut backend = load_at(save_rom(size), &source, &path, true)?;
            md_mut(&mut backend).emu.load_save_ram(&pattern(size))?;
            md_mut(&mut backend).set_host_persistence_enabled(false);
            assert_eq!(backend.flush_battery_sram()?, None);
            assert!(!source.with_extension("sav").exists());

            let mut absent = load_at(rom(b'U', &LOOP), &source, &path, true)?;
            assert_eq!(absent.save_ram_kind(), SaveRamKind::none());
            assert!(absent.battery_components().is_empty());
            assert_eq!(absent.flush_battery_sram()?, None);
            assert!(!source.with_extension("sav").exists());
        }
    }
    Ok(())
}

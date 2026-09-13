use std::sync::{Arc, atomic::AtomicBool};

use super::*;
use crate::audio_discovery::{ScanLimits, ScanStatus, test_support::gba_fixture};
use crate::test_support::{build_gb_test_rom, build_nes_test_rom};

fn config() -> BackendLoadConfig {
    BackendLoadConfig {
        gba_load_battery_sram: false,
        gba_seed_rtc_from_host: false,
        ..Default::default()
    }
}

#[test]
fn audio_discovery_uses_loaded_bytes_and_shares_rom_across_reset_clone_and_restore()
-> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-loaded-rom")?;
    let path = directory.path().join("fixture.gba");
    let bytes = gba_fixture();
    std::fs::write(&path, &bytes)?;
    let mut loaded =
        load_backend_from_rom_source(ActiveSystem::GameBoyAdvance, &path, &path, None, config())?;
    let input = loaded.backend.audio_discovery_input().unwrap();
    let EmuBackend::Gba(gba) = &mut loaded.backend else {
        unreachable!()
    };
    assert_eq!(input.bytes.as_ptr(), gba.emu.cartridge_rom_bytes().as_ptr());
    assert!(Arc::ptr_eq(
        &input.bytes,
        &gba.emu.clone().cartridge_rom_snapshot()
    ));
    let state = gba.emu.encode_state()?;
    std::fs::write(&path, b"changed after the ROM was loaded")?;
    let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert_eq!(manifest.scan.status, ScanStatus::Complete);
    assert_eq!(manifest.scan.candidates.len(), 1);
    let source = manifest.source.as_ref().unwrap();
    assert_eq!(source.sha256, zeff_firmware::sha256_hex(&bytes));
    assert_eq!(source.kind, "direct_gba_file");
    assert_eq!(
        manifest.scan.media.sha256.as_deref(),
        Some(zeff_firmware::sha256_hex(&bytes).as_str())
    );
    assert_eq!(state, gba.emu.encode_state()?);
    assert_eq!(gba.emu.frame_count(), 0);
    zeff_emu_common::time::Reset::reset(&mut gba.emu);
    gba.emu.load_state_from_bytes(state)?;
    assert!(Arc::ptr_eq(&input.bytes, &gba.emu.cartridge_rom_snapshot()));
    assert_eq!(&*input.bytes, &bytes);
    Ok(())
}

#[test]
fn audio_discovery_retains_modded_fds_original_and_effective_media() -> anyhow::Result<()> {
    static TEST_FDS_BIOS: [u8; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE] =
        [0xFF; zeff_nes_core::hardware::cartridge::mappers::FDS_BIOS_SIZE];
    let directory = crate::test_support::test_directory("audio-loaded-fds")?;
    crate::mods::with_test_mods_root(directory.path(), || -> anyhow::Result<()> {
        let raw = vec![0x55; zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE];
        let path = directory.path().join("fixture.fds");
        std::fs::write(&path, &raw)?;
        let mods_dir = crate::mods::mods_dir_for_rom(ActiveSystem::Nes, crc32fast::hash(&raw));
        std::fs::create_dir_all(&mods_dir)?;
        let patch = b"PATCH\x00\x00\x10\x00\x01\x44EOF";
        std::fs::write(mods_dir.join("effective.ips"), patch)?;
        crate::mods::save_mod_config(
            &mods_dir,
            &[crate::mods::ModEntry {
                filename: "effective.ips".into(),
                enabled: true,
                target: None,
            }],
        );

        let loaded = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &path,
            &path,
            None,
            BackendLoadConfig {
                apply_mods: true,
                fds_bios_override: Some(&TEST_FDS_BIOS),
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?;
        let input = loaded.backend.audio_discovery_input().unwrap();
        let repeated = loaded.backend.audio_discovery_input().unwrap();
        assert!(Arc::ptr_eq(&input, &repeated));
        assert_eq!(input.system, Some(zeff_emu_common::system::System::Nes));
        assert_eq!(input.bytes[0x10], 0x44);
        assert_eq!(std::fs::read(&path)?, raw);

        let provenance = input.provenance.as_ref().unwrap();
        assert_eq!(provenance.source.kind, "direct_cartridge_file");
        assert_eq!(provenance.source.sha256, zeff_firmware::sha256_hex(&raw));
        assert_eq!(provenance.source.len, raw.len());
        assert_eq!(provenance.transforms.len(), 1);
        assert_eq!(
            provenance.transforms[0].input_sha256,
            zeff_firmware::sha256_hex(&raw)
        );
        assert_eq!(
            provenance.transforms[0].output_sha256,
            zeff_firmware::sha256_hex(&input.bytes)
        );

        let loaded_manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        let direct_manifest = crate::audio_discovery::scan(
            zeff_emu_common::system::System::Nes,
            &raw,
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(loaded_manifest.scan.status, ScanStatus::Complete);
        assert_eq!(direct_manifest.status, ScanStatus::Complete);
        assert_eq!(
            loaded_manifest.source.as_ref().unwrap().sha256,
            zeff_firmware::sha256_hex(&raw)
        );
        assert_eq!(
            loaded_manifest.scan.media.sha256.as_deref(),
            Some(zeff_firmware::sha256_hex(&input.bytes).as_str())
        );
        assert_eq!(
            direct_manifest.media.sha256.as_deref(),
            Some(zeff_firmware::sha256_hex(&raw).as_str())
        );
        assert_ne!(
            loaded_manifest.scan.media.sha256,
            direct_manifest.media.sha256
        );

        let mut worker = crate::emu_thread::EmuThread::spawn(loaded.backend, false);
        let worker_input = worker.audio_discovery_input().unwrap();
        assert!(Arc::ptr_eq(&input, &worker_input));
        worker.shutdown();
        Ok(())
    })
}

#[test]
fn audio_discovery_retains_complete_gb_and_modded_nes_cartridge_media() -> anyhow::Result<()> {
    let gb = build_gb_test_rom();
    let gb_path = std::path::Path::new("preloaded.gb");
    let gb_backend = load_backend_from_rom_source(
        ActiveSystem::GameBoy,
        gb_path,
        gb_path,
        Some(gb.clone()),
        BackendLoadConfig {
            gb_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )?
    .backend;
    let gb_input = gb_backend.audio_discovery_input().unwrap();
    assert_eq!(gb_input.system, Some(zeff_emu_common::system::System::Gb));
    assert_eq!(&*gb_input.bytes, gb);
    let gb_source = &gb_input.provenance.as_ref().unwrap().source;
    assert_eq!(gb_source.kind, "preloaded_cartridge_bytes");
    assert_eq!(gb_source.sha256, zeff_firmware::sha256_hex(&gb));

    let directory = crate::test_support::test_directory("audio-loaded-nes-cartridge")?;
    crate::mods::with_test_mods_root(directory.path(), || -> anyhow::Result<()> {
        let raw = build_nes_test_rom();
        let path = directory.path().join("fixture.nes");
        std::fs::write(&path, &raw)?;
        let mods_dir = crate::mods::mods_dir_for_rom(ActiveSystem::Nes, crc32fast::hash(&raw));
        std::fs::create_dir_all(&mods_dir)?;
        let patch = b"PATCH\x00\x00\x10\x00\x01\x44EOF";
        std::fs::write(mods_dir.join("music.ips"), patch)?;
        crate::mods::save_mod_config(
            &mods_dir,
            &[crate::mods::ModEntry {
                filename: "music.ips".into(),
                enabled: true,
                target: None,
            }],
        );

        let loaded = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &path,
            &path,
            None,
            BackendLoadConfig {
                apply_mods: true,
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?;
        let input = loaded.backend.audio_discovery_input().unwrap();
        let same_input = loaded.backend.audio_discovery_input().unwrap();
        assert!(std::sync::Arc::ptr_eq(&input, &same_input));
        assert_eq!(input.system, Some(zeff_emu_common::system::System::Nes));
        assert_eq!(&input.bytes[..16], &raw[..16]);
        assert_eq!(input.bytes[16], 0x44);
        assert_eq!(std::fs::read(&path)?, raw);
        let provenance = input.provenance.as_ref().unwrap();
        assert_eq!(provenance.source.kind, "direct_cartridge_file");
        assert_eq!(provenance.source.sha256, zeff_firmware::sha256_hex(&raw));
        assert_eq!(provenance.transforms.len(), 1);
        assert_eq!(
            provenance.transforms[0].input_sha256,
            zeff_firmware::sha256_hex(&raw)
        );
        assert_eq!(
            provenance.transforms[0].output_sha256,
            zeff_firmware::sha256_hex(&input.bytes)
        );

        let mut worker = crate::emu_thread::EmuThread::spawn(loaded.backend, false);
        let worker_input = worker.audio_discovery_input().unwrap();
        assert!(std::sync::Arc::ptr_eq(&input, &worker_input));
        worker.shutdown();
        Ok(())
    })
}

#[test]
fn audio_discovery_retains_ordered_success_and_partial_failure_mod_receipts() -> anyhow::Result<()>
{
    let directory = crate::test_support::test_directory("audio-loaded-mods")?;
    crate::mods::with_test_mods_root(directory.path(), || -> anyhow::Result<()> {
        let raw = gba_fixture();
        let path = directory.path().join("fixture.gba");
        std::fs::write(&path, &raw)?;
        let mods_dir =
            crate::mods::mods_dir_for_rom(ActiveSystem::GameBoyAdvance, crc32fast::hash(&raw));
        std::fs::create_dir_all(&mods_dir)?;
        let first = b"PATCH\x00\x03\x10\x00\x01\x22EOF";
        let second = b"PATCH\x00\x03\x10\x00\x01\x44EOF";
        let partial = b"PATCH\x00\x03\x11\x00\x01\x33\x01";
        let entries = ["b-first.ips", "a-second.ips", "c-partial.ips"].map(|filename| {
            crate::mods::ModEntry {
                filename: filename.into(),
                enabled: true,
                target: None,
            }
        });
        for (entry, patch) in
            entries
                .iter()
                .zip([first.as_slice(), second.as_slice(), partial.as_slice()])
        {
            std::fs::write(mods_dir.join(&entry.filename), patch)?;
        }
        crate::mods::save_mod_config(&mods_dir, &entries);
        let loaded = load_backend_from_rom_source(
            ActiveSystem::GameBoyAdvance,
            &path,
            &path,
            None,
            BackendLoadConfig {
                apply_mods: true,
                ..config()
            },
        )?;
        let input = loaded.backend.audio_discovery_input().unwrap();
        assert_eq!(input.bytes[0x310..0x312], [0x44, 0x33]);
        std::fs::write(mods_dir.join(&entries[0].filename), b"replaced after load")?;
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(
            manifest.source.as_ref().unwrap().sha256,
            zeff_firmware::sha256_hex(&raw)
        );
        assert_eq!(
            manifest.scan.media.sha256.as_deref(),
            Some(const_hex::encode(loaded.backend.rom_hash()).as_str())
        );
        let steps = manifest.transforms.unwrap();
        assert_eq!(
            steps
                .iter()
                .map(|step| step.filename.as_str())
                .collect::<Vec<_>>(),
            ["b-first.ips", "a-second.ips", "c-partial.ips"]
        );
        assert_eq!(
            steps[0].patch_sha256.as_deref(),
            Some(zeff_firmware::sha256_hex(first).as_str())
        );
        assert_eq!(steps[0].input_sha256, zeff_firmware::sha256_hex(&raw));
        assert_eq!(steps[1].input_sha256, steps[0].output_sha256);
        assert_eq!(steps[2].input_sha256, steps[1].output_sha256);
        assert!(matches!(
            steps[2].outcome,
            crate::mods::ModApplicationOutcome::Failed { .. }
        ));
        assert_eq!(steps[2].output_sha256, manifest.scan.media.sha256.unwrap());
        assert_eq!(std::fs::read(path)?, raw);
        Ok(())
    })
}

#[test]
fn audio_discovery_reuses_authenticated_zip_member_and_does_not_guess_missing_identity()
-> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("audio-loaded-zip")?;
    let path = directory.path().join("fixture.zip");
    let raw = gba_fixture();
    let archive = crate::test_support::write_zip(
        &path,
        &[("unrelated.txt", b"ignore"), ("music/fixture.gba", &raw)],
    )?;
    let rom_path = path.join("music/fixture.gba");
    let extracted = crate::rom_archive::extract_authenticated_bounded_zip_member(
        &path,
        Some(&rom_path),
        "gba",
        128 * 1024 * 1024,
        32 * 1024 * 1024,
    )?;
    let witness = extracted.witness;
    let loaded = load_backend_from_rom_source(
        ActiveSystem::GameBoyAdvance,
        &path,
        &rom_path,
        Some(extracted.bytes),
        BackendLoadConfig {
            authenticated_zip_member: Some(witness.clone()),
            ..config()
        },
    )?;
    let input = loaded.backend.audio_discovery_input().unwrap();
    std::fs::write(&path, b"archive replaced after load")?;
    let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert_eq!(manifest.scan.candidates.len(), 1);
    let source = manifest.source.unwrap();
    assert_eq!(source.kind, "zip_member");
    assert_eq!(source.sha256, zeff_firmware::sha256_hex(&raw));
    assert_eq!(
        source.container.unwrap().sha256,
        zeff_firmware::sha256_hex(&archive)
    );
    assert_eq!(source.selected_member.unwrap().name, "music/fixture.gba");

    let unverified = load_backend_from_rom_source(
        ActiveSystem::GameBoyAdvance,
        &path,
        &rom_path,
        Some(raw),
        BackendLoadConfig {
            authenticated_zip_member: Some(witness),
            ..config()
        },
    )?;
    let input = unverified.backend.audio_discovery_input().unwrap();
    let provenance = input.provenance.as_ref().unwrap();
    assert_eq!(provenance.source.kind, "preloaded_gba_bytes");
    assert!(provenance.source.container.is_none());
    assert!(provenance.source.selected_member.is_none());
    Ok(())
}

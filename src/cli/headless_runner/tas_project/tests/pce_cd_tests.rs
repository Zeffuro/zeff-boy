use rars::rar50::{ArchiveEntry as RarArchiveEntry, Rar50Writer, WriterOptions};
use rars::{ArchiveVersion, EntrySource, FeatureSet};
use std::io::Write;
use zeff_emu_common::replay::ReplayPlayer;

use super::*;
use crate::emu_backend::loader::DirectPceCdTasExecutionLoader;
use crate::tas_project::{TasControllerInput, TasInputFrame};

mod arcade_multitap;
mod archive_ppf;
mod memory_base_multitap;
mod multicue;
mod ppf_multitap;

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_cue_multitap() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-multitap")?;
    let cue_path = directory.path().join("disc.cue");
    std::fs::write(
        directory.path().join("disc.bin"),
        vec![0xBC; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
    )?;
    std::fs::write(
        &cue_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        cue_path.clone(),
        system_card,
        firmware_sha256,
    );
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        cue_path,
        system_card,
        firmware_sha256,
    );
    verifies_and_exports_direct_pce_cd_multitap(directory.path(), loader)
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_iso_multitap() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-iso-multitap")?;
    let source_path = directory.path().join("disc.iso");
    std::fs::write(
        &source_path,
        vec![0xBD; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES],
    )?;
    std::fs::write(
        directory.path().join("disc.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        firmware_sha256,
    );
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        source_path,
        system_card,
        firmware_sha256,
    );
    verifies_and_exports_direct_pce_cd_multitap(directory.path(), loader)
}

fn verifies_and_exports_direct_pce_cd_multitap(
    directory: &std::path::Path,
    loader: DirectPceCdTasExecutionLoader,
) -> Result<()> {
    let mut project = loader.create_project()?;
    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 1,
                dpad: 8,
            },
            TasControllerInput {
                buttons: 2,
                dpad: 4,
            },
            TasControllerInput {
                buttons: 4,
                dpad: 2,
            },
            TasControllerInput {
                buttons: 8,
                dpad: 1,
            },
            TasControllerInput {
                buttons: 3,
                dpad: 5,
            },
        ],
        ..Default::default()
    };
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    let project_path = directory.join("movie.ztas");
    let export_path = directory.join("movie.zrpl");
    project.save_atomic(&project_path)?;
    run_tas_project_headless_with_plan(
        PrivateTasExecutionLoader::DirectPceCd(loader),
        &project_path,
        "main",
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;
    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    let frames = replay.peek_joypad_frames(0, 1);
    let frame = &frames[0];
    assert_eq!(
        [
            frame.buttons,
            frame.buttons_p2,
            frame.buttons_p3,
            frame.buttons_p4,
            frame.buttons_p5
        ],
        [1, 2, 4, 8, 3]
    );
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_chd_multitap() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-chd-multitap")?;
    let source_path = directory.path().join("disc.chd");
    crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&source_path)?;
    let mut bytes = std::fs::read(&source_path)?;
    bytes[4 * 2_448] ^= 0x34;
    std::fs::write(&source_path, bytes)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        firmware_sha256,
    );
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        source_path,
        system_card,
        firmware_sha256,
    );
    verifies_and_exports_direct_pce_cd_multitap(directory.path(), loader)
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

fn verifies_and_exports_direct_pce_cd(
    directory: &std::path::Path,
    loader: DirectPceCdTasExecutionLoader,
    memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode,
    arcade_card_mode: zeff_pce_core::hardware::PceArcadeCardMode,
) -> Result<()> {
    let project_path = directory.join("movie.ztas");
    let export_path = directory.join("movie.zrpl");
    assert_eq!(
        loader
            .load_fresh_backend()?
            .pce()
            .expect("direct PC Engine CD fixture must load a PC Engine backend")
            .memory_base_mode(),
        memory_base_mode
    );
    assert_eq!(
        loader
            .load_fresh_backend()?
            .pce()
            .expect("direct PC Engine CD fixture must load a PC Engine backend")
            .arcade_card_mode(),
        arcade_card_mode
    );
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.set_input_range(
            "main",
            0,
            1,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x01,
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
    project.save_atomic(&project_path)?;

    run_tas_project_headless_with_plan(
        PrivateTasExecutionLoader::DirectPceCd(loader),
        &project_path,
        "main",
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(replay.total_frames(), 1);
    assert_eq!(replay.peek_joypad_frames(0, 1)[0].buttons, 0x01);
    assert_eq!(replay.peek_joypad_frames(0, 1)[0].dpad, 0x04);
    Ok(())
}

fn write_rar_fixture(path: &std::path::Path, fill: u8) -> Result<()> {
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
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_zip_fixture(path: &std::path::Path, fill: u8) -> Result<()> {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path)?);
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

#[test]
fn native_cli_two_pass_verifies_and_exports_unique_rar_card_profiles() -> Result<()> {
    for (fill, memory_base, arcade) in [
        (0x81, false, false),
        (0x82, false, true),
        (0x83, true, false),
    ] {
        let directory = test_directory(&format!("tas-cli-pce-cd-rar-{fill:02x}"))?;
        let source_path = directory.path().join("disc.rar");
        write_rar_fixture(&source_path, fill)?;
        let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
        let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
            source_path,
            system_card,
            zeff_firmware::sha256_bytes(system_card),
        );
        let normalized_disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("RAR fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("RAR fixture must mount a normalized disc");
        let _memory_base_catalog = memory_base.then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                normalized_disc_sha256,
            )
        });
        let _arcade_catalog = arcade.then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                normalized_disc_sha256,
            )
        });
        verifies_and_exports_direct_pce_cd(
            directory.path(),
            loader,
            if memory_base {
                zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
            } else {
                zeff_pce_core::hardware::PceMemoryBaseMode::Disabled
            },
            if arcade {
                zeff_pce_core::hardware::PceArcadeCardMode::Enabled
            } else {
                zeff_pce_core::hardware::PceArcadeCardMode::Disabled
            },
        )?;
    }
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_and_exports_unique_zip_card_profiles() -> Result<()> {
    for (fill, memory_base, arcade) in [
        (0x91, false, false),
        (0x92, false, true),
        (0x93, true, false),
    ] {
        let directory = test_directory(&format!("tas-cli-pce-cd-zip-{fill:02x}"))?;
        let source_path = directory.path().join("disc.zip");
        write_zip_fixture(&source_path, fill)?;
        let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
        let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
            source_path,
            system_card,
            zeff_firmware::sha256_bytes(system_card),
        );
        let normalized_disc_sha256 = loader
            .load_fresh_backend()?
            .pce()
            .expect("ZIP fixture must load a PC Engine backend")
            .normalized_disc_hash()
            .expect("ZIP fixture must mount a normalized disc");
        let _memory_base_catalog = memory_base.then(|| {
            crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
                normalized_disc_sha256,
            )
        });
        let _arcade_catalog = arcade.then(|| {
            crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
                normalized_disc_sha256,
            )
        });
        verifies_and_exports_direct_pce_cd(
            directory.path(),
            loader,
            if memory_base {
                zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
            } else {
                zeff_pce_core::hardware::PceMemoryBaseMode::Disabled
            },
            if arcade {
                zeff_pce_core::hardware::PceArcadeCardMode::Enabled
            } else {
                zeff_pce_core::hardware::PceArcadeCardMode::Disabled
            },
        )?;
    }
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_pce_cd_chd_memory_base_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-chd-memory-base")?;
    let source_path = directory.path().join("disc.chd");
    crate::emu_backend::pce_cd_chd::write_synthetic_uncompressed_v5_chd(&source_path)?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path,
        system_card,
        zeff_firmware::sha256_bytes(system_card),
    );
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
    verifies_and_exports_direct_pce_cd(
        directory.path(),
        loader,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    )
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_pce_cd_iso_memory_base_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-iso-memory-base")?;
    let source_path = directory.path().join("disc.iso");
    std::fs::write(&source_path, vec![0x5A; 4 * 2048])?;
    std::fs::write(
        directory.path().join("disc.cue"),
        b"FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path,
        system_card,
        zeff_firmware::sha256_bytes(system_card),
    );
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
    verifies_and_exports_direct_pce_cd(
        directory.path(),
        loader,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    )
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_pce_cd_ppf_memory_base_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-ppf-memory-base")?;
    let source_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0xD1; 4 * 2048])?;
    std::fs::write(
        &source_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base_loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        system_card_sha256,
    );
    let source_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _memory_base_catalog =
        crate::emu_backend::pce_profiles::register_test_memory_base_catalog_hash(
            source_disc_sha256,
        );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![("memory-base.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?;
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        source_path,
        system_card,
        system_card_sha256,
        stack,
    );
    verifies_and_exports_direct_pce_cd(
        directory.path(),
        loader,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    )
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_pce_cd_ppf_arcade_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-ppf-arcade")?;
    let source_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0xD2; 4 * 2048])?;
    std::fs::write(
        &source_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let system_card_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base_loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        system_card_sha256,
    );
    let source_disc_sha256 = base_loader
        .load_fresh_backend()?
        .pce()
        .expect("direct PC Engine CD fixture must load a PC Engine backend")
        .normalized_disc_hash()
        .expect("direct PC Engine CD fixture must mount a normalized disc");
    let _arcade_catalog = crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(
        source_disc_sha256,
    );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![("arcade.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?;
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_and_ppf_stack(
        source_path,
        system_card,
        system_card_sha256,
        stack,
    );
    verifies_and_exports_direct_pce_cd(
        directory.path(),
        loader,
        zeff_pce_core::hardware::PceMemoryBaseMode::Disabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Enabled,
    )
}

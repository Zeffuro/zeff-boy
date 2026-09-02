use zeff_emu_common::replay::ReplayPlayer;

use super::*;
use crate::emu_backend::loader::DirectPceCdTasExecutionLoader;
use crate::tas_project::{TasControllerInput, TasInputFrame};

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}

fn verifies_and_exports_direct_pce_cd_memory_base(
    directory: &std::path::Path,
    loader: DirectPceCdTasExecutionLoader,
) -> Result<()> {
    let project_path = directory.join("movie.ztas");
    let export_path = directory.join("movie.zrpl");
    assert_eq!(
        loader
            .load_fresh_backend()?
            .pce()
            .expect("direct PC Engine CD fixture must load a PC Engine backend")
            .memory_base_mode(),
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled
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
    verifies_and_exports_direct_pce_cd_memory_base(directory.path(), loader)
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
    verifies_and_exports_direct_pce_cd_memory_base(directory.path(), loader)
}

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_pce_cd_ppf_memory_base_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-ppf-memory-base")?;
    let source_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0x5A; 4 * 2048])?;
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
    verifies_and_exports_direct_pce_cd_memory_base(directory.path(), loader)
}

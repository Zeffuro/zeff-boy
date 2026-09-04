use zeff_emu_common::replay::ReplayPlayer;

use super::*;
use crate::emu_backend::loader::DirectGbcTasExecutionLoader;
use crate::tas_project::{TasControllerInput, TasInputFrame};

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_gbc_input() -> Result<()> {
    let directory = test_directory("tas-cli-gbc")?;
    let rom_path = directory.path().join("game.gbc");
    let save_path = directory.path().join("game.sav");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom[0x143] = 0xC0;
    rom[0x147] = 0x08;
    rom[0x149] = 0x02;
    let sidecar = vec![0xC5; 8 * 1024];
    std::fs::write(&rom_path, rom)?;
    std::fs::write(&save_path, &sidecar)?;
    let loader = DirectGbcTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let mut project = loader.create_project()?;
    assert_eq!(
        project.identity().persistent_state,
        crate::tas_project::TasExternalIdentity::Absent
    );
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 1)?;
        edit.set_input_range(
            "main",
            0,
            2,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x02,
                        dpad: 0x01,
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

    run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    let saved = TasProject::load(&project_path)?;
    assert!(saved.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(replay.total_frames(), 2);
    assert_eq!(replay.peek_joypad_frames(0, 2)[0].buttons, 0x02);
    assert_eq!(replay.peek_joypad_frames(0, 2)[0].dpad, 0x01);
    assert_eq!(std::fs::read(save_path)?, sidecar);
    Ok(())
}

#[test]
fn native_cli_replays_project_owned_gbc_sram_without_touching_the_sidecar() -> Result<()> {
    let directory = test_directory("tas-cli-gbc-battery")?;
    let rom_path = directory.path().join("game.gbc");
    let save_path = directory.path().join("game.sav");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom[0x143] = 0xC0;
    rom[0x147] = 0x09;
    rom[0x148] = 0x00;
    rom[0x149] = 0x02;
    let initial_sram = (0..8 * 1024)
        .map(|index| (index as u8).wrapping_mul(17).wrapping_add(11))
        .collect::<Vec<_>>();
    std::fs::write(&rom_path, rom)?;
    std::fs::write(&save_path, initial_sram)?;
    let loader = DirectGbcTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let project = loader.create_project()?;
    project.save_atomic(&project_path)?;

    let changed_sidecar = vec![0xC5; 8 * 1024];
    std::fs::write(&save_path, &changed_sidecar)?;
    run_tas_project_headless(
        &rom_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    assert_eq!(ReplayPlayer::load(&export_path)?.total_frames(), 1);
    assert_eq!(std::fs::read(save_path)?, changed_sidecar);
    Ok(())
}

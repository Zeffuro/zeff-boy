use zeff_emu_common::replay::ReplayPlayer;

use super::*;
use crate::emu_backend::loader::DirectWsTasExecutionLoader;
use crate::tas_project::{TasControllerInput, TasInputFrame};

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_ws_keypad_input() -> Result<()> {
    let directory = test_directory("tas-cli-ws")?;
    let rom_path = directory.path().join("game.wsc");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    std::fs::write(&rom_path, direct_ws_rom())?;
    let loader = DirectWsTasExecutionLoader::new(rom_path.clone());
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| {
        edit.insert_frames("main", 1, 1)?;
        edit.set_input_range(
            "main",
            0,
            2,
            TasInputFrame {
                players: [
                    TasControllerInput {
                        buttons: 0x81,
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
    assert_eq!(
        replay.peek_joypad_frames(0, 2),
        vec![
            zeff_emu_common::replay::ReplayJoypadFrame {
                buttons: 0x81,
                dpad: 0x04,
                ..Default::default()
            };
            2
        ]
    );
    Ok(())
}

#[test]
fn native_cli_two_pass_verifies_direct_ws_rtc_ticking() -> Result<()> {
    let directory = test_directory("tas-cli-ws-rtc")?;
    let rom_path = directory.path().join("clock.wsc");
    let project_path = directory.path().join("clock.ztas");
    let export_path = directory.path().join("clock.zrpl");
    let mut rom = direct_ws_rom();
    let footer = rom.len() - 10;
    rom[footer + 7] = 1;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    std::fs::write(&rom_path, rom)?;
    let loader = DirectWsTasExecutionLoader::new(rom_path.clone());
    let mut project = loader.create_project()?;
    project.edit_transaction(|edit| edit.insert_frames("main", 1, 90))?;
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
    assert!(matches!(
        saved.identity().rtc_state,
        crate::tas_project::TasExternalIdentity::ExternalSha256(_)
    ));
    assert_eq!(ReplayPlayer::load(&export_path)?.total_frames(), 91);
    Ok(())
}

fn direct_ws_rom() -> Vec<u8> {
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer..].fill(0);
    rom[footer + 1] = 1;
    rom[footer + 4] = 0x01;
    rom[footer + 5] = 0;
    rom[footer + 6] = 1;
    rom[footer + 7] = 0;
    let checksum = zeff_ws_core::hardware::cartridge::compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

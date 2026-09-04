use zeff_emu_common::replay::ReplayPlayer;

use super::*;
use crate::emu_backend::loader::DirectPceTasExecutionLoader;
use crate::tas_project::{TasControllerInput, TasInputFrame};
use crate::test_support::write_zip;

fn multitap_input() -> TasInputFrame {
    TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0x01,
                dpad: 0x08,
            },
            TasControllerInput {
                buttons: 0x02,
                dpad: 0x04,
            },
            TasControllerInput {
                buttons: 0x04,
                dpad: 0x02,
            },
            TasControllerInput {
                buttons: 0x08,
                dpad: 0x01,
            },
            TasControllerInput {
                buttons: 0x0F,
                dpad: 0x05,
            },
        ],
        ..Default::default()
    }
}

fn pce_rom() -> Vec<u8> {
    let mut rom = vec![0; zeff_pce_core::hardware::PCEAS_HEADER_LEN];
    rom[0] = 1;
    rom.extend(vec![0xEA; 0x2000]);
    rom
}

#[test]
fn native_cli_verifies_and_exports_direct_pce_multitap_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-multitap")?;
    let rom_path = directory.path().join("multitap.pce");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    std::fs::write(&rom_path, pce_rom())?;

    let loader = DirectPceTasExecutionLoader::new_multitap(rom_path.clone());
    let mut project = loader.create_project()?;
    let input = multitap_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
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

    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(
        replay.peek_joypad_frames(0, 1).as_slice(),
        &[zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x08,
            buttons_p2: 0x02,
            dpad_p2: 0x04,
            buttons_p3: 0x04,
            dpad_p3: 0x02,
            buttons_p4: 0x08,
            dpad_p4: 0x01,
            buttons_p5: 0x0F,
            dpad_p5: 0x05,
            ..Default::default()
        }]
    );
    Ok(())
}

#[test]
fn native_cli_verifies_selected_zip_pce_multitap_input() -> Result<()> {
    let directory = test_directory("tas-cli-pce-multitap-zip")?;
    let archive_path = directory.path().join("games.zip");
    let first = pce_rom();
    let mut selected = pce_rom();
    *selected.last_mut().unwrap() ^= 1;
    write_zip(
        &archive_path,
        &[("first.pce", &first), ("folder/selected.pce", &selected)],
    )?;
    let selected_path = archive_path.join("folder/selected.pce");
    let loader =
        DirectPceTasExecutionLoader::new_zip_multitap(archive_path.clone(), Some(selected_path));
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let mut project = loader.create_project()?;
    let input = multitap_input();
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
    project.save_atomic(&project_path)?;

    run_tas_project_headless(
        &archive_path,
        Vec::new(),
        &HeadlessOptions {
            tas_project_path: Some(project_path.clone()),
            tas_export_path: Some(export_path.clone()),
            ..HeadlessOptions::default()
        },
    )?;

    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(
        replay.peek_joypad_frames(0, 1).as_slice(),
        &[zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x08,
            buttons_p2: 0x02,
            dpad_p2: 0x04,
            buttons_p3: 0x04,
            dpad_p3: 0x02,
            buttons_p4: 0x08,
            dpad_p4: 0x01,
            buttons_p5: 0x0F,
            dpad_p5: 0x05,
            ..Default::default()
        }]
    );
    Ok(())
}

#[test]
fn native_cli_verifies_and_exports_direct_supergrafx_input() -> Result<()> {
    let directory = test_directory("tas-cli-supergrafx")?;
    let rom_path = directory.path().join("supergrafx.pce");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let mut rom = vec![0xEA; 0x2000];
    rom[0] = 0x42;
    rom[0x1FFE] = 0;
    rom[0x1FFF] = 0;
    std::fs::write(&rom_path, rom)?;

    let loader = DirectPceTasExecutionLoader::new(rom_path.clone());
    assert_eq!(
        loader
            .load_fresh_backend()?
            .0
            .pce()
            .unwrap()
            .hardware_topology(),
        zeff_pce_core::hardware::PceHardwareTopology::SuperGrafx
    );
    let mut project = loader.create_project()?;
    let input = TasInputFrame {
        players: [
            TasControllerInput {
                buttons: 0x01,
                dpad: 0x02,
            },
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
            TasControllerInput::default(),
        ],
        ..Default::default()
    };
    project.edit_transaction(|edit| edit.set_input_range("main", 0, 1, input))?;
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

    assert!(TasProject::load(&project_path)?.verification_is_current("main")?);
    let replay = ReplayPlayer::load(&export_path)?;
    assert_eq!(
        replay.peek_joypad_frames(0, 1).as_slice(),
        &[zeff_emu_common::replay::ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x02,
            ..Default::default()
        }]
    );
    Ok(())
}

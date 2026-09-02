use zeff_emu_common::replay::ReplayPlayer;

use super::*;
use crate::emu_backend::loader::DirectGbTasExecutionLoader;

#[test]
fn native_cli_replays_project_owned_gb_sram_without_touching_the_sidecar() -> Result<()> {
    let directory = test_directory("tas-cli-gb-battery")?;
    let rom_path = directory.path().join("game.gb");
    let save_path = directory.path().join("game.sav");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom.resize(512 * 1024, 0);
    rom[0x147] = 0x03;
    rom[0x148] = 0x04;
    rom[0x149] = 0x03;
    let initial_sram = (0..32 * 1024)
        .map(|index| (index as u8).wrapping_mul(13).wrapping_add(5))
        .collect::<Vec<_>>();
    std::fs::write(&rom_path, rom)?;
    std::fs::write(&save_path, initial_sram)?;
    let loader = DirectGbTasExecutionLoader::new(rom_path.clone(), Vec::new());
    let project = loader.create_project()?;
    project.save_atomic(&project_path)?;

    let changed_sidecar = vec![0xA7; 32 * 1024];
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

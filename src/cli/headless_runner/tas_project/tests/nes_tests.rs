use zeff_emu_common::replay::ReplayPlayer;

use super::*;

#[test]
fn native_cli_replays_project_owned_nes_sram_without_touching_the_sidecar() -> Result<()> {
    let directory = test_directory("tas-cli-nes-battery")?;
    let rom_path = directory.path().join("game.nes");
    let save_path = rom_path.with_extension("sav");
    let project_path = directory.path().join("movie.ztas");
    let export_path = directory.path().join("movie.zrpl");
    let rom = crate::test_support::build_nes_battery_test_rom();
    let emulator =
        zeff_nes_core::emulator::Emulator::new(&rom, zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE)?;
    let initial_sram = vec![
        0x5A;
        emulator
            .dump_battery_sram()
            .expect("battery fixture must expose SRAM")
            .len()
    ];
    std::fs::write(&rom_path, rom)?;
    std::fs::write(&save_path, initial_sram)?;
    let loader = DirectNesTasExecutionLoader::new(rom_path.clone(), Vec::new());
    loader.create_project()?.save_atomic(&project_path)?;

    let changed_sidecar = std::fs::read(&save_path)?
        .into_iter()
        .map(|byte| byte ^ 0xFF)
        .collect::<Vec<_>>();
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

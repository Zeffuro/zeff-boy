mod app;
mod audio;
mod audio_recorder;
mod cheats;
mod cli;
mod debug;
mod emu_backend;
mod emu_thread;
mod graphics;
mod input;
mod libretro_metadata;
mod settings;
mod ui;

use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::settings::Settings;
use env_logger::Env;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut settings = Settings::load_or_default();
    let args = cli::parse_args().map_err(|e| anyhow::anyhow!("{e}"))?;

    if let Some(mode) = args.mode_override {
        settings.hardware_mode_preference = mode;
    }

    if let Some(headless_opts) = args.headless {
        let path_arg = args
            .rom_path
            .ok_or_else(|| anyhow::anyhow!("--headless requires a ROM path"))?;
        return cli::run_headless(
            Path::new(&path_arg),
            settings.hardware_mode_preference,
            &headless_opts,
        )
        .map_err(|e| anyhow::anyhow!("{e}"));
    }

    let backend = args
        .rom_path
        .map(|path_arg| -> anyhow::Result<EmuBackend> {
            let path = Path::new(&path_arg);
            let system = ActiveSystem::from_path(path).ok_or_else(|| {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("(none)");
                anyhow::anyhow!(
                    "Unsupported file type '.{ext}'. Supported extensions: {}",
                    ActiveSystem::supported_extensions()
                )
            })?;
            match system {
                ActiveSystem::GameBoy => {
                    let emu = zeff_gb_core::emulator::Emulator::from_rom_with_mode(
                        path,
                        settings.hardware_mode_preference,
                    )
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                    Ok(EmuBackend::from_gb(emu))
                }
                ActiveSystem::Nes => {
                    let rom_data = std::fs::read(path)
                        .map_err(|e| anyhow::anyhow!("Failed to read NES ROM: {e}"))?;
                    let emu = zeff_nes_core::emulator::Emulator::new(
                        &rom_data,
                        path.to_path_buf(),
                        48000.0,
                    )
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                    Ok(EmuBackend::from_nes(emu))
                }
            }
        })
        .transpose()?;

    app::run(backend, settings)?;

    Ok(())
}

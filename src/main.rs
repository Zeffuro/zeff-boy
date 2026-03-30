mod app;
mod audio;
mod audio_recorder;
mod camera;
mod cheats;
mod cli;
mod debug;
mod emu_backend;
mod emu_thread;
mod graphics;
mod input;
mod libretro_common;
mod libretro_metadata;
mod save_paths;
mod settings;
mod ui;

use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::settings::Settings;
use anyhow::Context;
use env_logger::Env;
use std::path::Path;

const NES_SAMPLE_RATE: f64 = 48000.0;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut settings = Settings::load_or_default();
    let args = cli::parse_args()?;

    if let Some(mode) = args.mode_override {
        settings.emulation.hardware_mode_preference = mode;
    }

    if let Some(headless_opts) = args.headless {
        let rom_path_arg = args
            .rom_path
            .context("--headless requires a ROM path")?;
        return cli::run_headless(
            Path::new(&rom_path_arg),
            settings.emulation.hardware_mode_preference,
            &headless_opts,
        );
    }

    let backend = args
        .rom_path
        .map(|rom_path_arg| create_backend(&rom_path_arg, &settings))
        .transpose()?;

    app::run(backend, settings)?;

    Ok(())
}

fn create_backend(rom_path_arg: &str, settings: &Settings) -> anyhow::Result<EmuBackend> {
    let path = Path::new(rom_path_arg);

    let is_zip = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));

    let (rom_path, preloaded_data) = if is_zip {
        let (virtual_path, data) = app::extract_rom_from_zip(path)?;
        log::info!(
            "Extracted ROM '{}' ({} bytes) from ZIP",
            virtual_path.file_name().unwrap_or_default().to_string_lossy(),
            data.len()
        );
        (virtual_path, Some(data))
    } else {
        (path.to_path_buf(), None)
    };

    let system = ActiveSystem::from_path(&rom_path).ok_or_else(|| {
        let ext = rom_path
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
            let rom_data = preloaded_data.map_or_else(
                || std::fs::read(path).context("Failed to read ROM"),
                Ok,
            )?;
            let mut emu = zeff_gb_core::emulator::Emulator::from_rom_data(
                &rom_data,
                settings.emulation.hardware_mode_preference,
            )?;
            if let Some(sram_path) = emu_backend::gb::try_load_battery_sram(&mut emu, &rom_path)
                .unwrap_or_else(|e| { log::warn!("Failed to load battery save: {e}"); None })
            {
                log::info!("Loaded battery save from {}", sram_path);
            }
            Ok(EmuBackend::from_gb(emu, rom_path))
        }
        ActiveSystem::Nes => {
            let rom_data = preloaded_data.map_or_else(
                || std::fs::read(path).context("Failed to read ROM"),
                Ok,
            )?;
            let mut emu = zeff_nes_core::emulator::Emulator::new(
                &rom_data,
                NES_SAMPLE_RATE,
            )?;
            if let Some(sram_path) = emu_backend::nes::try_load_battery_sram(&mut emu, &rom_path)
                .unwrap_or_else(|e| { log::warn!("Failed to load battery save: {e}"); None })
            {
                log::info!("Loaded battery save from {}", sram_path);
            }
            Ok(EmuBackend::from_nes(emu, rom_path))
        }
    }
}

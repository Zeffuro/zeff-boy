#![cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, unused_imports, unused_variables)
)]

mod app;
mod audio;
mod audio_recorder;
mod bps;
mod camera;
mod cheats;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
mod debug;
mod emu_backend;
mod emu_core_trait;
mod emu_thread;
mod graphics;
mod input;
mod ips;
mod libretro_common;
mod libretro_metadata;
mod mods;
mod platform;
mod save_paths;
mod settings;
mod ui;
mod ups;

#[cfg(not(target_arch = "wasm32"))]
use crate::emu_backend::{ActiveSystem, EmuBackend};
#[cfg(not(target_arch = "wasm32"))]
use crate::settings::Settings;
#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    platform::init_logging();

    let mut settings = Settings::load_or_default();
    let args = cli::parse_args()?;

    if let Some(mode) = args.mode_override {
        settings.emulation.hardware_mode_preference = mode;
    }

    if let Some(headless_opts) = args.headless {
        let rom_path_arg = args.rom_path.context("--headless requires a ROM path")?;
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

#[cfg(not(target_arch = "wasm32"))]
fn log_sram_result(result: anyhow::Result<Option<String>>) {
    match result {
        Ok(Some(path)) => log::info!("Loaded battery save from {path}"),
        Ok(None) => {}
        Err(e) => log::warn!("Failed to load battery save: {e}"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
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
            virtual_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
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

    let rom_data =
        preloaded_data.map_or_else(|| std::fs::read(path).context("Failed to read ROM"), Ok)?;

    match system {
        ActiveSystem::GameBoy => {
            let mut emu = zeff_gb_core::emulator::Emulator::from_rom_data(
                &rom_data,
                settings.emulation.hardware_mode_preference,
            )?;
            log_sram_result(emu_backend::gb::try_load_battery_sram(&mut emu, &rom_path));
            Ok(EmuBackend::from_gb(emu, rom_path))
        }
        ActiveSystem::Nes => {
            let mut emu = zeff_nes_core::emulator::Emulator::new(
                &rom_data,
                zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE,
            )?;
            log_sram_result(emu_backend::nes::try_load_battery_sram(&mut emu, &rom_path));
            Ok(EmuBackend::from_nes(emu, rom_path))
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    platform::init_logging();
    log::info!("zeff-boy v{} WASM starting", env!("CARGO_PKG_VERSION"));

    wasm_bindgen_futures::spawn_local(async {
        platform::init_storage().await;
        let settings = settings::Settings::load_or_default();
        app::run(None, settings).expect("app::run failed");
    });
}

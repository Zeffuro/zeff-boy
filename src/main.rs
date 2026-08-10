#![cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, unused_imports, unused_variables)
)]

mod app;
mod audio;
mod audio_recorder;
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
mod libretro_common;
mod libretro_metadata;
#[cfg(not(target_arch = "wasm32"))]
mod live_control;
mod mods;
mod patching;
mod platform;
mod save_paths;
mod settings;
mod ui;

#[cfg(not(target_arch = "wasm32"))]
use crate::emu_backend::{BackendLoadConfig, EmuBackend, load_backend_from_rom_source};
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
fn create_backend(rom_path_arg: &str, settings: &Settings) -> anyhow::Result<EmuBackend> {
    let path = Path::new(rom_path_arg);
    let (rom_path, preloaded_data, system) = app::detect_and_extract_rom(path)?;
    let loaded = load_backend_from_rom_source(
        system,
        path,
        &rom_path,
        preloaded_data,
        BackendLoadConfig {
            gb_hardware_mode_preference: settings.emulation.hardware_mode_preference,
            ..BackendLoadConfig::default()
        },
    )?;
    Ok(loaded.backend)
}

#[cfg(target_arch = "wasm32")]
fn main() {
    platform::init_logging();
    log::info!("zeff-boy v{} WASM starting", env!("CARGO_PKG_VERSION"));

    wasm_bindgen_futures::spawn_local(async {
        platform::set_boot_status(
            "Checking graphics support…",
            "zeff-boy needs browser WebGPU or WebGL2 access to render.",
        );
        if !platform::check_webgpu_support().await {
            log::error!("graphics preflight failed; not starting app");
            return;
        }

        platform::set_boot_status(
            "Loading emulator data…",
            "Preparing browser storage and settings.",
        );
        platform::init_storage().await;
        let settings = settings::Settings::load_or_default();
        platform::set_boot_status("Starting emulator UI…", "Creating the window and renderer.");
        if let Err(error) = app::run(None, settings) {
            log::error!("app::run failed: {error}");
            platform::show_boot_error(
                "zeff-boy failed to start.",
                "The emulator could not create its browser event loop.",
                &error.to_string(),
            );
        }
    });
}

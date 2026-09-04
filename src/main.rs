#![cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, unused_imports, unused_variables)
)]

#[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod app;
mod audio;
mod audio_recorder;
mod audio_tooling;
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
mod link;
#[cfg(not(target_arch = "wasm32"))]
mod live_control;
mod mods;
mod patching;
mod platform;
mod replay_execution;
mod rom_archive;
mod save_paths;
mod settings;
mod symbols;
pub mod tas_project;
#[cfg(test)]
mod test_support;
mod ui;
#[cfg(not(target_arch = "wasm32"))]
mod update;

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

    #[cfg(feature = "profile-cores")]
    if std::env::var("ZEFF_PROFILE_PCE_DISPLAY").as_deref() == Ok("1") {
        emu_backend::profile_pce_projection();
        return Ok(());
    }

    #[cfg(feature = "profile-cores")]
    if let Ok(archive) = std::env::var("ZEFF_PROFILE_PCE_CD_ARCHIVE") {
        let cache_root = std::env::var("ZEFF_PROFILE_PCE_CD_CACHE_ROOT")
            .context("ZEFF_PROFILE_PCE_CD_CACHE_ROOT is required")?;
        emu_backend::profile_pce_cd_cache(Path::new(&archive), Path::new(&cache_root))
            .map_err(anyhow::Error::msg)?;
        return Ok(());
    }

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
            settings.emulation.firmware_search_dirs(),
            &headless_opts,
        );
    }

    let (backend, deferred_initial_rom_load) = match args.rom_path {
        Some(path) if app::is_native_archive_path(Path::new(&path)) => (None, Some(path.into())),
        Some(path) => (Some(create_backend(&path, &settings)?), None),
        None => (None, None),
    };

    app::run(backend, settings, deferred_initial_rom_load)?;

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
            sega8_video_standard: settings.emulation.sega8_video_standard.forced_standard(),
            sega8_console_region: settings.emulation.sega8_console_region.forced_region(),
            pce_console_wiring: settings.emulation.pce_console_wiring.forced_wiring(),
            pce_arcade_card_mode: settings.emulation.pce_arcade_card.core_mode(),
            firmware_search_dirs: settings.emulation.firmware_search_dirs(),
            gb_use_external_boot_rom: matches!(
                settings.emulation.gb_boot_rom_mode,
                crate::settings::GbBootRomMode::External
            ),
            gba_use_external_bios: matches!(
                settings.emulation.gba_bios_mode,
                crate::settings::GbaBiosMode::External
            ),
            sega8_use_external_boot_rom: matches!(
                settings.emulation.sega_boot_rom_mode,
                crate::settings::SegaBootRomMode::External
            ),
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

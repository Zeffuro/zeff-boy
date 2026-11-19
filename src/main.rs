#![cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, unused_imports, unused_variables)
)]

#[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod app;
mod audio;
mod audio_discovery;
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
mod netplay;
mod patching;
#[cfg(not(target_arch = "wasm32"))]
mod pgo_training;
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

    if netplay::adapters::run_if_requested()? {
        return Ok(());
    }

    if netplay::qualification::native::run_if_requested()? {
        return Ok(());
    }

    if app::run_netplay_proof_if_requested()? {
        return Ok(());
    }

    if netplay::proof::run_if_requested()? {
        return Ok(());
    }

    if cli::run_audio_discovery_if_requested()? {
        return Ok(());
    }

    if pgo_training::run_if_requested()? {
        return Ok(());
    }

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
    let detected = app::detect_and_extract_rom_with_zip_witness(path)?;
    let loaded = load_backend_from_rom_source(
        detected.system,
        path,
        &detected.rom_path,
        detected.preloaded_data,
        BackendLoadConfig {
            authenticated_zip_member: detected.authenticated_zip_member,
            gb_hardware_mode_preference: settings.emulation.hardware_mode_preference,
            md_region: settings.emulation.md_region.forced_region(),
            md_timing: settings.emulation.md_timing.core_model(),
            md_controller: settings.emulation.md_controller.core_kind(),
            md_use_tmss: settings.emulation.md_use_tmss,
            md_svp_functional_profile: settings
                .emulation
                .md_svp_functional_profile
                .map(|profile| profile.core_profile()),
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn direct_md_rom_startup_uses_the_saved_controller_type() {
    use crate::settings::MdControllerPreference;
    use zeff_md_core::ControllerPort;

    let directory = crate::test_support::test_directory("md-startup-controller").unwrap();
    let path = directory.path().join("controller.md");
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = b'U';
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    std::fs::write(&path, rom).unwrap();
    let mut settings = Settings::default();
    for controller in [
        MdControllerPreference::ThreeButton,
        MdControllerPreference::SixButton,
    ] {
        settings.emulation.md_controller = controller;
        let EmuBackend::Md(backend) = create_backend(path.to_str().unwrap(), &settings).unwrap()
        else {
            panic!("expected a Mega Drive backend");
        };
        for port in [ControllerPort::One, ControllerPort::Two] {
            assert_eq!(
                backend.core().input().controller_kind(port),
                controller.core_kind()
            );
        }
    }
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

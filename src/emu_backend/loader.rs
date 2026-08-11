use std::path::{Path, PathBuf};

use anyhow::Context;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::{ActiveSystem, EmuBackend};

#[derive(Clone, Copy, Debug)]
pub(crate) struct BackendLoadConfig {
    pub(crate) gb_hardware_mode_preference: HardwareModePreference,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) apply_mods: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) sega8_video_standard: Option<Sega8VideoStandard>,
    pub(crate) sega8_console_region: Option<Sega8Region>,
}

impl Default for BackendLoadConfig {
    fn default() -> Self {
        Self {
            gb_hardware_mode_preference: HardwareModePreference::Auto,
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            sega8_video_standard: None,
            sega8_console_region: None,
        }
    }
}

pub(crate) struct LoadedBackend {
    pub(crate) backend: EmuBackend,
    pub(crate) original_crc32: u32,
}

pub(crate) fn load_backend_from_rom_source(
    system: ActiveSystem,
    source_path: &Path,
    rom_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    let mut rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(source_path)
            .with_context(|| format!("Failed to read {} ROM", system_load_label(system)))?,
    };

    let original_crc32 = if config.apply_mods {
        apply_mods_if_any(system, &mut rom_data)
    } else {
        crc32fast::hash(&rom_data)
    };

    let mut backend = match system {
        ActiveSystem::GameBoy => load_gb_backend(&rom_data, source_path, rom_path, config)?,
        ActiveSystem::Nes => load_nes_backend(&rom_data, source_path, rom_path, config)?,
        ActiveSystem::GameBoyAdvance => load_gba_backend(&rom_data, source_path, rom_path, config)?,
        ActiveSystem::WonderSwan => load_ws_backend(&rom_data, source_path, rom_path, config)?,
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            load_sega8_backend(system, &rom_data, source_path, rom_path, config)?
        }
    };

    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }

    Ok(LoadedBackend {
        backend,
        original_crc32,
    })
}

fn apply_mods_if_any(system: ActiveSystem, rom_data: &mut Vec<u8>) -> u32 {
    let crc = crc32fast::hash(rom_data);
    let dir = crate::mods::mods_dir_for_rom(system, crc);
    let mods = crate::mods::load_mod_config(&dir);
    let enabled = mods.iter().filter(|m| m.enabled).count();
    if enabled > 0 {
        let warnings = crate::mods::apply_enabled_mods(rom_data, &dir, &mods);
        for warning in &warnings {
            log::warn!("Mod warning: {warning}");
        }
        log::info!(
            "Applied {enabled} mod(s) to ROM ({} warnings)",
            warnings.len()
        );
    }
    crc
}

fn load_gb_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let mut emu = zeff_gb_core::emulator::Emulator::from_rom_data(
        rom_data,
        config.gb_hardware_mode_preference,
    )?;
    if let Some(sample_rate) = config.sample_rate {
        emu.set_sample_rate(sample_rate);
    }
    log_sram_result(super::gb::try_load_battery_sram(&mut emu, rom_path));
    Ok(wrap_gb_backend(emu, source_path, rom_path))
}

fn load_nes_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .map(f64::from)
        .unwrap_or(zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE);
    let mut emu = zeff_nes_core::emulator::Emulator::new(rom_data, sample_rate)?;
    log_sram_result(super::nes::try_load_battery_sram(&mut emu, rom_path));
    Ok(wrap_nes_backend(emu, source_path, rom_path))
}

fn load_gba_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_gba_core::emulator::DEFAULT_SAMPLE_RATE);
    let mut emu = zeff_gba_core::emulator::Emulator::new(rom_data, sample_rate)?;
    log_sram_result(super::gba::try_load_battery_sram(&mut emu, rom_path));
    Ok(wrap_gba_backend(emu, source_path, rom_path))
}

fn load_ws_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE);
    let mut emu = zeff_ws_core::emulator::Emulator::new(rom_data, sample_rate)?;
    log_sram_result(super::ws::try_load_battery_sram(&mut emu, rom_path));
    Ok(wrap_ws_backend(emu, source_path, rom_path))
}

fn load_sega8_backend(
    system: ActiveSystem,
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE);
    let hint =
        super::sega8::hint_for_active_system(system).expect("Sega 8-bit systems must have a hint");
    let video_standard = config
        .sega8_video_standard
        .or_else(|| super::sega8::video_standard_from_paths(source_path, rom_path))
        .unwrap_or_default();
    let console_region_fallback = super::sega8::console_region_from_paths(source_path, rom_path);
    let mut emu =
        zeff_sega8_core::emulator::Emulator::new_with_hint_video_standard_region_fallback(
            rom_data,
            sample_rate,
            hint,
            video_standard,
            config.sega8_console_region,
            console_region_fallback,
        )?;
    log_sram_result(super::sega8::try_load_battery_sram(&mut emu, rom_path));
    Ok(wrap_sega8_backend(emu, source_path, rom_path))
}

fn wrap_gb_backend(
    emu: zeff_gb_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
) -> EmuBackend {
    wrap_backend_paths(
        emu,
        source_path,
        rom_path,
        EmuBackend::from_gb,
        EmuBackend::from_gb_with_source,
    )
}

fn wrap_nes_backend(
    emu: zeff_nes_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
) -> EmuBackend {
    wrap_backend_paths(
        emu,
        source_path,
        rom_path,
        EmuBackend::from_nes,
        EmuBackend::from_nes_with_source,
    )
}

fn wrap_gba_backend(
    emu: zeff_gba_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
) -> EmuBackend {
    wrap_backend_paths(
        emu,
        source_path,
        rom_path,
        EmuBackend::from_gba,
        EmuBackend::from_gba_with_source,
    )
}

fn wrap_ws_backend(
    emu: zeff_ws_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
) -> EmuBackend {
    wrap_backend_paths(
        emu,
        source_path,
        rom_path,
        EmuBackend::from_ws,
        EmuBackend::from_ws_with_source,
    )
}

fn wrap_sega8_backend(
    emu: zeff_sega8_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
) -> EmuBackend {
    wrap_backend_paths(
        emu,
        source_path,
        rom_path,
        EmuBackend::from_sega8,
        EmuBackend::from_sega8_with_source,
    )
}

fn wrap_backend_paths<T>(
    emu: T,
    source_path: &Path,
    rom_path: &Path,
    from_rom_path: fn(T, PathBuf) -> EmuBackend,
    from_source_path: fn(T, PathBuf, PathBuf) -> EmuBackend,
) -> EmuBackend {
    if source_path == rom_path {
        from_rom_path(emu, rom_path.to_path_buf())
    } else {
        from_source_path(emu, rom_path.to_path_buf(), source_path.to_path_buf())
    }
}

fn log_sram_result(result: anyhow::Result<Option<String>>) {
    match result {
        Ok(Some(path)) => log::info!("Loaded battery save from {path}"),
        Ok(None) => {}
        Err(err) => log::warn!("Failed to load battery save: {err}"),
    }
}

fn system_load_label(system: ActiveSystem) -> &'static str {
    match system {
        ActiveSystem::GameBoy => "GB",
        ActiveSystem::GameBoyAdvance => "GBA",
        ActiveSystem::Nes => "NES",
        ActiveSystem::WonderSwan => "WonderSwan",
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => "Sega 8-bit",
    }
}

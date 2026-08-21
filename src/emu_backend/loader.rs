use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_sega8_core::emulator::Sega8LoadConfig;
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::{ActiveSystem, EmuBackend};

#[derive(Clone, Debug)]
pub(crate) struct BackendLoadConfig {
    pub(crate) gb_hardware_mode_preference: HardwareModePreference,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) apply_mods: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) sega8_video_standard: Option<Sega8VideoStandard>,
    pub(crate) sega8_console_region: Option<Sega8Region>,
    pub(crate) firmware_search_dirs: Vec<PathBuf>,
    pub(crate) firmware_inventory: Option<Arc<zeff_firmware::FirmwareInventory>>,
    pub(crate) gb_use_external_boot_rom: bool,
    pub(crate) gba_use_external_bios: bool,
    pub(crate) sega8_use_external_boot_rom: bool,
    #[cfg(test)]
    pub(crate) fds_bios_override: Option<&'static [u8]>,
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
            firmware_search_dirs: Vec::new(),
            firmware_inventory: None,
            gb_use_external_boot_rom: false,
            gba_use_external_bios: false,
            sega8_use_external_boot_rom: false,
            #[cfg(test)]
            fds_bios_override: None,
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

    let default_firmware_manifests =
        super::firmware::default_firmware_manifests_for_active_system(system);

    let mut backend = match system {
        ActiveSystem::GameBoy => load_gb_backend(&rom_data, source_path, rom_path, &config)?,
        ActiveSystem::Nes => load_nes_backend(&rom_data, source_path, rom_path, &config)?,
        ActiveSystem::GameBoyAdvance => {
            load_gba_backend(&rom_data, source_path, rom_path, &config)?
        }
        ActiveSystem::WonderSwan => load_ws_backend(&rom_data, source_path, rom_path, &config)?,
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            load_sega8_backend(system, &rom_data, source_path, rom_path, &config)?
        }
    };

    if !default_firmware_manifests.is_empty()
        && !(system == ActiveSystem::GameBoy && config.gb_use_external_boot_rom)
        && !(system == ActiveSystem::GameBoyAdvance && config.gba_use_external_bios)
        && !(matches!(system, ActiveSystem::MasterSystem | ActiveSystem::GameGear)
            && config.sega8_use_external_boot_rom)
    {
        backend.set_firmware_manifests(default_firmware_manifests);
    }

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
    config: &BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let external_boot_rom = if config.gb_use_external_boot_rom {
        let header = zeff_gb_core::hardware::rom_header::RomHeader::from_rom(rom_data)?;
        let mode = config.gb_hardware_mode_preference.resolve(
            header.is_cgb_compatible,
            header.is_sgb_supported,
            header.old_licensee_code,
        );
        let firmware_id = if matches!(
            mode,
            zeff_gb_core::hardware::types::hardware_mode::HardwareMode::CGBNormal
                | zeff_gb_core::hardware::types::hardware_mode::HardwareMode::CGBDouble
        ) {
            "nintendo.gb.boot.cgb"
        } else {
            "nintendo.gb.boot.dmg"
        };
        Some(super::firmware::resolve_gb_boot_rom_with_manifest(
            firmware_id,
            config.firmware_inventory.as_deref(),
            &config.firmware_search_dirs,
            Some(rom_path),
        )?)
    } else {
        None
    };
    let mut emu = match &external_boot_rom {
        Some(boot_rom) => zeff_gb_core::emulator::Emulator::from_rom_data_with_boot_rom(
            rom_data,
            config.gb_hardware_mode_preference,
            &boot_rom.bytes,
        )?,
        None => zeff_gb_core::emulator::Emulator::from_rom_data(
            rom_data,
            config.gb_hardware_mode_preference,
        )?,
    };
    if let Some(sample_rate) = config.sample_rate {
        emu.set_sample_rate(sample_rate);
    }
    log_sram_result(super::gb::try_load_battery_sram(&mut emu, rom_path));
    let mut backend = wrap_gb_backend(emu, source_path, rom_path);
    if let Some(boot_rom) = external_boot_rom {
        let mut manifests =
            super::firmware::default_firmware_manifests_for_active_system(ActiveSystem::GameBoy);
        let firmware_id = match &boot_rom.manifest {
            zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. } => {
                firmware_id.as_str()
            }
            _ => unreachable!("resolved GB boot ROM must be external"),
        };
        if let Some(manifest) = manifests.iter_mut().find(|manifest| {
            matches!(manifest, zeff_emu_common::replay::ReplayFirmwareManifest::Skipped { firmware_id: id, .. } if id == firmware_id)
        }) {
            *manifest = boot_rom.manifest;
        }
        backend.set_firmware_manifests(manifests);
    }
    Ok(backend)
}

fn load_nes_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    if is_fds_path(rom_path) {
        let sample_rate = config
            .sample_rate
            .map(f64::from)
            .unwrap_or(zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE);
        let bios = resolve_fds_bios(config, rom_path)?;
        let mut emu =
            zeff_nes_core::emulator::Emulator::new_fds(rom_data, bios.bytes, sample_rate)?;
        log_sram_result(super::nes::try_load_battery_sram(&mut emu, rom_path));
        let mut backend = wrap_nes_backend(emu, source_path, rom_path);
        backend.set_firmware_manifests(vec![bios.manifest]);
        return Ok(backend);
    }

    let sample_rate = config
        .sample_rate
        .map(f64::from)
        .unwrap_or(zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE);
    let mut emu = zeff_nes_core::emulator::Emulator::new(rom_data, sample_rate)?;
    log_sram_result(super::nes::try_load_battery_sram(&mut emu, rom_path));
    Ok(wrap_nes_backend(emu, source_path, rom_path))
}

fn resolve_fds_bios(
    _config: &BackendLoadConfig,
    rom_path: &Path,
) -> anyhow::Result<super::firmware::ResolvedFirmwareBytes> {
    #[cfg(test)]
    if let Some(bytes) = _config.fds_bios_override {
        return Ok(super::firmware::ResolvedFirmwareBytes {
            bytes: bytes.to_vec(),
            manifest: zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: "nintendo.fds.bios".to_owned(),
                variant: Some("test-override".to_owned()),
                sha256: zeff_firmware::sha256_bytes(bytes),
            },
        });
    }

    super::firmware::resolve_fds_bios_with_manifest(
        _config.firmware_inventory.as_deref(),
        &_config.firmware_search_dirs,
        Some(rom_path),
    )
}

fn is_fds_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fds"))
}

fn load_gba_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_gba_core::emulator::DEFAULT_SAMPLE_RATE);
    let external_bios = if config.gba_use_external_bios {
        Some(super::firmware::resolve_gba_bios_with_manifest(
            config.firmware_inventory.as_deref(),
            &config.firmware_search_dirs,
            Some(rom_path),
        )?)
    } else {
        None
    };
    let mut emu = match &external_bios {
        Some(bios) => {
            zeff_gba_core::emulator::Emulator::new_with_bios(rom_data, &bios.bytes, sample_rate)?
        }
        None => zeff_gba_core::emulator::Emulator::new(rom_data, sample_rate)?,
    };
    log_sram_result(super::gba::try_load_battery_sram(&mut emu, rom_path));
    if emu.has_rtc() {
        emu.set_rtc_date_time(crate::platform::local_gba_rtc_date_time());
    }
    let mut backend = wrap_gba_backend(emu, source_path, rom_path);
    if let Some(bios) = external_bios {
        backend.set_firmware_manifests(vec![bios.manifest]);
    }
    Ok(backend)
}

fn load_ws_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
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
    config: &BackendLoadConfig,
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
    let mapper_kind = super::sega8::mapper_kind_from_paths(source_path, rom_path);
    let load_config = Sega8LoadConfig::new(sample_rate)
        .with_system_hint(hint)
        .with_mapper_kind(mapper_kind)
        .with_video_standard(video_standard)
        .with_console_region(config.sega8_console_region)
        .with_console_region_fallback(console_region_fallback);
    let external_boot_rom = if config.sega8_use_external_boot_rom
        && matches!(system, ActiveSystem::MasterSystem | ActiveSystem::GameGear)
    {
        Some(super::firmware::resolve_sega8_boot_rom_with_manifest(
            system,
            config.sega8_console_region.or(console_region_fallback),
            config.firmware_inventory.as_deref(),
            &config.firmware_search_dirs,
            Some(rom_path),
        )?)
    } else {
        None
    };
    let mut emu = match &external_boot_rom {
        Some(boot_rom) => zeff_sega8_core::emulator::Emulator::new_with_config_and_boot_rom(
            rom_data,
            load_config,
            &boot_rom.bytes,
        )?,
        None => zeff_sega8_core::emulator::Emulator::new_with_config(rom_data, load_config)?,
    };
    log_sram_result(super::sega8::try_load_battery_sram(&mut emu, rom_path));
    let mut backend = wrap_sega8_backend(emu, source_path, rom_path);
    if let Some(boot_rom) = external_boot_rom {
        backend.set_firmware_manifests(vec![boot_rom.manifest]);
    }
    Ok(backend)
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

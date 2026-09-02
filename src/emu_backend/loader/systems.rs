use std::path::{Path, PathBuf};

use zeff_pce_core::hardware::PceArcadeCardMode;
use zeff_sega8_core::emulator::Sega8LoadConfig;

use super::{BackendLoadConfig, EmuBackend};
use crate::emu_backend::ActiveSystem;
use crate::emu_core_trait::EmulatorCore;

pub(super) struct ModLoadOutcome {
    pub(super) original_crc32: u32,
    pub(super) any_enabled: bool,
    pub(super) any_applied: bool,
}

pub(super) fn apply_mods_if_any(system: ActiveSystem, rom_data: &mut Vec<u8>) -> ModLoadOutcome {
    let crc = crc32fast::hash(rom_data);
    let dir = crate::mods::mods_dir_for_rom(system, crc);
    let mods = crate::mods::load_mod_config(&dir);
    let enabled = mods.iter().filter(|m| m.enabled).count();
    if enabled > 0 {
        let warnings = crate::mods::apply_enabled_mods(rom_data, &dir, &mods);
        let any_applied = warnings.len() < enabled;
        for warning in &warnings {
            log::warn!("Mod warning: {warning}");
        }
        log::info!(
            "Applied {enabled} mod(s) to ROM ({} warnings)",
            warnings.len()
        );
        return ModLoadOutcome {
            original_crc32: crc,
            any_enabled: true,
            any_applied,
        };
    }
    ModLoadOutcome {
        original_crc32: crc,
        any_enabled: false,
        any_applied: false,
    }
}

pub(super) fn load_gb_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    provenance: super::super::gb::GbTasLoadProvenanceSeed,
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
        Some(super::super::firmware::resolve_gb_boot_rom_with_manifest(
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
    let persistent_load = if config.gb_load_battery_sram {
        let result = super::super::gb::try_load_battery_sram_at_time(
            &mut emu,
            rom_path,
            config.gb_rtc_time_override,
        );
        let outcome = super::super::gb::persistent_load_outcome(&result);
        log_sram_result(result);
        outcome
    } else {
        super::super::gb::GbPersistentLoadOutcome::Absent
    };
    let provenance = provenance.finish(
        persistent_load,
        emu.hardware_mode(),
        emu.sample_rate(),
        emu.has_boot_rom(),
    );
    let mut backend = wrap_gb_backend_with_provenance(emu, source_path, rom_path, provenance);
    if let Some(boot_rom) = external_boot_rom {
        let mut manifests = super::super::firmware::default_firmware_manifests_for_active_system(
            ActiveSystem::GameBoy,
        );
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

pub(super) fn load_nes_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    provenance: super::super::nes::NesTasLoadProvenanceSeed,
) -> anyhow::Result<EmuBackend> {
    if is_fds_path(rom_path) {
        let sample_rate = config
            .sample_rate
            .map(f64::from)
            .unwrap_or(zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE);
        let bios = resolve_fds_bios(config, rom_path)?;
        let mut emu =
            zeff_nes_core::emulator::Emulator::new_fds(rom_data, bios.bytes, sample_rate)?;
        let persistent_load = load_nes_persistent_data(&mut emu, rom_path, config);
        let battery_backed = emu.save_ram_kind().is_battery_backed();
        let mut backend = wrap_nes_backend(
            emu,
            source_path,
            rom_path,
            provenance.finish(persistent_load, battery_backed),
        );
        backend.set_firmware_manifests(vec![bios.manifest]);
        return Ok(backend);
    }

    let sample_rate = config
        .sample_rate
        .map(f64::from)
        .unwrap_or(zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE);
    let mut emu = zeff_nes_core::emulator::Emulator::new(rom_data, sample_rate)?;
    let persistent_load = load_nes_persistent_data(&mut emu, rom_path, config);
    let battery_backed = emu.save_ram_kind().is_battery_backed();
    Ok(wrap_nes_backend(
        emu,
        source_path,
        rom_path,
        provenance.finish(persistent_load, battery_backed),
    ))
}

fn load_nes_persistent_data(
    emu: &mut zeff_nes_core::emulator::Emulator,
    rom_path: &Path,
    config: &BackendLoadConfig,
) -> super::super::nes::NesPersistentLoadOutcome {
    if !config.nes_load_battery_sram {
        return super::super::nes::NesPersistentLoadOutcome::Absent;
    }
    let result = super::super::nes::try_load_battery_sram(emu, rom_path);
    let outcome = super::super::nes::persistent_load_outcome(&result);
    log_sram_result(result);
    outcome
}

pub(super) fn load_coleco_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    provenance: super::super::coleco::ColecoTasLoadProvenanceSeed,
) -> anyhow::Result<EmuBackend> {
    let bios = resolve_coleco_bios(config, rom_path)?;
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_coleco_core::constants::DEFAULT_SAMPLE_RATE);
    let emu = zeff_coleco_core::Emulator::new(rom_data, &bios.bytes, sample_rate)?;
    let rom_hash = super::super::ColecoBackend::rom_hash_for_bytes(rom_data);
    let mut backend = wrap_coleco_backend(emu, source_path, rom_path, rom_hash)
        .with_coleco_tas_load_provenance(provenance);
    backend.set_firmware_manifests(vec![bios.manifest]);
    Ok(backend)
}

fn resolve_coleco_bios(
    config: &BackendLoadConfig,
    rom_path: &Path,
) -> anyhow::Result<super::super::firmware::ResolvedFirmwareBytes> {
    #[cfg(test)]
    if let Some(bytes) = config.coleco_bios_override {
        return Ok(super::super::firmware::ResolvedFirmwareBytes {
            bytes: bytes.to_vec(),
            sha256: zeff_firmware::sha256_bytes(bytes),
            manifest: zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: "coleco.vision.bios".to_owned(),
                variant: Some("test-override".to_owned()),
                sha256: zeff_firmware::sha256_bytes(bytes),
            },
        });
    }

    super::super::firmware::resolve_coleco_bios_with_manifest(
        config.firmware_inventory.as_deref(),
        &config.firmware_search_dirs,
        Some(rom_path),
    )
}

pub(super) fn load_pce_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    provenance: super::super::pce::PceTasLoadProvenanceSeed,
) -> anyhow::Result<EmuBackend> {
    anyhow::ensure!(
        config.pce_arcade_card_mode != PceArcadeCardMode::Enabled,
        "Arcade Card requires CD media and a System Card v3 environment"
    );
    if rom_data.is_empty() {
        anyhow::bail!("PC Engine HuCard ROM is empty");
    }
    let mut backend = if source_path == rom_path {
        super::super::PceBackend::new_with_overrides(
            rom_data.to_vec(),
            rom_path.to_path_buf(),
            config.pce_console_wiring,
            config.pce_hucard_board,
            config.pce_cartridge_hardware,
        )?
    } else {
        super::super::PceBackend::with_source_path_and_overrides(
            rom_data.to_vec(),
            rom_path.to_path_buf(),
            source_path.to_path_buf(),
            config.pce_console_wiring,
            config.pce_hucard_board,
            config.pce_cartridge_hardware,
        )?
    };
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    backend.set_pce_mouse_state(config.pce_controller_mode, 0, 0, 0);
    backend.set_pce_memory_base_mode(config.pce_memory_base_mode);
    let persistent_load = if config.pce_load_battery_bram {
        let result = backend.try_load_memory_base128();
        let outcome = super::super::pce::pce_persistent_load_outcome(&result);
        log_sram_result(result);
        outcome
    } else {
        super::super::pce::PceTasPersistentLoadOutcome::Skipped
    };
    let provenance = provenance.finish(&backend, persistent_load);
    backend = backend.with_tas_load_provenance(provenance);
    Ok(EmuBackend::from_pce(backend))
}

fn resolve_fds_bios(
    config: &BackendLoadConfig,
    rom_path: &Path,
) -> anyhow::Result<super::super::firmware::ResolvedFirmwareBytes> {
    #[cfg(test)]
    if let Some(bytes) = config.fds_bios_override {
        return Ok(super::super::firmware::ResolvedFirmwareBytes {
            bytes: bytes.to_vec(),
            sha256: zeff_firmware::sha256_bytes(bytes),
            manifest: zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: "nintendo.fds.bios".to_owned(),
                variant: Some("test-override".to_owned()),
                sha256: zeff_firmware::sha256_bytes(bytes),
            },
        });
    }

    super::super::firmware::resolve_fds_bios_with_manifest(
        config.firmware_inventory.as_deref(),
        &config.firmware_search_dirs,
        Some(rom_path),
    )
}

fn is_fds_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fds"))
}

pub(super) fn load_gba_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    tas_provenance: super::super::gba::GbaTasLoadProvenanceSeed,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_gba_core::emulator::DEFAULT_SAMPLE_RATE);
    let external_bios = if config.gba_use_external_bios {
        Some(super::super::firmware::resolve_gba_bios_with_manifest(
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
    let persistent_load = if config.gba_load_battery_sram {
        let result = super::super::gba::try_load_battery_sram(&mut emu, rom_path);
        let outcome = super::super::gba::persistent_load_outcome(&result);
        log_sram_result(result);
        outcome
    } else {
        super::super::gba::GbaTasPersistentLoadOutcome::Skipped
    };
    let rtc_seeded_from_host = config.gba_seed_rtc_from_host && emu.has_rtc();
    if rtc_seeded_from_host {
        emu.set_rtc_date_time(crate::platform::local_gba_rtc_date_time());
    }
    let provenance = tas_provenance.finish(
        persistent_load,
        emu.apu_debug_snapshot().sample_rate,
        rtc_seeded_from_host,
    );
    let mut backend = EmuBackend::from_gba_with_tas_load_provenance(
        emu,
        rom_path.to_path_buf(),
        source_path.to_path_buf(),
        provenance,
    );
    if let Some(bios) = external_bios {
        backend.set_firmware_manifests(vec![bios.manifest]);
    }
    Ok(backend)
}

pub(super) fn load_ws_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    tas_provenance: super::super::ws::WsTasLoadProvenanceSeed,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE);
    let mut emu = zeff_ws_core::emulator::Emulator::new(rom_data, sample_rate)?;
    let persistent_load = if config.ws_load_battery_sram {
        let result = super::super::ws::try_load_battery_sram(&mut emu, rom_path);
        let outcome = super::super::ws::persistent_load_outcome(&result);
        log_sram_result(result);
        outcome
    } else if emu.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None {
        super::super::ws::WsTasPersistentLoadOutcome::Absent
    } else {
        super::super::ws::WsTasPersistentLoadOutcome::Skipped
    };
    let provenance = tas_provenance.finish(persistent_load);
    let backend = wrap_ws_backend(emu, source_path, rom_path);
    Ok(match backend {
        EmuBackend::Ws(ws) => EmuBackend::Ws(Box::new((*ws).with_tas_load_provenance(provenance))),
        _ => unreachable!("WonderSwan wrapper returned another backend"),
    })
}

pub(super) fn load_sega8_backend(
    system: ActiveSystem,
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
    tas_provenance: Option<super::super::sega8::Sega8TasLoadProvenanceSeed>,
) -> anyhow::Result<EmuBackend> {
    let sample_rate = config
        .sample_rate
        .unwrap_or(zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE);
    let hint = super::super::sega8::hint_for_active_system(system)
        .expect("Sega 8-bit systems must have a hint");
    let video_standard = config
        .sega8_video_standard
        .or_else(|| super::super::sega8::video_standard_from_paths(source_path, rom_path))
        .unwrap_or_default();
    let console_region_fallback =
        super::super::sega8::console_region_from_paths(source_path, rom_path);
    let mapper_kind = super::super::sega8::mapper_kind_from_paths(source_path, rom_path);
    let mut load_config = Sega8LoadConfig::new(sample_rate)
        .with_system_hint(hint)
        .with_mapper_kind(mapper_kind)
        .with_video_standard(video_standard)
        .with_console_region(config.sega8_console_region)
        .with_console_region_fallback(console_region_fallback);
    if let Some(identity) = config.game_gear_standard_mapper_ram_identity {
        load_config = load_config.with_game_gear_standard_mapper_ram_identity(identity);
    }
    let external_boot_rom = if config.sega8_use_external_boot_rom
        && matches!(system, ActiveSystem::MasterSystem | ActiveSystem::GameGear)
    {
        Some(
            super::super::firmware::resolve_sega8_boot_rom_with_manifest(
                system,
                config.sega8_console_region.or(console_region_fallback),
                config.firmware_inventory.as_deref(),
                &config.firmware_search_dirs,
                Some(rom_path),
            )?,
        )
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
    let persistent_load = if config.sega8_load_battery_sram {
        super::super::sega8::try_load_battery_sram(&mut emu, rom_path)
    } else {
        Ok(None)
    };
    let game_gear_persistent_load = if config.sega8_load_battery_sram
        || emu.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None
    {
        super::super::sega8::game_gear_persistent_load_outcome(&persistent_load)
    } else {
        super::super::sega8::GameGearTasPersistentLoadOutcome::Skipped
    };
    let sg1000_persistent_load =
        super::super::sega8::sg1000_persistent_load_outcome(&persistent_load);
    log_sram_result(persistent_load);
    let mut backend = wrap_sega8_backend(emu, source_path, rom_path);
    if let Some(provenance) = tas_provenance {
        backend = match provenance {
            super::super::sega8::Sega8TasLoadProvenanceSeed::MasterSystem(provenance) => {
                backend.with_sms_tas_load_provenance(provenance.finish())
            }
            super::super::sega8::Sega8TasLoadProvenanceSeed::GameGear(provenance) => backend
                .with_game_gear_tas_load_provenance(provenance.finish(game_gear_persistent_load)),
            super::super::sega8::Sega8TasLoadProvenanceSeed::Sg1000(provenance) => {
                backend.with_sg1000_tas_load_provenance(provenance.finish(sg1000_persistent_load))
            }
        };
    }
    if let Some(boot_rom) = external_boot_rom {
        backend.set_firmware_manifests(vec![boot_rom.manifest]);
    }
    Ok(backend)
}

fn wrap_gb_backend_with_provenance(
    emu: zeff_gb_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
    provenance: super::super::gb::GbTasLoadProvenance,
) -> EmuBackend {
    EmuBackend::Gb(Box::new(super::super::GbBackend::with_load_provenance(
        emu,
        rom_path.to_path_buf(),
        source_path.to_path_buf(),
        provenance,
    )))
}

fn wrap_nes_backend(
    emu: zeff_nes_core::emulator::Emulator,
    source_path: &Path,
    rom_path: &Path,
    provenance: super::super::nes::NesTasLoadProvenance,
) -> EmuBackend {
    EmuBackend::Nes(Box::new(super::super::NesBackend::with_load_provenance(
        emu,
        rom_path.to_path_buf(),
        source_path.to_path_buf(),
        provenance,
    )))
}

fn wrap_coleco_backend(
    emu: zeff_coleco_core::Emulator,
    source_path: &Path,
    rom_path: &Path,
    rom_hash: [u8; 32],
) -> EmuBackend {
    if source_path == rom_path {
        EmuBackend::from_coleco(emu, rom_path.to_path_buf(), rom_hash)
    } else {
        EmuBackend::from_coleco_with_source(
            emu,
            rom_path.to_path_buf(),
            source_path.to_path_buf(),
            rom_hash,
        )
    }
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

pub(super) fn log_sram_result(result: anyhow::Result<Option<String>>) {
    match result {
        Ok(Some(path)) => log::info!("Loaded battery save from {path}"),
        Ok(None) => {}
        Err(err) => log::warn!("Failed to load battery save: {err}"),
    }
}

pub(super) fn system_load_label(system: ActiveSystem) -> &'static str {
    match system {
        ActiveSystem::GameBoy => "GB",
        ActiveSystem::GameBoyAdvance => "GBA",
        ActiveSystem::Nes => "NES",
        ActiveSystem::Coleco => "ColecoVision",
        ActiveSystem::Pce => "PC Engine",
        ActiveSystem::WonderSwan => "WonderSwan",
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => "Sega 8-bit",
    }
}

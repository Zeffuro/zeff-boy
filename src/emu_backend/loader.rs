use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_pce_core::hardware::{PceArcadeCardMode, PceConsoleWiring, PceHuCardBoard};
use zeff_sega8_core::emulator::Sega8LoadConfig;
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::{ActiveSystem, EmuBackend};
use crate::emu_core_trait::EmulatorCore;

#[derive(Clone, Debug)]
pub(crate) struct BackendLoadConfig {
    pub(crate) gb_hardware_mode_preference: HardwareModePreference,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) apply_mods: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) sega8_video_standard: Option<Sega8VideoStandard>,
    pub(crate) sega8_console_region: Option<Sega8Region>,
    pub(crate) pce_console_wiring: Option<PceConsoleWiring>,
    pub(crate) pce_hucard_board: Option<PceHuCardBoard>,
    pub(crate) pce_arcade_card_mode: PceArcadeCardMode,
    pub(crate) pce_cd_archive_memory_limit_mib: usize,
    pub(crate) pce_load_battery_bram: bool,
    pub(crate) firmware_search_dirs: Vec<PathBuf>,
    pub(crate) firmware_inventory: Option<Arc<zeff_firmware::FirmwareInventory>>,
    pub(crate) gb_use_external_boot_rom: bool,
    pub(crate) gba_use_external_bios: bool,
    pub(crate) sega8_use_external_boot_rom: bool,
    #[cfg(test)]
    pub(crate) fds_bios_override: Option<&'static [u8]>,
    #[cfg(test)]
    pub(crate) pce_cd_system_card_override: Option<&'static [u8]>,
    #[cfg(test)]
    pub(crate) pce_cd_system_card_sha256_override: Option<[u8; 32]>,
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
            pce_console_wiring: None,
            pce_hucard_board: None,
            pce_arcade_card_mode: PceArcadeCardMode::Automatic,
            pce_cd_archive_memory_limit_mib: 128,
            pce_load_battery_bram: true,
            firmware_search_dirs: Vec::new(),
            firmware_inventory: None,
            gb_use_external_boot_rom: false,
            gba_use_external_bios: false,
            sega8_use_external_boot_rom: false,
            #[cfg(test)]
            fds_bios_override: None,
            #[cfg(test)]
            pce_cd_system_card_override: None,
            #[cfg(test)]
            pce_cd_system_card_sha256_override: None,
        }
    }
}

pub(crate) struct LoadedBackend {
    pub(crate) backend: EmuBackend,
    pub(crate) original_crc32: u32,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum PreparedSevenZipBackend {
    Ready {
        rom_path: PathBuf,
        system: ActiveSystem,
        loaded: LoadedBackend,
    },
    Selection(Vec<crate::rom_archive::ArchiveRomEntry>),
}

pub(crate) fn load_backend_from_rom_source(
    system: ActiveSystem,
    source_path: &Path,
    rom_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    if system == ActiveSystem::Pce && is_pce_cd_cue_path(rom_path) {
        return load_pce_cd_backend(source_path, rom_path, preloaded_data, &config);
    }
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
        ActiveSystem::Pce => load_pce_backend(&rom_data, source_path, rom_path, &config)?,
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

fn is_pce_cd_cue_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_pce_cd_backend(
    source_path: &Path,
    cue_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: &BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    if preloaded_data.is_some() {
        return Err(super::pce_cd::PceCdLoadError::PackagedCdSetUnsupported.into());
    }
    let loaded_disc = if source_path == cue_path {
        super::pce_cd::load_direct_cue_with_mods(cue_path, config.apply_mods)?
    } else if path_extension_is(source_path, "7z") {
        let cancel = AtomicBool::new(false);
        let progress = super::pce_cd_archive::PceCdPackageProgress::default();
        let (actual, loaded) = super::pce_cd_archive::load_7z_cue_with_control_and_mods(
            source_path,
            &cancel,
            &progress,
            config.pce_cd_archive_memory_limit_mib,
            config.apply_mods,
        )?;
        if actual != cue_path {
            return Err(super::pce_cd::PceCdLoadError::ArchiveChanged.into());
        }
        loaded
    } else {
        return Err(super::pce_cd::PceCdLoadError::PackagedCdSetUnsupported.into());
    };
    let console_wiring = pce_cd_console_wiring(config, loaded_disc.content_sha256);
    let system_card = resolve_pce_cd_system_card(
        config,
        source_path,
        console_wiring,
        loaded_disc.disc.content_hash() == super::pce_cd::ADPCM_FIXTURE_DISC_SHA256,
    )?;
    let system_card_profile = pce_system_card_profile(&system_card, console_wiring)?;
    let system_card_board = pce_system_card_board(system_card_profile);
    let mut backend = super::PceBackend::new_cdrom2(
        system_card.bytes,
        loaded_disc.disc,
        super::pce::PceCdBackendConfig {
            system_card_board,
            cue_path: cue_path.to_path_buf(),
            source_path: source_path.to_path_buf(),
            content_hash: loaded_disc.content_sha256,
            content_crc32: loaded_disc.content_crc32,
            source_disc_hash: loaded_disc.source_disc_sha256,
            console_wiring,
            arcade_card_mode: config.pce_arcade_card_mode,
        },
    )?;
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    if config.pce_load_battery_bram {
        load_pce_cd_bram(&mut backend);
        log_sram_result(backend.try_load_memory_base128());
    }
    backend.set_firmware_manifests(vec![system_card.manifest]);
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    Ok(LoadedBackend {
        backend: EmuBackend::from_pce(backend),
        original_crc32: loaded_disc.content_crc32,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn prepare_pce_cd_7z_backend(
    source_path: &Path,
    expected_cue_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &AtomicBool,
    progress: &super::pce_cd_archive::PceCdPackageProgress,
) -> anyhow::Result<(PathBuf, LoadedBackend)> {
    check_package_cancel(cancel)?;
    let (cue_path, loaded_disc) = super::pce_cd_archive::load_7z_cue_with_control_and_mods(
        source_path,
        cancel,
        progress,
        config.pce_cd_archive_memory_limit_mib,
        config.apply_mods,
    )?;
    if expected_cue_path.is_some_and(|expected| expected != cue_path) {
        return Err(super::pce_cd::PceCdLoadError::ArchiveChanged.into());
    }
    check_package_cancel(cancel)?;
    progress.set_phase(super::pce_cd_archive::PceCdPackageLoadPhase::Firmware);
    let console_wiring = pce_cd_console_wiring(config, loaded_disc.content_sha256);
    let system_card = resolve_pce_cd_system_card(
        config,
        source_path,
        console_wiring,
        loaded_disc.disc.content_hash() == super::pce_cd::ADPCM_FIXTURE_DISC_SHA256,
    )?;
    let system_card_profile = pce_system_card_profile(&system_card, console_wiring)?;
    let system_card_board = pce_system_card_board(system_card_profile);
    check_package_cancel(cancel)?;
    progress.set_phase(super::pce_cd_archive::PceCdPackageLoadPhase::Building);
    let mut backend = super::PceBackend::new_cdrom2(
        system_card.bytes,
        loaded_disc.disc,
        super::pce::PceCdBackendConfig {
            system_card_board,
            cue_path: cue_path.clone(),
            source_path: source_path.to_path_buf(),
            content_hash: loaded_disc.content_sha256,
            content_crc32: loaded_disc.content_crc32,
            source_disc_hash: loaded_disc.source_disc_sha256,
            console_wiring,
            arcade_card_mode: config.pce_arcade_card_mode,
        },
    )?;
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    if config.pce_load_battery_bram {
        load_pce_cd_bram(&mut backend);
        log_sram_result(backend.try_load_memory_base128());
    }
    backend.set_firmware_manifests(vec![system_card.manifest]);
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    check_package_cancel(cancel)?;
    progress.set_phase(super::pce_cd_archive::PceCdPackageLoadPhase::Complete);
    Ok((
        cue_path,
        LoadedBackend {
            backend: EmuBackend::from_pce(backend),
            original_crc32: loaded_disc.content_crc32,
        },
    ))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn prepare_seven_zip_backend(
    source_path: &Path,
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &AtomicBool,
    progress: &super::pce_cd_archive::PceCdPackageProgress,
) -> anyhow::Result<PreparedSevenZipBackend> {
    check_package_cancel(cancel)?;
    match super::pce_cd_archive::inspect_7z_contents(
        source_path,
        config.pce_cd_archive_memory_limit_mib,
    )? {
        super::pce_cd_archive::SevenZipContents::Cd { cue_path } => {
            if selected_entry_index.is_some()
                || expected_rom_path.is_some_and(|expected| expected != cue_path)
            {
                return Err(super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let (actual, loaded) =
                prepare_pce_cd_7z_backend(source_path, Some(&cue_path), config, cancel, progress)?;
            Ok(PreparedSevenZipBackend::Ready {
                rom_path: actual,
                system: ActiveSystem::Pce,
                loaded,
            })
        }
        super::pce_cd_archive::SevenZipContents::Roms(entries) => {
            let selected = if let Some(index) = selected_entry_index {
                entries.iter().find(|entry| entry.index == index)
            } else if let Some(expected) = expected_rom_path {
                entries.iter().find(|entry| {
                    entry
                        .name
                        .split('/')
                        .fold(source_path.to_path_buf(), |path, part| path.join(part))
                        == expected
                })
            } else if entries.len() == 1 {
                entries.first()
            } else {
                return Ok(PreparedSevenZipBackend::Selection(entries));
            }
            .ok_or(super::pce_cd::PceCdLoadError::ArchiveChanged)?;
            let (rom_path, bytes, system) = super::pce_cd_archive::load_7z_rom_entry_with_control(
                source_path,
                selected.index,
                cancel,
                progress,
                config.pce_cd_archive_memory_limit_mib,
            )?;
            check_package_cancel(cancel)?;
            progress.set_phase(super::pce_cd_archive::PceCdPackageLoadPhase::Building);
            let loaded = load_backend_from_rom_source(
                system,
                source_path,
                &rom_path,
                Some(bytes),
                config.clone(),
            )?;
            check_package_cancel(cancel)?;
            progress.set_phase(super::pce_cd_archive::PceCdPackageLoadPhase::Complete);
            Ok(PreparedSevenZipBackend::Ready {
                rom_path,
                system,
                loaded,
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn check_package_cancel(cancel: &AtomicBool) -> Result<(), super::pce_cd::PceCdLoadError> {
    if cancel.load(Ordering::Acquire) {
        Err(super::pce_cd::PceCdLoadError::ArchiveCancelled)
    } else {
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_pce_cd_bram(backend: &mut super::PceBackend) {
    let rom_path = backend.rom_path().to_path_buf();
    log_sram_result(crate::save_paths::try_load_battery_sram(
        &rom_path,
        "PC Engine CD",
        true,
        |bytes| backend.load_cd_bram(bytes),
    ));
}

#[cfg(not(target_arch = "wasm32"))]
fn pce_system_card_profile(
    firmware: &super::firmware::ResolvedFirmwareBytes,
    console_wiring: PceConsoleWiring,
) -> Result<zeff_firmware::PceSystemCardFirmware, super::pce_cd::PceCdLoadError> {
    let profile = zeff_firmware::classify_pce_system_card_sha256(firmware.sha256)
        .ok_or(super::pce_cd::PceCdLoadError::UnrecognizedSystemCardFirmware(firmware.sha256))?;
    let expected = match console_wiring {
        PceConsoleWiring::PcEngine => zeff_firmware::PceSystemCardRegion::Japan,
        PceConsoleWiring::TurboGrafx16 => zeff_firmware::PceSystemCardRegion::Usa,
    };
    if profile.region() != expected {
        return Err(super::pce_cd::PceCdLoadError::SystemCardRegionMismatch {
            expected,
            actual: profile.region(),
        });
    }
    Ok(profile)
}

fn pce_system_card_board(profile: zeff_firmware::PceSystemCardFirmware) -> PceHuCardBoard {
    match profile.board() {
        zeff_firmware::PceSystemCardBoard::OriginalCdRom2 => PceHuCardBoard::SystemCardV1V2,
        zeff_firmware::PceSystemCardBoard::SuperCdRom2 => PceHuCardBoard::SystemCardV3,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pce_cd_console_wiring(config: &BackendLoadConfig, content_sha256: [u8; 32]) -> PceConsoleWiring {
    const KNOWN_TURBOGRAFX_CD_SHA256: [[u8; 32]; 2] = [
        [
            0x08, 0x02, 0xF4, 0xFA, 0x00, 0x2E, 0x0E, 0x53, 0x2A, 0x30, 0xA7, 0x8B, 0xE5, 0x38,
            0x81, 0x40, 0x7B, 0xE9, 0x40, 0x33, 0x47, 0x2D, 0x95, 0x25, 0xC0, 0x0F, 0xBC, 0x5D,
            0xDE, 0xA8, 0xA1, 0x7F,
        ],
        [
            0x6B, 0xD7, 0x28, 0xCD, 0xF8, 0x7C, 0x6C, 0xEE, 0x9E, 0x96, 0xA0, 0x3D, 0xCE, 0x25,
            0x9C, 0x4C, 0xF3, 0x4F, 0xF3, 0x87, 0xCE, 0xE9, 0xB7, 0x97, 0xA7, 0x35, 0xCF, 0x80,
            0x8E, 0xE7, 0xD2, 0x94,
        ],
    ];
    config.pce_console_wiring.unwrap_or_else(|| {
        if KNOWN_TURBOGRAFX_CD_SHA256.contains(&content_sha256) {
            PceConsoleWiring::TurboGrafx16
        } else {
            PceConsoleWiring::PcEngine
        }
    })
}

fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_pce_cd_system_card(
    _config: &BackendLoadConfig,
    cue_path: &Path,
    console_wiring: PceConsoleWiring,
    require_open_fixture: bool,
) -> anyhow::Result<super::firmware::ResolvedFirmwareBytes> {
    #[cfg(test)]
    if let Some(bytes) = _config.pce_cd_system_card_override {
        return Ok(super::firmware::ResolvedFirmwareBytes {
            bytes: bytes.to_vec(),
            sha256: _config
                .pce_cd_system_card_sha256_override
                .unwrap_or_else(|| zeff_firmware::sha256_bytes(bytes)),
            manifest: zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: "nec.pce.cd.system_card".to_owned(),
                variant: Some("test-override".to_owned()),
                sha256: _config
                    .pce_cd_system_card_sha256_override
                    .unwrap_or_else(|| zeff_firmware::sha256_bytes(bytes)),
            },
        });
    }
    super::firmware::resolve_pce_cd_system_card_with_manifest(
        _config.firmware_inventory.as_deref(),
        &_config.firmware_search_dirs,
        Some(cue_path),
        console_wiring,
        require_open_fixture,
    )
}

#[cfg(target_arch = "wasm32")]
fn load_pce_cd_backend(
    _source_path: &Path,
    _cue_path: &Path,
    _preloaded_data: Option<Vec<u8>>,
    _config: &BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    anyhow::bail!("PC Engine CD-ROM2 direct CUE sets are not available in the browser build")
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

fn load_pce_backend(
    rom_data: &[u8],
    source_path: &Path,
    rom_path: &Path,
    config: &BackendLoadConfig,
) -> anyhow::Result<EmuBackend> {
    anyhow::ensure!(
        config.pce_arcade_card_mode != PceArcadeCardMode::Enabled,
        "Arcade Card requires CD media and a System Card v3 environment"
    );
    if rom_data.is_empty() {
        anyhow::bail!("PC Engine HuCard ROM is empty");
    }
    let mut backend = if source_path == rom_path {
        super::PceBackend::new_with_overrides(
            rom_data.to_vec(),
            rom_path.to_path_buf(),
            config.pce_console_wiring,
            config.pce_hucard_board,
        )?
    } else {
        super::PceBackend::with_source_path_and_overrides(
            rom_data.to_vec(),
            rom_path.to_path_buf(),
            source_path.to_path_buf(),
            config.pce_console_wiring,
            config.pce_hucard_board,
        )?
    };
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    if config.pce_load_battery_bram {
        log_sram_result(backend.try_load_memory_base128());
    }
    Ok(EmuBackend::from_pce(backend))
}

fn resolve_fds_bios(
    _config: &BackendLoadConfig,
    rom_path: &Path,
) -> anyhow::Result<super::firmware::ResolvedFirmwareBytes> {
    #[cfg(test)]
    if let Some(bytes) = _config.fds_bios_override {
        return Ok(super::firmware::ResolvedFirmwareBytes {
            bytes: bytes.to_vec(),
            sha256: zeff_firmware::sha256_bytes(bytes),
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
        ActiveSystem::Pce => "PC Engine",
        ActiveSystem::WonderSwan => "WonderSwan",
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => "Sega 8-bit",
    }
}

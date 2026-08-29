use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_pce_core::hardware::{PceArcadeCardMode, PceConsoleWiring, PceHuCardBoard};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::{ActiveSystem, EmuBackend};
mod pce_cd;
mod systems;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use pce_cd::{PreparedNativeArchiveBackend, prepare_native_archive_backend};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use pce_cd::{
    PreparedSevenZipBackend, prepare_pce_cd_7z_backend, prepare_seven_zip_backend,
};

#[derive(Clone, Debug)]
pub(crate) struct BackendLoadConfig {
    pub(crate) gb_hardware_mode_preference: HardwareModePreference,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) apply_mods: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) nes_load_battery_sram: bool,
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
    pub(crate) coleco_bios_override: Option<&'static [u8]>,
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
            nes_load_battery_sram: true,
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
            coleco_bios_override: None,
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

pub(crate) fn load_backend_from_rom_source(
    system: ActiveSystem,
    source_path: &Path,
    rom_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    if system == ActiveSystem::Pce && pce_cd::is_pce_cd_path(rom_path) {
        return pce_cd::load_pce_cd_backend(source_path, rom_path, preloaded_data, &config);
    }
    let mut rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(source_path).with_context(|| {
            format!("Failed to read {} ROM", systems::system_load_label(system))
        })?,
    };

    let original_crc32 = if config.apply_mods {
        systems::apply_mods_if_any(system, &mut rom_data)
    } else {
        crc32fast::hash(&rom_data)
    };

    let default_firmware_manifests =
        super::firmware::default_firmware_manifests_for_active_system(system);
    let mut backend = match system {
        ActiveSystem::GameBoy => {
            systems::load_gb_backend(&rom_data, source_path, rom_path, &config)?
        }
        ActiveSystem::Nes => systems::load_nes_backend(&rom_data, source_path, rom_path, &config)?,
        ActiveSystem::Coleco => {
            systems::load_coleco_backend(&rom_data, source_path, rom_path, &config)?
        }
        ActiveSystem::Pce => systems::load_pce_backend(&rom_data, source_path, rom_path, &config)?,
        ActiveSystem::GameBoyAdvance => {
            systems::load_gba_backend(&rom_data, source_path, rom_path, &config)?
        }
        ActiveSystem::WonderSwan => {
            systems::load_ws_backend(&rom_data, source_path, rom_path, &config)?
        }
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            systems::load_sega8_backend(system, &rom_data, source_path, rom_path, &config)?
        }
    };

    if !default_firmware_manifests.is_empty()
        && !(system == ActiveSystem::GameBoy && config.gb_use_external_boot_rom)
        && !(system == ActiveSystem::GameBoyAdvance && config.gba_use_external_bios)
        && system != ActiveSystem::Coleco
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

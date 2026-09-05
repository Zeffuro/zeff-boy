use std::path::PathBuf;
use std::sync::Arc;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceConsoleWiring, PceControllerMode, PceHuCardBoard,
};
use zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRamIdentity;
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

#[derive(Clone, Debug)]
pub(crate) struct BackendLoadConfig {
    pub(crate) gb_hardware_mode_preference: HardwareModePreference,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) apply_mods: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) gb_tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
    pub(crate) gb_load_battery_sram: bool,
    pub(crate) gb_rtc_time_override: Option<u64>,
    pub(crate) gba_load_battery_sram: bool,
    pub(crate) gba_seed_rtc_from_host: bool,
    pub(crate) nes_load_battery_sram: bool,
    pub(crate) sega8_load_battery_sram: bool,
    pub(crate) ws_load_battery_sram: bool,
    pub(crate) game_gear_standard_mapper_ram_identity: Option<GameGearStandardMapperRamIdentity>,
    pub(crate) sega8_video_standard: Option<Sega8VideoStandard>,
    pub(crate) sega8_console_region: Option<Sega8Region>,
    pub(crate) pce_console_wiring: Option<PceConsoleWiring>,
    pub(crate) pce_hucard_board: Option<PceHuCardBoard>,
    pub(crate) pce_cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    pub(crate) pce_cd_tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
    pub(crate) authenticated_zip_member: Option<crate::rom_archive::AuthenticatedZipMember>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_archive_cue:
        Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_rar_cue:
        Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_zip_cue:
        Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_ppf_stack: Option<crate::emu_backend::pce_cd::PceCdTasPpfStack>,
    pub(crate) pce_controller_mode: PceControllerMode,
    pub(crate) pce_memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode,
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
            gb_tas_source_media: None,
            gb_load_battery_sram: true,
            gb_rtc_time_override: None,
            gba_load_battery_sram: true,
            gba_seed_rtc_from_host: true,
            nes_load_battery_sram: true,
            sega8_load_battery_sram: true,
            ws_load_battery_sram: true,
            game_gear_standard_mapper_ram_identity: None,
            sega8_video_standard: None,
            sega8_console_region: None,
            pce_console_wiring: None,
            pce_hucard_board: None,
            pce_cartridge_hardware: None,
            pce_cd_tas_source_media: None,
            authenticated_zip_member: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_archive_cue: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_rar_cue: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_zip_cue: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_ppf_stack: None,
            pce_controller_mode: PceControllerMode::Automatic,
            pce_memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode::Automatic,
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

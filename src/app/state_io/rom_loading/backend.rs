use super::{ActiveSystem, App, BackendLoadConfig, EmuBackend, Path};
use crate::emu_backend::load_backend_from_rom_source;

pub(in crate::app::state_io) fn direct_netplay_media(system: ActiveSystem, path: &Path) -> bool {
    let extension = match system {
        ActiveSystem::Nes => "nes",
        ActiveSystem::Md => {
            return path.extension().is_some_and(|value| {
                ["md", "gen", "bin"]
                    .iter()
                    .any(|extension| value.eq_ignore_ascii_case(extension))
            });
        }
        ActiveSystem::MasterSystem => "sms",
        ActiveSystem::Sg1000 => "sg",
        ActiveSystem::Pce => "pce",
        ActiveSystem::WonderSwan => {
            return path.extension().is_some_and(|value| {
                value.eq_ignore_ascii_case("ws") || value.eq_ignore_ascii_case("wsc")
            });
        }
        _ => return false,
    };
    path.extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

impl App {
    pub(in crate::app) fn backend_load_config(&self, system: ActiveSystem) -> BackendLoadConfig {
        #[cfg(not(target_arch = "wasm32"))]
        let configured_firmware_dir = self.settings.emulation.firmware_directory_path();
        #[cfg(not(target_arch = "wasm32"))]
        let firmware_inventory = if !self.debug_windows.firmware_inventory.needs_refresh
            && self.debug_windows.firmware_inventory.directory.as_deref()
                == configured_firmware_dir.as_deref()
        {
            self.debug_windows.firmware_inventory.inventory.clone()
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let firmware_inventory = Some(crate::platform::firmware_inventory_snapshot());

        BackendLoadConfig {
            gb_hardware_mode_preference: self.settings.emulation.hardware_mode_preference,
            md_region: self.settings.emulation.md_region.forced_region(),
            md_timing: self.settings.emulation.md_timing.core_model(),
            md_use_tmss: self.settings.emulation.md_use_tmss,
            md_svp_functional_profile: self
                .settings
                .emulation
                .md_svp_functional_profile
                .map(|profile| profile.core_profile()),
            md_controller: self.settings.emulation.md_controller.core_kind(),
            sample_rate: self
                .audio
                .as_ref()
                .map(|audio| audio.emulator_sample_rate()),
            apply_mods: true,
            initial_input: Some(self.host_joypad_input_for_system(system)),
            gb_tas_source_media: None,
            gb_load_battery_sram: true,
            gb_rtc_time_override: None,
            gba_load_battery_sram: true,
            gba_seed_rtc_from_host: true,
            nes_load_battery_sram: true,
            sega8_load_battery_sram: true,
            md_load_save_ram: true,
            md_host_persistence: true,
            #[cfg(target_arch = "wasm32")]
            netplay_browser_media: None,
            pce_netplay: false,
            ws_load_battery_sram: true,
            game_gear_standard_mapper_ram_identity: None,
            sega8_video_standard: self
                .settings
                .emulation
                .sega8_video_standard
                .forced_standard(),
            sega8_console_region: self.settings.emulation.sega8_console_region.forced_region(),
            pce_console_wiring: self.settings.emulation.pce_console_wiring.forced_wiring(),
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
            pce_controller_mode: self.settings.emulation.pce_controller.core_mode(),
            pce_memory_base_mode: self.settings.emulation.pce_memory_base.core_mode(),
            pce_arcade_card_mode: self.settings.emulation.pce_arcade_card.core_mode(),
            pce_cd_archive_memory_limit_mib: self
                .settings
                .emulation
                .pce_cd_archive_memory_limit
                .mib(),
            pce_load_battery_bram: true,
            firmware_search_dirs: self.settings.emulation.firmware_search_dirs(),
            firmware_inventory,
            gb_use_external_boot_rom: matches!(
                self.settings.emulation.gb_boot_rom_mode,
                crate::settings::GbBootRomMode::External
            ),
            gba_use_external_bios: matches!(
                self.settings.emulation.gba_bios_mode,
                crate::settings::GbaBiosMode::External
            ),
            sega8_use_external_boot_rom: matches!(
                self.settings.emulation.sega_boot_rom_mode,
                crate::settings::SegaBootRomMode::External
            ),
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

    pub(in crate::app::state_io) fn init_backend(
        &self,
        system: ActiveSystem,
        path: &Path,
        rom_path: &Path,
        preloaded_data: Option<Vec<u8>>,
        config: BackendLoadConfig,
    ) -> anyhow::Result<(EmuBackend, u32)> {
        #[cfg(target_arch = "wasm32")]
        let mut config = config;
        #[cfg(target_arch = "wasm32")]
        {
            config.netplay_browser_media = preloaded_data.as_ref().and_then(|bytes| {
                direct_netplay_media(system, rom_path)
                    .then(|| crate::emu_backend::loader::NetplayRomMedia::browser(bytes))
                    .flatten()
            });
        }
        let loaded = load_backend_from_rom_source(system, path, rom_path, preloaded_data, config)?;
        Ok((loaded.backend, loaded.original_crc32))
    }
}

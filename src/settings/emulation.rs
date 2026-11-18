use serde::{Deserialize, Serialize};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::md::{
    MdControllerPreference, MdRegionPreference, MdSvpFunctionalProfile, MdTimingPreference,
};
use super::structs::{
    GbBootRomMode, GbaBiosMode, PceArcadeCardPreference, PceCdArchiveMemoryLimit,
    PceConsoleWiringPreference, PceControllerPreference, PceMemoryBasePreference,
    PceMouseCursorMode, Sega8ConsoleRegionPreference, Sega8VideoStandardPreference,
    SegaBootRomMode,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct EmulationSettings {
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) md_region: MdRegionPreference,
    pub(crate) md_timing: MdTimingPreference,
    pub(crate) md_controller: MdControllerPreference,
    pub(crate) md_use_tmss: bool,
    pub(crate) md_svp_functional_profile: Option<MdSvpFunctionalProfile>,
    #[serde(default)]
    pub(crate) sega8_video_standard: Sega8VideoStandardPreference,
    #[serde(default)]
    pub(crate) sega8_console_region: Sega8ConsoleRegionPreference,
    #[serde(default)]
    pub(crate) pce_console_wiring: PceConsoleWiringPreference,
    #[serde(default)]
    pub(crate) pce_controller: PceControllerPreference,
    #[serde(default)]
    pub(crate) pce_memory_base: PceMemoryBasePreference,
    #[serde(default)]
    pub(crate) pce_arcade_card: PceArcadeCardPreference,
    #[serde(default = "default_pce_mouse_sensitivity")]
    pub(crate) pce_mouse_sensitivity: f32,
    #[serde(default)]
    pub(crate) pce_mouse_cursor_mode: PceMouseCursorMode,
    #[serde(default)]
    pub(crate) pce_cd_archive_memory_limit: PceCdArchiveMemoryLimit,
    pub(crate) fast_forward_multiplier: usize,
    #[serde(default = "default_slow_motion_divisor")]
    pub(crate) slow_motion_divisor: usize,
    #[serde(default)]
    pub(crate) slow_motion_enabled: bool,
    pub(crate) uncapped_frames_per_tick: usize,
    pub(crate) uncapped_speed: bool,
    pub(crate) frame_skip: bool,
    pub(crate) save_recovery_state: bool,
    pub(crate) resume_recovery_state: bool,
    pub(crate) recovery_migration_notice_pending: bool,
    #[serde(default = "default_tcp_link_addr")]
    pub(crate) tcp_link_addr: String,
    #[serde(default)]
    pub(crate) sgb_border_enabled: bool,
    #[serde(default)]
    pub(crate) nes_zapper_enabled: bool,
    #[serde(default = "default_pause_on_unfocus")]
    pub(crate) pause_on_unfocus: bool,
    #[serde(default)]
    pub(crate) firmware_directory: String,
    #[serde(default)]
    pub(crate) gb_boot_rom_mode: GbBootRomMode,
    #[serde(default)]
    pub(crate) sega_boot_rom_mode: SegaBootRomMode,
    #[serde(default)]
    pub(crate) gba_bios_mode: GbaBiosMode,
}

fn default_slow_motion_divisor() -> usize {
    4
}

fn default_pce_mouse_sensitivity() -> f32 {
    1.25
}

fn default_pause_on_unfocus() -> bool {
    true
}

fn default_tcp_link_addr() -> String {
    "127.0.0.1:8765".to_string()
}

impl Default for EmulationSettings {
    fn default() -> Self {
        Self {
            hardware_mode_preference: HardwareModePreference::Auto,
            md_region: MdRegionPreference::Auto,
            md_timing: MdTimingPreference::Standard,
            md_controller: MdControllerPreference::SixButton,
            md_use_tmss: false,
            md_svp_functional_profile: None,
            sega8_video_standard: Sega8VideoStandardPreference::Auto,
            sega8_console_region: Sega8ConsoleRegionPreference::Auto,
            pce_console_wiring: PceConsoleWiringPreference::Auto,
            pce_controller: PceControllerPreference::Auto,
            pce_memory_base: PceMemoryBasePreference::Auto,
            pce_arcade_card: PceArcadeCardPreference::Auto,
            pce_mouse_sensitivity: default_pce_mouse_sensitivity(),
            pce_mouse_cursor_mode: PceMouseCursorMode::default(),
            pce_cd_archive_memory_limit: PceCdArchiveMemoryLimit::default(),
            fast_forward_multiplier: 4,
            slow_motion_divisor: default_slow_motion_divisor(),
            slow_motion_enabled: false,
            uncapped_frames_per_tick: 60,
            uncapped_speed: false,
            frame_skip: false,
            save_recovery_state: true,
            resume_recovery_state: false,
            recovery_migration_notice_pending: false,
            tcp_link_addr: default_tcp_link_addr(),
            sgb_border_enabled: false,
            nes_zapper_enabled: false,
            pause_on_unfocus: true,
            firmware_directory: String::new(),
            gb_boot_rom_mode: GbBootRomMode::Skip,
            sega_boot_rom_mode: SegaBootRomMode::Skip,
            gba_bios_mode: GbaBiosMode::Hle,
        }
    }
}

impl EmulationSettings {
    pub(crate) fn firmware_directory_path(&self) -> Option<std::path::PathBuf> {
        let path = self.firmware_directory.trim();
        if path.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(path))
        }
    }

    pub(crate) fn firmware_search_dirs(&self) -> Vec<std::path::PathBuf> {
        #[cfg(target_arch = "wasm32")]
        {
            Vec::new()
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            const LEGACY_ENV_VAR: &str = "ZEFF_FIRMWARE_DIR";

            self.firmware_search_dirs_with_roots(
                crate::platform::managed_firmware_dir(),
                std::env::var_os(LEGACY_ENV_VAR).map(std::path::PathBuf::from),
            )
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn firmware_search_dirs_with_roots(
        &self,
        managed_dir: std::path::PathBuf,
        legacy_env_dir: Option<std::path::PathBuf>,
    ) -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();
        push_unique_firmware_dir(&mut dirs, managed_dir);
        if let Some(path) = self.firmware_directory_path() {
            push_unique_firmware_dir(&mut dirs, path);
        }
        if let Some(path) = legacy_env_dir {
            push_unique_firmware_dir(&mut dirs, path);
        }
        dirs
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn push_unique_firmware_dir(dirs: &mut Vec<std::path::PathBuf>, path: std::path::PathBuf) {
    if path.as_os_str().is_empty() {
        return;
    }
    if dirs.iter().any(|existing| existing == &path) {
        return;
    }
    dirs.push(path);
}

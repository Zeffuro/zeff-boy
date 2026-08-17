use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::enums::{
    AudioRecordingFormat, ColorCorrection, DebugPresentation, DmgPalettePreset, EffectPreset,
    EffectiveColorCorrection, GbaColorCorrection, NesPaletteMode, ShaderParams, ShaderPreset,
    UiDensity, UiThemePreset, VsyncMode, WonderSwanColorCorrection,
};
use super::keycode_serde::keycode_from_string;
use super::tilt_bindings::TiltKeyBindings;
use super::{
    LeftStickMode, TiltInputMode, default_color_correction_matrix, default_offscreen_scale,
    effective_gb_color_correction, effective_gba_color_correction,
    effective_wonderswan_color_correction,
};

fn default_camera_gamma() -> f32 {
    1.05
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct DebugColors {
    pub(crate) address: [u8; 4],
    pub(crate) opcode: [u8; 4],
    pub(crate) mnemonic: [u8; 4],
    pub(crate) symbol: [u8; 4],
    pub(crate) source: [u8; 4],
    pub(crate) pc: [u8; 4],
    pub(crate) changed: [u8; 4],
    pub(crate) breakpoint: [u8; 4],
    pub(crate) watchpoint: [u8; 4],
    pub(crate) selection: [u8; 4],
    pub(crate) interrupt: [u8; 4],
}

impl Default for DebugColors {
    fn default() -> Self {
        Self {
            address: [140, 140, 170, 255],
            opcode: [145, 155, 175, 255],
            mnemonic: [205, 205, 220, 255],
            symbol: [110, 190, 140, 255],
            source: [155, 135, 190, 255],
            pc: [63, 48, 82, 255],
            changed: [255, 100, 80, 255],
            breakpoint: [255, 80, 80, 255],
            watchpoint: [255, 180, 60, 255],
            selection: [145, 105, 220, 255],
            interrupt: [80, 190, 220, 255],
        }
    }
}

impl DebugColors {
    pub(crate) fn for_theme(theme: UiThemePreset) -> Self {
        match theme {
            UiThemePreset::DefaultDark => Self::default(),
            UiThemePreset::HighContrastDark => Self {
                address: [185, 195, 235, 255],
                opcode: [195, 205, 225, 255],
                mnemonic: [245, 245, 250, 255],
                symbol: [120, 235, 160, 255],
                source: [210, 170, 245, 255],
                pc: [82, 62, 108, 255],
                changed: [255, 145, 80, 255],
                breakpoint: [255, 95, 105, 255],
                watchpoint: [255, 210, 80, 255],
                selection: [180, 135, 255, 255],
                interrupt: [95, 220, 255, 255],
            },
            UiThemePreset::Light => Self {
                address: [68, 72, 112, 255],
                opcode: [82, 88, 108, 255],
                mnemonic: [28, 30, 38, 255],
                symbol: [20, 112, 62, 255],
                source: [102, 54, 132, 255],
                pc: [225, 212, 244, 255],
                changed: [190, 58, 28, 255],
                breakpoint: [190, 32, 42, 255],
                watchpoint: [150, 92, 0, 255],
                selection: [112, 65, 175, 255],
                interrupt: [0, 112, 150, 255],
            },
            UiThemePreset::Retro => Self {
                address: [100, 220, 190, 255],
                opcode: [130, 190, 170, 255],
                mnemonic: [200, 245, 220, 255],
                symbol: [245, 220, 95, 255],
                source: [130, 205, 245, 255],
                pc: [35, 88, 72, 255],
                changed: [255, 145, 75, 255],
                breakpoint: [255, 90, 90, 255],
                watchpoint: [250, 210, 80, 255],
                selection: [245, 105, 220, 255],
                interrupt: [80, 225, 245, 255],
            },
        }
    }
}

fn default_camera_contrast() -> f32 {
    1.65
}

fn default_camera_brightness() -> f32 {
    0.15
}

fn default_output_sample_rate() -> u32 {
    48_000
}

fn default_audio_low_pass_cutoff_hz() -> u32 {
    4_800
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct CameraSettings {
    #[serde(rename = "camera_device_index")]
    pub(crate) device_index: u32,
    #[serde(rename = "camera_auto_levels")]
    pub(crate) auto_levels: bool,
    #[serde(rename = "camera_gamma", default = "default_camera_gamma")]
    pub(crate) gamma: f32,
    #[serde(rename = "camera_brightness", default = "default_camera_brightness")]
    pub(crate) brightness: f32,
    #[serde(rename = "camera_contrast", default = "default_camera_contrast")]
    pub(crate) contrast: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            device_index: 0,
            auto_levels: false,
            gamma: default_camera_gamma(),
            brightness: default_camera_brightness(),
            contrast: default_camera_contrast(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct TiltSettings {
    #[serde(rename = "tilt_key_bindings")]
    pub(crate) key_bindings: TiltKeyBindings,
    #[serde(rename = "left_stick_mode")]
    pub(crate) left_stick_mode: LeftStickMode,
    #[serde(rename = "tilt_input_mode")]
    pub(crate) input_mode: TiltInputMode,
    #[serde(rename = "tilt_sensitivity")]
    pub(crate) sensitivity: f32,
    #[serde(rename = "tilt_lerp")]
    pub(crate) lerp: f32,
    #[serde(rename = "tilt_deadzone")]
    pub(crate) deadzone: f32,
    #[serde(rename = "tilt_invert_x")]
    pub(crate) invert_x: bool,
    #[serde(rename = "tilt_invert_y")]
    pub(crate) invert_y: bool,
    #[serde(rename = "stick_tilt_bypass_lerp")]
    pub(crate) stick_bypass_lerp: bool,
}

impl Default for TiltSettings {
    fn default() -> Self {
        Self {
            key_bindings: TiltKeyBindings::default(),
            left_stick_mode: LeftStickMode::Auto,
            input_mode: TiltInputMode::default(),
            sensitivity: 1.0,
            lerp: 0.25,
            deadzone: 0.12,
            invert_x: false,
            invert_y: false,
            stick_bypass_lerp: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct AudioSettings {
    #[serde(rename = "master_volume")]
    pub(crate) volume: f32,
    #[serde(skip)]
    pub(crate) pre_mute_volume: Option<f32>,
    #[serde(rename = "mute_audio_during_fast_forward")]
    pub(crate) mute_during_fast_forward: bool,
    #[serde(rename = "audio_recording_format", default)]
    pub(crate) recording_format: AudioRecordingFormat,
    #[serde(
        rename = "audio_output_sample_rate",
        default = "default_output_sample_rate"
    )]
    pub(crate) output_sample_rate: u32,
    #[serde(rename = "audio_low_pass_enabled", default)]
    pub(crate) low_pass_enabled: bool,
    #[serde(
        rename = "audio_low_pass_cutoff_hz",
        default = "default_audio_low_pass_cutoff_hz"
    )]
    pub(crate) low_pass_cutoff_hz: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pre_mute_volume: None,
            mute_during_fast_forward: false,
            recording_format: AudioRecordingFormat::default(),
            output_sample_rate: default_output_sample_rate(),
            low_pass_enabled: false,
            low_pass_cutoff_hz: default_audio_low_pass_cutoff_hz(),
        }
    }
}

pub(super) fn default_rewind_speed() -> usize {
    3
}
pub(super) fn default_rewind_seconds() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct RewindSettings {
    #[serde(rename = "rewind_enabled")]
    pub(crate) enabled: bool,
    #[serde(rename = "rewind_key")]
    pub(crate) key: String,
    #[serde(rename = "rewind_speed", default = "default_rewind_speed")]
    pub(crate) speed: usize,
    #[serde(rename = "rewind_seconds", default = "default_rewind_seconds")]
    pub(crate) seconds: usize,
}

impl Default for RewindSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            key: "KeyR".to_string(),
            speed: default_rewind_speed(),
            seconds: default_rewind_seconds(),
        }
    }
}

impl RewindSettings {
    pub(crate) fn key_code(&self) -> KeyCode {
        keycode_from_string(&self.key).unwrap_or(KeyCode::KeyR)
    }

    pub(crate) fn capture_interval(&self) -> usize {
        4
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct VideoSettings {
    pub(crate) shader_preset: ShaderPreset,
    pub(crate) scaling_mode: ScalingMode,
    pub(crate) effect_preset: EffectPreset,
    #[serde(default = "default_offscreen_scale")]
    pub(crate) offscreen_scale: u32,
    pub(crate) shader_params: ShaderParams,
    pub(crate) custom_shader_path: String,

    #[serde(rename = "gb_color_correction", alias = "color_correction")]
    pub(crate) gb_color_correction: ColorCorrection,
    #[serde(default = "default_color_correction_matrix")]
    #[serde(
        rename = "gb_color_correction_matrix",
        alias = "color_correction_matrix"
    )]
    pub(crate) gb_color_correction_matrix: [f32; 9],
    #[serde(default)]
    #[serde(rename = "gb_dmg_palette_preset", alias = "dmg_palette_preset")]
    pub(crate) gb_dmg_palette_preset: DmgPalettePreset,

    #[serde(default)]
    pub(crate) gba_color_correction: GbaColorCorrection,
    #[serde(default = "default_color_correction_matrix")]
    pub(crate) gba_color_correction_matrix: [f32; 9],

    #[serde(default)]
    pub(crate) ws_color_correction: WonderSwanColorCorrection,
    #[serde(default = "default_color_correction_matrix")]
    pub(crate) ws_color_correction_matrix: [f32; 9],

    #[serde(default)]
    pub(crate) nes_palette_mode: NesPaletteMode,
    #[serde(default)]
    pub(crate) nes_custom_palette_path: String,
    #[serde(default)]
    pub(crate) nes_custom_palette_name: String,
    #[serde(default)]
    pub(crate) nes_custom_palette_bytes: Vec<u8>,
    pub(crate) vsync_mode: VsyncMode,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            shader_preset: ShaderPreset::None,
            scaling_mode: ScalingMode::PixelPerfect,
            effect_preset: EffectPreset::None,
            offscreen_scale: default_offscreen_scale(),
            shader_params: ShaderParams::default(),
            custom_shader_path: String::new(),
            gb_color_correction: ColorCorrection::None,
            gb_color_correction_matrix: default_color_correction_matrix(),
            gb_dmg_palette_preset: DmgPalettePreset::default(),
            gba_color_correction: GbaColorCorrection::None,
            gba_color_correction_matrix: default_color_correction_matrix(),
            ws_color_correction: WonderSwanColorCorrection::None,
            ws_color_correction_matrix: default_color_correction_matrix(),
            nes_palette_mode: NesPaletteMode::default(),
            nes_custom_palette_path: String::new(),
            nes_custom_palette_name: String::new(),
            nes_custom_palette_bytes: Vec::new(),
            vsync_mode: VsyncMode::default(),
        }
    }
}

impl VideoSettings {
    pub(crate) fn migrate_shader_preset(&mut self) {
        if self.shader_preset != ShaderPreset::None
            && self.scaling_mode == ScalingMode::PixelPerfect
            && self.effect_preset == EffectPreset::None
        {
            let (scaling, effect) = self.shader_preset.to_scaling_and_effect();
            self.scaling_mode = scaling;
            self.effect_preset = effect;
        }
    }

    pub(crate) fn effective_color_correction(
        &self,
        active_system: Option<crate::emu_backend::ActiveSystem>,
    ) -> EffectiveColorCorrection {
        match active_system {
            Some(crate::emu_backend::ActiveSystem::GameBoy) => effective_gb_color_correction(
                self.gb_color_correction,
                self.gb_color_correction_matrix,
            ),
            Some(crate::emu_backend::ActiveSystem::GameBoyAdvance) => {
                effective_gba_color_correction(
                    self.gba_color_correction,
                    self.gba_color_correction_matrix,
                )
            }
            Some(crate::emu_backend::ActiveSystem::WonderSwan) => {
                effective_wonderswan_color_correction(
                    self.ws_color_correction,
                    self.ws_color_correction_matrix,
                )
            }
            Some(
                crate::emu_backend::ActiveSystem::Nes
                | crate::emu_backend::ActiveSystem::MasterSystem
                | crate::emu_backend::ActiveSystem::GameGear
                | crate::emu_backend::ActiveSystem::Sg1000,
            )
            | None => EffectiveColorCorrection::None,
        }
    }
}

use super::enums::ScalingMode;

fn default_ui_scale() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_debugger_window_size() -> [u32; 2] {
    [1100, 760]
}

fn default_settings_window_size() -> [u32; 2] {
    [760, 680]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct UiSettings {
    pub(crate) show_fps: bool,
    pub(crate) enable_memory_editing: bool,
    #[serde(default)]
    pub(crate) autohide_menu_bar: bool,
    #[serde(default = "default_ui_scale")]
    pub(crate) ui_scale: f32,
    #[serde(skip)]
    pub(crate) ui_scale_needs_auto: bool,
    pub(crate) open_debug_tabs: Vec<String>,
    #[serde(default)]
    pub(crate) theme_preset: UiThemePreset,
    #[serde(default)]
    pub(crate) ui_density: UiDensity,
    #[serde(default = "default_debug_monospace_scale")]
    pub(crate) debug_monospace_scale: f32,
    #[serde(default)]
    pub(crate) debug_colors: DebugColors,
    #[serde(default)]
    pub(crate) debug_presentation: DebugPresentation,
    #[serde(default = "default_true")]
    pub(crate) debugger_window_open: bool,
    #[serde(default)]
    pub(crate) debugger_window_position: Option<[i32; 2]>,
    #[serde(default = "default_debugger_window_size")]
    pub(crate) debugger_window_size: [u32; 2],
    #[serde(default)]
    pub(crate) debugger_window_maximized: bool,
    #[serde(default)]
    pub(crate) settings_window_position: Option<[i32; 2]>,
    #[serde(default = "default_settings_window_size")]
    pub(crate) settings_window_size: [u32; 2],
    #[serde(default)]
    pub(crate) settings_window_maximized: bool,
    #[serde(default)]
    pub(crate) debugger_dock_layout: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) floating_dock_layout: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) ide_dock_layout: Option<serde_json::Value>,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            show_fps: true,
            enable_memory_editing: false,
            autohide_menu_bar: false,
            ui_scale: default_ui_scale(),
            ui_scale_needs_auto: false,
            open_debug_tabs: Vec::new(),
            theme_preset: UiThemePreset::default(),
            ui_density: UiDensity::default(),
            debug_monospace_scale: default_debug_monospace_scale(),
            debug_colors: DebugColors::default(),
            debug_presentation: DebugPresentation::default(),
            debugger_window_open: true,
            debugger_window_position: None,
            debugger_window_size: default_debugger_window_size(),
            debugger_window_maximized: false,
            settings_window_position: None,
            settings_window_size: default_settings_window_size(),
            settings_window_maximized: false,
            debugger_dock_layout: None,
            floating_dock_layout: None,
            ide_dock_layout: None,
        }
    }
}

fn default_debug_monospace_scale() -> f32 {
    1.0
}

impl UiSettings {
    pub(crate) fn effective_debug_colors(&self) -> DebugColors {
        if self.debug_colors == DebugColors::default()
            && self.theme_preset != UiThemePreset::DefaultDark
        {
            DebugColors::for_theme(self.theme_preset)
        } else {
            self.debug_colors
        }
    }

    pub(crate) fn dock_layout(
        &self,
        presentation: DebugPresentation,
    ) -> Option<&serde_json::Value> {
        match presentation {
            DebugPresentation::GameAndDebugger => self.debugger_dock_layout.as_ref(),
            DebugPresentation::Floating => self.floating_dock_layout.as_ref(),
            DebugPresentation::Ide => self.ide_dock_layout.as_ref(),
        }
    }

    pub(crate) fn set_dock_layout(
        &mut self,
        presentation: DebugPresentation,
        layout: serde_json::Value,
    ) {
        match presentation {
            DebugPresentation::GameAndDebugger => self.debugger_dock_layout = Some(layout),
            DebugPresentation::Floating => self.floating_dock_layout = Some(layout),
            DebugPresentation::Ide => self.ide_dock_layout = Some(layout),
        }
    }
}

impl UiSettings {
    pub(crate) fn auto_detect_ui_scale(&mut self, monitor_height: u32, os_scale_factor: f64) {
        if !self.ui_scale_needs_auto {
            return;
        }
        self.ui_scale_needs_auto = false;

        if os_scale_factor > 1.1 {
            self.ui_scale = 1.0;
            return;
        }

        self.ui_scale = match monitor_height {
            0..=900 => 1.0,
            901..=1600 => 1.0,
            _ => 1.25,
        };
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum Sega8VideoStandardPreference {
    #[default]
    Auto,
    Ntsc,
    Pal,
}

impl Sega8VideoStandardPreference {
    pub(crate) fn forced_standard(self) -> Option<Sega8VideoStandard> {
        match self {
            Self::Auto => None,
            Self::Ntsc => Some(Sega8VideoStandard::Ntsc),
            Self::Pal => Some(Sega8VideoStandard::Pal),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum Sega8ConsoleRegionPreference {
    #[default]
    Auto,
    Export,
    Japanese,
    JapanesePowerBaseConverter,
}

impl Sega8ConsoleRegionPreference {
    pub(crate) fn forced_region(self) -> Option<Sega8Region> {
        match self {
            Self::Auto => None,
            Self::Export => Some(Sega8Region::Export),
            Self::Japanese => Some(Sega8Region::Japanese),
            Self::JapanesePowerBaseConverter => Some(Sega8Region::JapanesePowerBaseConverter),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct EmulationSettings {
    pub(crate) hardware_mode_preference: HardwareModePreference,
    #[serde(default)]
    pub(crate) sega8_video_standard: Sega8VideoStandardPreference,
    #[serde(default)]
    pub(crate) sega8_console_region: Sega8ConsoleRegionPreference,
    pub(crate) fast_forward_multiplier: usize,
    #[serde(default = "default_slow_motion_divisor")]
    pub(crate) slow_motion_divisor: usize,
    #[serde(default)]
    pub(crate) slow_motion_enabled: bool,
    pub(crate) uncapped_frames_per_tick: usize,
    pub(crate) uncapped_speed: bool,
    pub(crate) frame_skip: bool,
    pub(crate) auto_save_state: bool,
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
}

fn default_slow_motion_divisor() -> usize {
    4
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
            sega8_video_standard: Sega8VideoStandardPreference::Auto,
            sega8_console_region: Sega8ConsoleRegionPreference::Auto,
            fast_forward_multiplier: 4,
            slow_motion_divisor: default_slow_motion_divisor(),
            slow_motion_enabled: false,
            uncapped_frames_per_tick: 60,
            uncapped_speed: false,
            frame_skip: false,
            auto_save_state: false,
            tcp_link_addr: default_tcp_link_addr(),
            sgb_border_enabled: false,
            nes_zapper_enabled: false,
            pause_on_unfocus: true,
            firmware_directory: String::new(),
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

            self.firmware_search_dirs_with_env(
                std::env::var_os(LEGACY_ENV_VAR).map(std::path::PathBuf::from),
            )
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn firmware_search_dirs_with_env(
        &self,
        legacy_env_dir: Option<std::path::PathBuf>,
    ) -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecentRomEntry {
    pub(crate) path: String,
    pub(crate) name: String,
}

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::enums::{
    AudioRecordingFormat, ColorCorrection, DmgPalettePreset, EffectPreset, NesPaletteMode,
    ShaderParams, ShaderPreset, UiThemePreset, VsyncMode,
};
use super::keycode_serde::keycode_from_string;
use super::tilt_bindings::TiltKeyBindings;
use super::{
    LeftStickMode, TiltInputMode, default_color_correction_matrix, default_offscreen_scale,
};

fn default_camera_gamma() -> f32 {
    1.05
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
    pub(crate) color_correction: ColorCorrection,
    #[serde(default = "default_color_correction_matrix")]
    pub(crate) color_correction_matrix: [f32; 9],
    #[serde(default)]
    pub(crate) dmg_palette_preset: DmgPalettePreset,
    #[serde(default)]
    pub(crate) nes_palette_mode: NesPaletteMode,
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
            color_correction: ColorCorrection::None,
            color_correction_matrix: default_color_correction_matrix(),
            dmg_palette_preset: DmgPalettePreset::default(),
            nes_palette_mode: NesPaletteMode::default(),
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
}

use super::enums::ScalingMode;

fn default_ui_scale() -> f32 {
    1.0
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct EmulationSettings {
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) fast_forward_multiplier: usize,
    pub(crate) uncapped_frames_per_tick: usize,
    pub(crate) uncapped_speed: bool,
    pub(crate) frame_skip: bool,
    pub(crate) auto_save_state: bool,
    #[serde(default)]
    pub(crate) sgb_border_enabled: bool,
    #[serde(default)]
    pub(crate) nes_zapper_enabled: bool,
}

impl Default for EmulationSettings {
    fn default() -> Self {
        Self {
            hardware_mode_preference: HardwareModePreference::Auto,
            fast_forward_multiplier: 4,
            uncapped_frames_per_tick: 60,
            uncapped_speed: false,
            frame_skip: false,
            auto_save_state: false,
            sgb_border_enabled: false,
            nes_zapper_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecentRomEntry {
    pub(crate) path: String,
    pub(crate) name: String,
}

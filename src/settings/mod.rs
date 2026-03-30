mod binding_actions;
mod enums;
mod gamepad;
mod keyboard_bindings;
mod keycode_serde;
mod shortcuts;
mod tilt_bindings;

pub(crate) use enums::{
    build_gpu_params, default_color_correction_matrix, default_offscreen_scale,
    AudioRecordingFormat, ColorCorrection, EffectPreset, LeftStickMode, ScalingMode, ShaderParams,
    ShaderPreset, TiltInputMode, VsyncMode,
};
pub(crate) use binding_actions::{
    BindingAction, InputBindingAction,
};
pub(crate) use gamepad::{GamepadAction, GamepadBindings};
pub(crate) use keyboard_bindings::KeyBindings;
pub(crate) use shortcuts::{ShortcutAction, ShortcutBindings};
pub(crate) use tilt_bindings::{TiltBindingAction, TiltKeyBindings};
pub(crate) use keycode_serde::keycode_from_string;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

const MAX_RECENT_ROMS: usize = 10;

fn default_camera_gamma() -> f32 {
    1.05
}

fn default_camera_contrast() -> f32 {
    1.65
}

fn default_camera_brightness() -> f32 {
    0.15
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
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pre_mute_volume: None,
            mute_during_fast_forward: false,
            recording_format: AudioRecordingFormat::default(),
        }
    }
}

// --- RewindSettings ---

fn default_rewind_speed() -> usize {
    3
}
fn default_rewind_seconds() -> usize {
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecentRomEntry {
    pub(crate) path: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    #[serde(flatten)]
    pub(crate) emulation: EmulationSettings,
    #[serde(flatten)]
    pub(crate) ui: UiSettings,
    pub(crate) key_bindings: KeyBindings,
    #[serde(flatten)]
    pub(crate) tilt: TiltSettings,
    #[serde(flatten)]
    pub(crate) audio: AudioSettings,
    pub(crate) recent_roms: Vec<RecentRomEntry>,
    pub(crate) speedup_key: String,
    #[serde(flatten)]
    pub(crate) rewind: RewindSettings,
    #[serde(flatten)]
    pub(crate) video: VideoSettings,
    #[serde(default)]
    pub(crate) shortcut_bindings: ShortcutBindings,
    #[serde(default)]
    pub(crate) gamepad_bindings: GamepadBindings,
    #[serde(flatten)]
    pub(crate) camera: CameraSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            emulation: EmulationSettings::default(),
            ui: UiSettings::default(),
            key_bindings: KeyBindings::default(),
            tilt: TiltSettings::default(),
            audio: AudioSettings::default(),
            recent_roms: Vec::new(),
            speedup_key: "Space".to_string(),
            rewind: RewindSettings::default(),
            video: VideoSettings::default(),
            shortcut_bindings: ShortcutBindings::default(),
            gamepad_bindings: GamepadBindings::default(),
            camera: CameraSettings::default(),
        }
    }
}

impl Settings {
    pub(crate) fn speedup_key_code(&self) -> KeyCode {
        keycode_from_string(&self.speedup_key).unwrap_or(KeyCode::Backquote)
    }

    pub(crate) fn add_recent_rom(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        self.recent_roms.retain(|r| r.path != path_str);

        self.recent_roms.insert(
            0,
            RecentRomEntry {
                path: path_str,
                name,
            },
        );

        self.recent_roms.truncate(MAX_RECENT_ROMS);
    }

    fn legacy_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("settings.json")
    }

    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|base| base.join("zeff-boy").join("settings.json"))
    }

    fn active_path() -> PathBuf {
        Self::config_path().unwrap_or_else(Self::legacy_path)
    }

    pub(crate) fn settings_dir() -> PathBuf {
        Self::active_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn load_from_path(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice::<Self>(&bytes).ok()
    }

    fn save_to_path(&self, path: &Path) {
        let Ok(serialized) = serde_json::to_vec_pretty(self) else {
            log::error!("failed to serialize settings");
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            log::error!("failed to create settings directory {}: {e}", parent.display());
            return;
        }
        if let Err(e) = fs::write(path, serialized) {
            log::error!("failed to write settings to {}: {e}", path.display());
        }
    }

    pub(crate) fn load_or_default() -> Self {
        let mut settings = if let Some(config_path) = Self::config_path() {
            if let Some(settings) = Self::load_from_path(&config_path) {
                settings
            } else {
                let legacy_path = Self::legacy_path();
                if let Some(settings) = Self::load_from_path(&legacy_path) {
                    settings.save_to_path(&config_path);
                    settings
                } else {
                    Settings { ui: UiSettings { ui_scale_needs_auto: true, ..Default::default() }, ..Default::default() }
                }
            }
        } else {
            Self::load_from_path(&Self::legacy_path()).unwrap_or_else(|| {
                Settings { ui: UiSettings { ui_scale_needs_auto: true, ..Default::default() }, ..Default::default() }
            })
        };

        settings.video.migrate_shader_preset();
        settings
    }

    pub(crate) fn auto_detect_ui_scale(&mut self, monitor_height: u32, os_scale_factor: f64) {
        self.ui.auto_detect_ui_scale(monitor_height, os_scale_factor);
    }


    pub(crate) fn save(&self) {
        self.save_to_path(&Self::active_path());
    }
}

#[cfg(test)]
mod tests;
